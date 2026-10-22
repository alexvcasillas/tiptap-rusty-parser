//! Tiptap Document Server REST client.
//!
//! READ-ONLY BY CONSTRUCTION: this module exposes `GET` and nothing else. There
//! is no code path that issues POST, PATCH, PUT or DELETE, and none may be
//! added: studio is a debugging tool and a `Documents:Api:All` token is an
//! admin credential. Redirects are not followed (so the `Authorization` header
//! can never be forwarded to another host).
//!
//! Endpoints (https://tiptap.dev/docs/collaboration/documents/rest-api):
//! - `GET /api/documents?take=&skip=`
//! - `GET /api/documents/:id?format=json&fragment=default[&version=]`
//! - `GET /api/documents/:id/versions`
//!
//! Response shapes are read defensively (see the `parse_*` fns), because the
//! docs do not pin them down.

use std::time::Duration;

use serde_json::Value;

use super::secret::Secret;

pub const PAGE: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// 401 / 403.
    Auth(u16),
    NotFound,
    Status(u16),
    Network(String),
    Timeout,
    Parse(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Auth(c) => write!(f, "{c}: token rejected or lacks Documents:Api:All"),
            ApiError::NotFound => f.write_str("404: not found (check the app id / document name)"),
            ApiError::Status(c) => write!(f, "server answered HTTP {c}"),
            ApiError::Network(e) => write!(f, "network error: {e}"),
            ApiError::Timeout => f.write_str("timed out (10 s connect / 30 s read)"),
            ApiError::Parse(e) => write!(f, "unexpected response: {e}"),
        }
    }
}

/// `https://<app_id>.collab.tiptap.cloud`, or a validated override. Only
/// `https://` is accepted, except plain `http://` to localhost / 127.0.0.1 for
/// testing against a local server.
pub fn base_url(app_id: &str, override_url: &str) -> Result<String, String> {
    let o = override_url.trim().trim_end_matches('/');
    if !o.is_empty() {
        if o.starts_with("https://") && o.len() > "https://".len() {
            return Ok(o.to_string());
        }
        if let Some(rest) = o.strip_prefix("http://") {
            let host = rest.split(['/', ':']).next().unwrap_or("");
            if host == "localhost" || host == "127.0.0.1" {
                return Ok(o.to_string());
            }
        }
        return Err("Base URL must be https:// (http:// only for localhost / 127.0.0.1)".into());
    }
    let id = app_id.trim();
    if id.is_empty() {
        return Err("App id is required".into());
    }
    if !id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("App id may contain only letters, digits, - and _".into());
    }
    Ok(format!("https://{id}.collab.tiptap.cloud"))
}

/// Percent-encode one path segment / query value (RFC 3986 unreserved set
/// kept), so `a/b c` → `a%2Fb%20c`.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocEntry {
    pub name: String,
    pub updated: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VersionEntry {
    pub id: String,
    pub name: Option<String>,
    pub date: Option<String>,
}

pub struct Client {
    agent: ureq::Agent,
    base: String,
    auth: Secret,
}

impl Client {
    pub fn new(base: String, token: &Secret) -> Self {
        use ureq::tls::{RootCerts, TlsConfig};
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_recv_body(Some(Duration::from_secs(30)))
            .max_redirects(0)
            .http_status_as_error(false)
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .user_agent("tiptap-studio")
            .build()
            .new_agent();
        Self {
            agent,
            base,
            auth: token.auth_header(),
        }
    }

    /// The single network primitive. GET only.
    fn get(&self, path_and_query: &str) -> Result<String, ApiError> {
        let url = format!("{}{path_and_query}", self.base);
        let res = self
            .agent
            .get(&url)
            .header("Authorization", self.auth.expose())
            .header("Accept", "application/json")
            .call();
        let mut res = match res {
            Ok(r) => r,
            Err(ureq::Error::Timeout(_)) => return Err(ApiError::Timeout),
            Err(e) => return Err(ApiError::Network(net_reason(&e))),
        };
        match res.status().as_u16() {
            200..=299 => {}
            401 | 403 => return Err(ApiError::Auth(res.status().as_u16())),
            404 => return Err(ApiError::NotFound),
            c => return Err(ApiError::Status(c)),
        }
        res.body_mut()
            .with_config()
            .limit(256 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| match e {
                ureq::Error::Timeout(_) => ApiError::Timeout,
                e => ApiError::Network(net_reason(&e)),
            })
    }

