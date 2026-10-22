//! Server profiles and their tokens.
//!
//! Profile metadata (label, app id, base-URL override; never the token) lives in
//! `<config>/tiptap-studio/servers.json`. Tokens live ONLY in the OS keychain
//! (service [`SERVICE`], account = profile id). When no keychain is available
//! the caller keeps the token in memory for the session; nothing here ever
//! writes a token to a file.

use std::path::PathBuf;

use serde_json::{json, Value};

use super::secret::Secret;

pub const SERVICE: &str = "dev.tiptap.studio";

#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub id: String,
    pub label: String,
    pub app_id: String,
    /// Empty = `https://<app_id>.collab.tiptap.cloud`.
    pub base_url: String,
}

impl Profile {
    pub fn new_id() -> String {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("p{n:x}")
    }

    /// Short name for titles: the label, else the app id.
    pub fn display(&self) -> &str {
        if self.label.trim().is_empty() {
            &self.app_id
        } else {
            &self.label
        }
    }
}

/// `~/Library/Application Support/tiptap-studio` on macOS, `%APPDATA%\tiptap-studio`
/// on Windows, `$XDG_CONFIG_HOME/tiptap-studio` (or `~/.config/…`) elsewhere.
pub fn config_dir() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support"))
    } else if cfg!(windows) {
        env("APPDATA")
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))
    }?;
    Some(base.join("tiptap-studio"))
}

fn servers_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("servers.json"))
}

pub fn load_profiles() -> Vec<Profile> {
    let Some(text) = servers_file().and_then(|p| std::fs::read_to_string(p).ok()) else {
        return Vec::new();
    };
    parse_profiles(&text)
}

fn parse_profiles(text: &str) -> Vec<Profile> {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let s = |p: &Value, k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    v.get("profiles")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|p| p.get("id").and_then(Value::as_str).is_some())
                .map(|p| Profile {
                    id: s(p, "id"),
                    label: s(p, "label"),
                    app_id: s(p, "app_id"),
                    base_url: s(p, "base_url"),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Metadata only. There is no token field to write.
fn profiles_json(profiles: &[Profile]) -> String {
    let list: Vec<Value> = profiles
        .iter()
        .map(
            |p| json!({ "id": p.id, "label": p.label, "app_id": p.app_id, "base_url": p.base_url }),
        )
        .collect();
    serde_json::to_string_pretty(&json!({ "version": 1, "profiles": list })).unwrap_or_default()
}

pub fn save_profiles(profiles: &[Profile]) -> Result<(), String> {
    let path = servers_file().ok_or("no config directory")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, profiles_json(profiles)).map_err(|e| e.to_string())
}

// ---- keychain (blocking: call from a background task) --------------------------

fn entry(id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, id).map_err(|e| e.to_string())
}

/// `Ok(None)` = no item stored for this profile.
pub fn keychain_get(id: &str) -> Result<Option<Secret>, String> {
    match entry(id)?.get_password() {
        Ok(p) => Ok(Some(Secret::new(p))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn keychain_set(id: &str, token: &Secret) -> Result<(), String> {
    entry(id)?
        .set_password(token.expose())
        .map_err(|e| e.to_string())
}

pub fn keychain_delete(id: &str) -> Result<(), String> {
    match entry(id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_roundtrip_without_tokens() {
        let p = vec![Profile {
            id: "p1".into(),
            label: "Prod".into(),
            app_id: "abc".into(),
            base_url: String::new(),
        }];
        let text = profiles_json(&p);
        assert!(!text.contains("token"));
        assert_eq!(parse_profiles(&text), p);
        assert!(parse_profiles("garbage").is_empty());
    }
}
