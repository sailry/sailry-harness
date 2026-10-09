use super::*;

#[test]
fn accepts_each_supported_shape() {
    for kind in ["line", "bar", "area", "pie", "radar"] {
        assert!(Chart::parse(&format!(r#"{{"type":"{kind}","title":"Values","data":[{{"label":"A","value":1}},{{"label":"B","value":2}},{{"label":"C","value":3}}]}}"#)).is_some());
    }
    assert!(
        Chart::parse(
            r#"{"type":"candlestick","data":[{"label":"Mon","open":2,"high":4,"low":1,"close":3}]}"#
        )
        .is_some()
    );
    assert!(Chart::parse(r#"{"type":"sankey","nodes":[{"id":"a","label":"A"},{"id":"b","label":"B"}],"links":[{"source":"a","target":"b","value":1}]}"#).is_some());
}

#[test]
fn refuses_malformed_and_unsafe_layout_data() {
    for source in [
        r#"{"type":"line","data":[],"unknown":true}"#,
        r#"{"type":"pie","data":[{"label":"A","value":-1}]}"#,
        r#"{"type":"candlestick","data":[{"label":"A","open":2,"high":1,"low":0,"close":3}]}"#,
        r#"{"type":"sankey","nodes":[{"id":"a","label":"A"},{"id":"b","label":"B"}],"links":[{"source":"a","target":"b","value":1},{"source":"b","target":"a","value":2}]}"#,
    ] {
        assert!(Chart::parse(source).is_none(), "{source}");
    }
    assert!(Chart::parse(&" ".repeat(super::super::visuals::MAX_SOURCE_BYTES + 1)).is_none());
}