    pub fn list_documents(&self, skip: usize) -> Result<Vec<DocEntry>, ApiError> {
        let body = self.get(&format!("/api/documents?take={PAGE}&skip={skip}"))?;
        parse_doc_list(&json(&body)?)
    }

    pub fn versions(&self, id: &str) -> Result<Vec<VersionEntry>, ApiError> {
        let body = self.get(&format!("/api/documents/{}/versions", encode(id)))?;
        parse_versions(&json(&body)?)
    }

    /// Tiptap JSON of `id` (latest, or at `version`).
    pub fn document(&self, id: &str, version: Option<&str>) -> Result<Value, ApiError> {
        let mut q = format!("/api/documents/{}?format=json&fragment=default", encode(id));
        if let Some(v) = version {
            q.push_str("&version=");
            q.push_str(&encode(v));
        }
        extract_doc(json(&self.get(&q)?)?, "default")
    }
}

/// Reason text without the URL (it is not secret, but keeps messages short).
fn net_reason(e: &ureq::Error) -> String {
    match e {
        ureq::Error::HostNotFound => "host not found".into(),
        ureq::Error::Io(io) => io.to_string(),
        ureq::Error::ConnectionFailed => "connection failed".into(),
        e => e.to_string(),
    }
}

fn json(body: &str) -> Result<Value, ApiError> {
    serde_json::from_str(body).map_err(|e| ApiError::Parse(e.to_string()))
}

/// Assumed list shapes, any of:
/// `[ "name", … ]`, `[ { "name"|"identifier"|"id"|"documentName": …, "updated_at"? }, … ]`,
/// or an object wrapping such an array under `data` / `documents` / `items` / `results`.
pub fn parse_doc_list(v: &Value) -> Result<Vec<DocEntry>, ApiError> {
    let arr = unwrap_array(v, &["data", "documents", "items", "results"])
        .ok_or_else(|| ApiError::Parse("document list is not an array".into()))?;
    Ok(arr
        .iter()
        .filter_map(|item| match item {
            Value::String(s) => Some(DocEntry {
                name: s.clone(),
                updated: None,
            }),
            Value::Object(_) => Some(DocEntry {
                name: first_str(item, &["name", "identifier", "documentName", "id"])?,
                updated: first_str(
                    item,
                    &["updated_at", "updatedAt", "created_at", "createdAt"],
                ),
            }),
            _ => None,
        })
        .collect())
}

/// Assumed versions shapes: an array (or `{versions|data|items: [..]}`) of
/// `{ "version"|"id"|"versionId": n|s, "name"|"meta.name"?, "date"|"created_at"|"createdAt"|"timestamp"? }`.
/// Returned newest first: by date when every entry has one, else by numeric id.
pub fn parse_versions(v: &Value) -> Result<Vec<VersionEntry>, ApiError> {
    let arr = unwrap_array(v, &["versions", "data", "items", "results"])
        .ok_or_else(|| ApiError::Parse("versions list is not an array".into()))?;
    let mut out: Vec<VersionEntry> = arr
        .iter()
        .filter_map(|item| {
            Some(VersionEntry {
                id: first_str(item, &["version", "id", "versionId"])?,
                name: first_str(item, &["name"])
                    .or_else(|| item.get("meta").and_then(|m| first_str(m, &["name"]))),
                date: first_str(item, &["date", "created_at", "createdAt", "timestamp"]),
            })
        })
        .collect();
    let num = |e: &VersionEntry| e.id.parse::<i64>().ok();
    if out.iter().all(|e| e.date.is_some()) {
        // ISO-8601 and epoch numbers both sort correctly as (len, text).
        out.sort_by(|a, b| {
            let (a, b) = (a.date.as_deref().unwrap(), b.date.as_deref().unwrap());
            (b.len(), b).cmp(&(a.len(), a))
        });
    } else {
        out.sort_by_key(|e| std::cmp::Reverse(num(e)));
    }
    Ok(out)
}

