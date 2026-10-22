//! Token handling: a redacting, zeroizing [`Secret`] and a non-verifying JWT
//! payload reader used only to show the user what their token can do.
//!
//! Nothing here ever prints, logs or serializes the token itself.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use zeroize::Zeroize;

/// An API credential. `Debug`/`Display` are redacted and the buffer is
/// zeroized on drop. Deliberately not `Clone`: share it through `Arc`.
pub struct Secret(String);

impl Secret {
    pub fn new(s: String) -> Self {
        Self(s)
    }

    /// The raw token, only for building the `Authorization` header and for the
    /// keychain write.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// `Authorization` header value: `Bearer <jwt>` for a JWT, the bare value
    /// for a legacy API secret.
    pub fn auth_header(&self) -> Secret {
        if looks_like_jwt(&self.0) {
            Secret(format!("Bearer {}", self.0))
        } else {
            Secret(self.0.clone())
        }
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Decoded (NOT verified) JWT claims we surface in the UI.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JwtInfo {
    pub aud: Vec<String>,
    pub permissions: Vec<String>,
    pub exp: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenInfo {
    Empty,
    Jwt(JwtInfo),
    /// Not a JWT: treated as a legacy, long-lived API secret.
    Opaque,
}

/// Classify a token. A JWT is three base64url segments whose header decodes to
/// a JSON object with `alg`; the payload is decoded without verifying anything.
pub fn analyze(token: &str) -> TokenInfo {
    let token = token.trim();
    if token.is_empty() {
        return TokenInfo::Empty;
    }
    match decode_jwt(token) {
        Some(info) => TokenInfo::Jwt(info),
        None => TokenInfo::Opaque,
    }
}

fn looks_like_jwt(token: &str) -> bool {
    decode_jwt(token.trim()).is_some()
}

fn decode_jwt(token: &str) -> Option<JwtInfo> {
    let mut parts = token.split('.');
    let (h, p, _sig) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let header: Value = serde_json::from_slice(&b64url_decode(h)?).ok()?;
    header.get("alg")?;
    let payload: Value = serde_json::from_slice(&b64url_decode(p)?).ok()?;
    let strings = |v: Option<&Value>| -> Vec<String> {
        match v {
            Some(Value::String(s)) => vec![s.clone()],
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect(),
            _ => Vec::new(),
        }
    };
    Some(JwtInfo {
        aud: strings(payload.get("aud")),
        permissions: strings(payload.get("permissions")),
        exp: payload
            .get("exp")
            .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))),
    })
}

/// base64url (RFC 4648 §5), padding optional. `None` on any invalid byte.
pub fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        } as u32)
    }
    let bytes = s.trim_end_matches('=').as_bytes();
    if bytes.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            acc |= val(c)? << (18 - 6 * i);
        }
        let n = chunk.len() - 1; // decoded bytes in this chunk
        for i in 0..n {
            out.push((acc >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// How the UI should present a token's lifetime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expiry {
    Missing,
    Expired,
    /// Seconds left.
    In(i64),
}

impl JwtInfo {
    pub fn expiry(&self, now: i64) -> Expiry {
        match self.exp {
            None => Expiry::Missing,
            Some(e) if e <= now => Expiry::Expired,
            Some(e) => Expiry::In(e - now),
        }
    }

    pub fn has_api_permission(&self) -> bool {
        self.permissions
            .iter()
            .any(|p| p == "Documents:Api:All" || p == "*" || p == "Documents:*")
    }
}

pub const LONG_LIVED_SECS: i64 = 24 * 3600;

/// `14 min`, `3 h`, `2 days`.
pub fn fmt_duration(secs: i64) -> String {
    if secs < 90 {
        format!("{secs} s")
    } else if secs < 90 * 60 {
        format!("{} min", (secs + 30) / 60)
    } else if secs < 48 * 3600 {
        format!("{} h", (secs + 1800) / 3600)
    } else {
        format!("{} days", secs / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64url(bytes: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut s = String::new();
        for c in bytes.chunks(3) {
            let n = (c[0] as u32) << 16
                | (*c.get(1).unwrap_or(&0) as u32) << 8
                | *c.get(2).unwrap_or(&0) as u32;
            for i in 0..=c.len() {
                s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
        }
        s
    }

    fn jwt(payload: &str) -> String {
        format!(
            "{}.{}.sig",
            b64url(br#"{"alg":"HS256","typ":"JWT"}"#),
            b64url(payload.as_bytes())
        )
    }

    #[test]
    fn b64url_roundtrip() {
        for s in [
            "",
            "f",
            "fo",
            "foo",
            "foob",
            "fooba",
            "foobar",
            "\u{1F600}?>",
        ] {
            assert_eq!(b64url_decode(&b64url(s.as_bytes())).unwrap(), s.as_bytes());
        }
        assert_eq!(b64url_decode("Zm9vYg==").unwrap(), b"foob");
        assert!(b64url_decode("a").is_none());
        assert!(b64url_decode("ab$d").is_none());
    }

    #[test]
    fn decodes_jwt_claims() {
        let t = jwt(r#"{"aud":"Documents","permissions":["Documents:Api:All"],"exp":1000}"#);
        let TokenInfo::Jwt(info) = analyze(&t) else {
            panic!("not a jwt")
        };
        assert_eq!(info.aud, vec!["Documents"]);
        assert!(info.has_api_permission());
        assert_eq!(info.expiry(400), Expiry::In(600));
        assert_eq!(info.expiry(1000), Expiry::Expired);

        let t = jwt(r#"{"aud":["a","b"],"permissions":[]}"#);
        let TokenInfo::Jwt(info) = analyze(&t) else {
            panic!("not a jwt")
        };
        assert_eq!(info.aud, vec!["a", "b"]);
        assert!(!info.has_api_permission());
        assert_eq!(info.expiry(0), Expiry::Missing);
    }

    #[test]
    fn opaque_and_empty() {
        assert_eq!(analyze("  "), TokenInfo::Empty);
        assert_eq!(analyze("a1b2c3d4e5"), TokenInfo::Opaque);
        assert_eq!(analyze("a.b.c"), TokenInfo::Opaque); // header is not JSON
        assert_eq!(
            Secret::new("raw-secret".into()).auth_header().expose(),
            "raw-secret"
        );
        let t = jwt(r#"{"exp":1}"#);
        assert_eq!(
            Secret::new(t.clone()).auth_header().expose(),
            format!("Bearer {t}")
        );
    }

    #[test]
    fn secret_is_redacted() {
        let s = Secret::new("super-secret-token".into());
        assert_eq!(format!("{s:?}"), "Secret(<redacted>)");
        assert_eq!(format!("{s}"), "<redacted>");
        assert!(!format!("{:?}", Some(&s)).contains("super"));
    }

    #[test]
    fn durations() {
        assert_eq!(fmt_duration(30), "30 s");
        assert_eq!(fmt_duration(14 * 60), "14 min");
        assert_eq!(fmt_duration(5 * 3600), "5 h");
        assert_eq!(fmt_duration(5 * 86_400), "5 days");
    }
}
