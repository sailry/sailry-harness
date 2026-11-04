use super::*;

#[test]
fn preserves_document_behavior() {
    for (scenario, check) in [
        (
            "accepts_each_supported_shape",
            (|scenario: &'static str| {
                for kind in ["line", "bar", "area", "pie", "radar"] {
                    assert!(Chart::parse(&format!(r#"{{"type":"{kind}","title":"Values","data":[{{"label":"A","value":1}},{{"label":"B","value":2}},{{"label":"C","value":3}}]}}"#)).is_some(), "{scenario}");
                }
                assert!(
                Chart::parse(
                    r#"{"type":"candlestick","data":[{"label":"Mon","open":2,"high":4,"low":1,"close":3}]}"#
                )
                .is_some(), "{scenario}");
                assert!(Chart::parse(r#"{"type":"sankey","nodes":[{"id":"a","label":"A"},{"id":"b","label":"B"}],"links":[{"source":"a","target":"b","value":1}]}"#).is_some(), "{scenario}");
            }) as fn(&'static str),
        ),
        (
            "refuses_malformed_and_unsafe_layout_data",
            (|scenario: &'static str| {
                for source in [
                    r#"{"type":"line","data":[],"unknown":true}"#,
                    r#"{"type":"pie","data":[{"label":"A","value":-1}]}"#,
                    r#"{"type":"candlestick","data":[{"label":"A","open":2,"high":1,"low":0,"close":3}]}"#,
                    r#"{"type":"sankey","nodes":[{"id":"a","label":"A"},{"id":"b","label":"B"}],"links":[{"source":"a","target":"b","value":1},{"source":"b","target":"a","value":2}]}"#,
                ] {
                    assert!(Chart::parse(source).is_none(), "{scenario}: {source}");
                }
                assert!(
                    Chart::parse(&" ".repeat(super::super::visuals::MAX_SOURCE_BYTES + 1))
                        .is_none(),
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
