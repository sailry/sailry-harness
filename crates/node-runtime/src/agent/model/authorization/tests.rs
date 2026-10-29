use super::*;
use rmcp::model::Tool;

fn tool(name: &str) -> Tool {
    Tool::new(name.to_owned(), "", serde_json::Map::new())
}

#[test]
fn reverses_only_exact_wire_names() {
    let names = names(&[tool("read"), tool("custom")]).unwrap();
    assert_eq!(names.get("Read").map(String::as_str), Some("read"));
    assert_eq!(names.get("custom").map(String::as_str), Some("custom"));
    assert!(!names.contains_key("READ"));
    assert!(!names.contains_key("CUSTOM"));
}

#[test]
fn rejects_normalized_collisions() {
    assert!(names(&[tool("read"), tool("Read")]).is_err());
    assert!(names(&[tool("custom"), tool("custom")]).is_err());
}