/// The body is the doc itself (`{"type":"doc",…}`) or keyed by fragment name
/// (`{"default": {…}}`); a lone key holding a doc is accepted too.
pub fn extract_doc(v: Value, fragment: &str) -> Result<Value, ApiError> {
    if v.get("type").is_some() || v.get("content").is_some() {
        return Ok(v);
    }
    let Value::Object(mut map) = v else {
        return Err(ApiError::Parse("document is not a JSON object".into()));
    };
    if let Some(d) = map.remove(fragment) {
        return Ok(d);
    }
    if map.len() == 1 {
        if let Some((_, d)) = map.into_iter().next() {
            if d.get("type").is_some() {
                return Ok(d);
            }
        }
        return Err(ApiError::Parse("no Tiptap document in response".into()));
    }
    Err(ApiError::Parse(format!(
        "no \"{fragment}\" fragment in response"
    )))
}

fn unwrap_array<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a Vec<Value>> {
    match v {
        Value::Array(a) => Some(a),
        Value::Object(_) => keys.iter().find_map(|k| v.get(*k)?.as_array()),
        _ => None,
    }
}

fn first_str(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| match v.get(*k)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encodes_identifiers() {
        assert_eq!(encode("doc-42"), "doc-42");
        assert_eq!(encode("team/a b"), "team%2Fa%20b");
        assert_eq!(encode("é?&#"), "%C3%A9%3F%26%23");
    }

    #[test]
    fn base_urls() {
        assert_eq!(
            base_url("abc123", "").unwrap(),
            "https://abc123.collab.tiptap.cloud"
        );
        assert!(base_url("evil.com/x", "").is_err());
        assert!(base_url("", "").is_err());
        assert_eq!(
            base_url("x", "http://127.0.0.1:8765/").unwrap(),
            "http://127.0.0.1:8765"
        );
        assert!(base_url("x", "http://localhost").is_ok());
        assert!(base_url("x", "http://example.com").is_err());
        assert!(base_url("x", "http://localhost.evil.com").is_err());
        assert!(base_url("x", "https://docs.example.com").is_ok());
        assert!(base_url("x", "ftp://a").is_err());
    }

    #[test]
    fn parses_doc_lists() {
        let a = parse_doc_list(&json!([{"name": "a", "updated_at": "2026-01-01"}, "b"])).unwrap();
        assert_eq!(a[0].name, "a");
        assert_eq!(a[0].updated.as_deref(), Some("2026-01-01"));
        assert_eq!(a[1].name, "b");
        let b = parse_doc_list(&json!({"data": [{"identifier": "x"}, {"id": 7}, 3]})).unwrap();
        assert_eq!(
            b.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            ["x", "7"]
        );
        assert!(parse_doc_list(&json!({"nope": 1})).is_err());
    }

    #[test]
    fn parses_versions_newest_first() {
        let v = parse_versions(&json!([
            {"version": 1, "date": "2026-01-01T00:00:00Z"},
            {"version": 3, "date": "2026-03-01T00:00:00Z", "meta": {"name": "final"}},
            {"version": 2, "date": "2026-02-01T00:00:00Z"},
        ]))
        .unwrap();
        assert_eq!(
            v.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["3", "2", "1"]
        );
        assert_eq!(v[0].name.as_deref(), Some("final"));
        let v = parse_versions(&json!({"versions": [{"id": "2"}, {"id": "10"}]})).unwrap();
        assert_eq!(v[0].id, "10");
    }

    #[test]
    fn extracts_docs() {
        let d = json!({"type": "doc", "content": []});
        assert_eq!(extract_doc(d.clone(), "default").unwrap(), d);
        assert_eq!(
            extract_doc(json!({"default": d.clone()}), "default").unwrap(),
            d
        );
        assert_eq!(
            extract_doc(json!({"body": d.clone()}), "default").unwrap(),
            d
        );
        assert!(extract_doc(json!({"a": 1, "b": 2}), "default").is_err());
        assert!(extract_doc(json!([1]), "default").is_err());
    }
}
