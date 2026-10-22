//! Clipboard payloads: a node is copied as pretty JSON; paste accepts one node
//! (an object with `type`), an array of nodes, or a whole `doc` (its content).

use serde_json::Value;
use tiptap_rusty_parser::Node;

/// Parse clipboard text into the nodes to insert. `Err` is a short reason for
/// the inspector footer.
pub fn parse_fragment(text: &str) -> Result<Vec<Node>, String> {
    let v: Value = serde_json::from_str(text.trim()).map_err(|_| "not JSON")?;
    let items = match v {
        Value::Array(a) => a,
        Value::Object(mut o) => {
            if o.get("type").and_then(Value::as_str) == Some("doc") {
                match o.remove("content") {
                    Some(Value::Array(a)) => a,
                    _ => Vec::new(),
                }
            } else {
                vec![Value::Object(o)]
            }
        }
        _ => return Err("not a node".into()),
    };
    if items.is_empty() {
        return Err("no nodes".into());
    }
    items
        .into_iter()
        .enumerate()
        .map(|(i, item)| {
            if item.get("type").and_then(Value::as_str).is_none() {
                return Err(format!("item {i} has no `type`"));
            }
            serde_json::from_value(item).map_err(|e| format!("item {i}: {e}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_node() {
        let n = parse_fragment(r#"{"type":"callout","attrs":{"variant":"info"}}"#).unwrap();
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].node_type.as_deref(), Some("callout"));
    }

    #[test]
    fn array_and_doc() {
        let a = parse_fragment(r#"[{"type":"paragraph"},{"type":"horizontalRule"}]"#).unwrap();
        assert_eq!(a.len(), 2);
        let d = parse_fragment(r#"{"type":"doc","content":[{"type":"paragraph"}]}"#).unwrap();
        assert_eq!(d[0].node_type.as_deref(), Some("paragraph"));
    }

    #[test]
    fn rejects() {
        assert!(parse_fragment("hello").is_err());
        assert!(parse_fragment("42").is_err());
        assert!(parse_fragment("[]").is_err());
        assert!(parse_fragment(r#"{"type":"doc"}"#).is_err());
        assert!(parse_fragment(r#"[{"text":"x"}]"#).is_err());
    }
}
