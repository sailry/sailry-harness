use super::*;
use serde_json::json;

fn content() -> Value {
    json!({"version":1,"blocks":[
        {"kind":"table","columns":["File","State"],"rows":[["中文 🙂","Ready"]]},
        {"kind":"diff","path":"hello.rs","text":"@@ -1 +1 @@\n-old\n+new\n"},
        {"kind":"image","index":2}
    ]})
}

#[test]
fn extracts() {
    let value = content();
    let native = json!({"sailry_content":value,"original":"retained"});
    let parsed = Presentation::Content.content(&native).unwrap();
    assert_eq!(parsed.blocks.len(), 3);
    assert_eq!(
        Presentation::Content.content(&json!({"output":native})),
        Some(parsed)
    );
    assert!(Presentation::Details.content(&native).is_none());
    assert!(Presentation::Summary.content(&native).is_none());
    assert_eq!(native["original"], "retained");
}

#[test]
fn validates() {
    let original = content();
    for change in 0..10 {
        let mut invalid = original.clone();
        match change {
            0 => invalid["version"] = json!(2),
            1 => invalid["blocks"][0]["rows"][0] = json!(["missing cell"]),
            2 => invalid["blocks"][0]["rows"][0][0] = json!({"html":"<script>"}),
            3 => invalid["blocks"][2]["url"] = json!("https://example.com/image.png"),
            4 => invalid["blocks"][1]["path"] = json!("bad\npath"),
            5 => invalid["blocks"] = json!([]),
            6 => invalid["blocks"] = json!(vec![original["blocks"][1].clone(); 17]),
            7 => invalid["blocks"][0]["rows"][0][0] = json!("x".repeat(16385)),
            8 => invalid["blocks"][1]["text"] = json!("x".repeat(MAX_BYTES)),
            9 => invalid["blocks"]
                .as_array_mut()
                .unwrap()
                .push(json!({"kind":"image","index":2})),
            _ => unreachable!(),
        }
        assert!(
            Presentation::Content
                .content(&json!({"sailry_content":invalid}))
                .is_none(),
            "change {change}"
        );
    }
}

#[test]
fn failures() {
    for result in [
        json!({"sailry_content":content(),"error":"failed"}),
        json!({"output":{"sailry_content":content(),"error":"failed"}}),
        json!({"output":{"sailry_content":content()},"isError":true}),
    ] {
        assert!(Presentation::Content.content(&result).is_none());
    }
}

mod files {
    use super::*;

    fn file() -> Value {
        json!({"kind":"file","path":"output/报告.html","mime":"text/html"})
    }

    fn parse(block: Value) -> Option<Content> {
        Presentation::Content.content(&json!({"sailry_content":{"version":1,"blocks":[block]}}))
    }

    #[test]
    fn native_and_mcp() {
        let content = parse(file()).unwrap();
        let Block::File(file) = &content.blocks[0] else {
            panic!("file expected");
        };
        assert_eq!(file.label(), "报告.html");
        assert_eq!(
            Presentation::Content.content(&json!({"output":{"sailry_content": content}})),
            Some(content)
        );
        let document = json!({"kind":"file","path":"report.docx","name":"Report","mime":"application/vnd.openxmlformats-officedocument.wordprocessingml.document"});
        assert!(parse(document.clone()).is_some());
    }

    #[test]
    fn rejects_unscoped_paths() {
        for path in [
            "",
            "/tmp/file",
            "../outside",
            "a/../outside",
            "a//file",
            "a\\file",
            "https://example.test/file",
            "bad\nfile",
        ] {
            let mut block = file();
            block["path"] = json!(path);
            assert!(parse(block).is_none(), "{path}");
        }
    }
}

mod text {
    use super::*;

    fn result() -> Value {
        json!({"stream":"完整内容 🙂\n```rust\n", "sailry_content":{"version":1,"blocks":[
            {"kind":"text","path":"/stream","notices":[{"label":"Partial","locales":{"zh-CN":"不完整"}}]}
        ]}})
    }

    #[test]
    fn retains_literal_diagnostics() {
        let original = result();
        for mut value in [original.clone(), json!({"output":original})] {
            value["isError"] = json!(true);
            let content = Presentation::Content.content(&value).unwrap();
            let Block::Text(text) = &content.blocks[0] else {
                panic!();
            };
            assert_eq!(text.read(&value), Some("完整内容 🙂\n```rust\n"));
            assert_eq!(text.notices[0].label("zh-CN"), "不完整");
            assert!(content.has_visible(&value));
            assert!(
                !serde_json::to_string(&content)
                    .unwrap()
                    .contains("完整内容")
            );
            value["error"] = json!("failed before producing a result");
            assert!(Presentation::Content.content(&value).is_none());
        }
    }

    #[test]
    fn checks_references_and_limits() {
        for change in 0..5 {
            let mut value = result();
            match change {
                0 => value["stream"] = json!(42),
                1 => value["stream"] = json!("x".repeat(MAX_BYTES + 1)),
                2 => value["sailry_content"]["blocks"][0]["path"] = json!("/missing"),
                3 => value["sailry_content"]["blocks"][0]["path"] = json!("/bad~2"),
                4 => value["sailry_content"]["blocks"][0]["notices"][0]["label"] = json!(""),
                _ => unreachable!(),
            }
            assert!(
                Presentation::Content.content(&value).is_none(),
                "change {change}"
            );
        }
        let value = json!({"stream":"", "sailry_content":{"version":1,"blocks":[{"kind":"text","path":"/stream"}]}});
        assert!(
            !Presentation::Content
                .content(&value)
                .unwrap()
                .has_visible(&value)
        );
    }
}
