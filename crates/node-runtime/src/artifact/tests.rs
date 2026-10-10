use super::*;

#[test]
fn strips_executable_and_external_content() {
    let (html,warnings)=sanitize("<section style='color:red' onclick='run()'><script>bad()</script><style>body{}</style><iframe src='https://x'>bad</iframe><a href='javascript:run()'>bad</a><a href='https://x'>external</a><a href='#here'>anchor</a><img src='https://x'><p>正文</p></section>").unwrap();
    for value in [
        "onclick",
        "style=",
        "script",
        "iframe",
        "javascript:",
        "https://",
        "bad()",
    ] {
        assert!(!html.contains(value), "{value}: {html}");
    }
    assert!(html.contains("href=\"#here\""));
    assert!(html.contains("正文"));
    assert!(!warnings.is_empty());
}

#[test]
fn retains_declarative_forms_and_diagrams() {
    let (html,_)=sanitize("<form class='brief-form' data-name='choices' action='https://x'><fieldset data-name='target' data-required><legend>Target</legend><label><input type='radio' name='target' value='node' checked>Node</label><input type='file'><textarea name='note' rows='4'></textarea><button type='submit'>Send</button></fieldset></form><pre class='mermaid'>flowchart LR\nA--&gt;B</pre>").unwrap();
    assert!(html.contains("class=\"brief-form\""));
    assert!(html.contains("type=\"radio\""));
    assert!(!html.contains("type=\"file\""));
    assert!(!html.contains("action="));
    assert!(html.contains("class=\"mermaid\""));
}

#[test]
fn validates_closed_chart_shapes() {
    let (html, _) =
        sanitize(r#"<pre class="chart">{"type":"line","data":[{"label":"A","value":1}]}</pre>"#)
            .unwrap();
    assert!(html.contains("class=\"chart\""));
    for content in [
        r#"{"type":"line","extra":1,"data":[{"label":"A","value":1}]}"#,
        r#"{"type":"sankey","nodes":[{"id":"A","label":"A"}],"links":[{"source":"A","target":"A","value":1}]}"#,
        "<b>invalid</b>",
    ] {
        let (html, warnings) = sanitize(&format!("<pre class='chart'>{content}</pre>")).unwrap();
        assert!(!html.contains("class=\"chart\""));
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("Invalid chart"))
        );
        assert!(html.contains("<pre"));
    }
}

#[test]
fn preserves_quoted_chart_attributes() {
    let valid = r#"{"type":"line","data":[{"label":"A","value":1}]}"#;
    for (data, chart) in [(valid, true), ("invalid", false)] {
        let (html, warnings) = sanitize(&format!(
            r#"<pre data-title="x>y" class="chart">{data}</pre>"#,
        ))
        .unwrap();
        assert!(html.contains("data-title=\"x&gt;y\""));
        assert_eq!(html.contains("class=\"chart\""), chart);
        assert_eq!(
            warnings
                .iter()
                .any(|warning| warning.contains("Invalid chart")),
            !chart,
        );
    }
}

#[test]
fn ignores_attribute_value_class_markers() {
    let valid = r#"{"type":"line","data":[{"label":"A","value":1}]}"#;
    for attribute in ["data-title", "data-kind"] {
        for value in ["class=", "prefix class="] {
            for (data, chart) in [(valid, true), ("invalid", false)] {
                let (html, warnings) = sanitize(&format!(
                    r#"<pre {attribute}="{value}" class="info chart">{data}</pre>"#,
                ))
                .unwrap();
                assert!(html.contains(&format!(r#"{attribute}="{value}""#)));
                assert!(html.contains(if chart {
                    "class=\"info chart\""
                } else {
                    "class=\"info\""
                }));
                assert_eq!(
                    warnings
                        .iter()
                        .any(|warning| warning.contains("Invalid chart")),
                    !chart,
                );
            }
        }
    }
}

#[test]
fn validates_nested_chart_blocks() {
    for class in ["", " class='chart'"] {
        let (html, warnings) = sanitize(&format!(
            "<pre{class}><pre class='chart'>invalid</pre></pre>",
        ))
        .unwrap();
        assert!(!html.contains("class=\"chart\""));
        assert!(html.contains("invalid"));
        assert_eq!(
            warnings
                .iter()
                .filter(|warning| warning.contains("Invalid chart"))
                .count(),
            1,
        );
    }
    let (html, _) = sanitize(
        r#"<pre class="chart"><pre class="chart">{"type":"line","data":[{"label":"A","value":1}]}</pre></pre>"#,
    )
    .unwrap();
    assert_eq!(html.matches("class=\"chart\"").count(), 1);
}

#[test]
fn retains_math_and_bounded_field_names() {
    let (html, _) = sanitize(r#"<span class="math-inline">x^2</span><pre class="math">y = x^2</pre><form data-name="answer.main"><input name="option.first"><input name="_invalid"><input name="1invalid"><textarea name="notes-extra"></textarea></form>"#).unwrap();
    assert!(html.contains("class=\"math-inline\""));
    assert!(html.contains("class=\"math\""));
    assert!(html.contains("data-name=\"answer.main\""));
    assert!(html.contains("name=\"option.first\""));
    assert!(html.contains("name=\"notes-extra\""));
    assert!(!html.contains("_invalid"));
    assert!(!html.contains("1invalid"));
}

#[test]
fn rejects_empty_and_oversized_fragments() {
    assert!(sanitize("<script>bad()</script>").is_err());
    assert!(sanitize(&"x".repeat(MAX_HTML_BYTES + 1)).is_err());
    assert!(sanitize("\0").is_err());
}
