use super::*;
use serde_json::json;

mod artifacts {
    use super::*;
    use crate::artifact;

    fn summary(kind: artifact::Kind) -> artifact::Summary {
        artifact::Summary {
            id: crate::ArtifactId::new(),
            kind,
            session: crate::SessionId::new(),
            turn: crate::TurnId::new(),
            package: crate::plugin::Reference {
                name: "document-fixture".into(),
                digest: "a".repeat(64),
                settings_revision: 0,
            },
            title: "Options".into(),
            summary: "Compare the approaches".into(),
            revision: 1,
            size: 12,
            created_at: 10,
            updated_at: 10,
        }
    }

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "round_trips_both_kinds",
                (|scenario: &'static str| {
                    for kind in [artifact::Kind::Document, artifact::Kind::Preview] {
                        let artifact = summary(kind);
                        let content = Content {
                            version: 1,
                            blocks: vec![Block::Artifact {
                                artifact: artifact.clone(),
                            }],
                        };
                        let encoded = serde_json::to_value(&content).unwrap();
                        assert_eq!(encoded["blocks"][0]["kind"], "artifact", "{scenario}");
                        assert_eq!(
                            encoded["blocks"][0]["artifact"]["kind"],
                            serde_json::to_value(kind).unwrap(),
                            "{scenario}"
                        );
                        assert_eq!(
                            serde_json::from_value::<Content>(encoded.clone()).unwrap(),
                            content,
                            "{scenario}"
                        );
                        assert_eq!(
                            serde_json::from_str::<Content>(
                                &serde_json::to_string(&content).unwrap()
                            )
                            .unwrap(),
                            content,
                            "{scenario}"
                        );
                        let native = json!({"sailry_content":encoded,"original":"retained"});
                        for result in [native.clone(), json!({"output":native.clone()})] {
                            assert_eq!(
                                Presentation::Content.content(&result),
                                Some(content.clone()),
                                "{scenario}"
                            );
                        }
                        assert_eq!(native["original"], "retained", "{scenario}");
                        assert_eq!(
                            serde_json::from_value::<artifact::Summary>(
                                native["sailry_content"]["blocks"][0]["artifact"].clone()
                            )
                            .unwrap(),
                            artifact,
                            "{scenario}"
                        );
                    }
                }) as fn(&'static str),
            ),
            (
                "rejects_invalid_kind",
                (|scenario: &'static str| {
                    let content = json!(Content {
                        version: 1,
                        blocks: vec![Block::Artifact {
                            artifact: summary(artifact::Kind::Document),
                        }],
                    });
                    for change in 0..3 {
                        let mut invalid = content.clone();
                        match change {
                            0 => invalid["blocks"][0]["artifact"]["kind"] = json!("unknown"),
                            1 => invalid["blocks"][0]["kind"] = json!("document"),
                            2 => invalid["blocks"][0]["artifact"]["unknown"] = json!(true),
                            _ => unreachable!(),
                        }
                        assert!(
                            Presentation::Content
                                .content(&json!({"sailry_content":invalid}))
                                .is_none(),
                            "{scenario}"
                        );
                    }
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}

mod cards {
    use super::*;

    fn summary() -> crate::plugin::card::Summary {
        crate::plugin::card::Summary {
            receipt: crate::plugin::card::Receipt {
                session: crate::SessionId::new(),
                turn: crate::TurnId::new(),
                call: crate::RequestId::new(),
                block: 0,
            },
            node: crate::NodeId([1; 32]),
            worktree: crate::WorktreeId::new(),
            package: crate::plugin::Reference {
                name: "example".into(),
                digest: "a".repeat(64),
                settings_revision: 0,
            },
            key: "connection".into(),
            props: json!({"configured":false}),
            fallback: serde_json::from_value(json!({"label":"Connect","locales":{"zh-CN":"连接"}}))
                .unwrap(),
        }
    }

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "preserves_failed_receipts_and_original_errors",
                (|scenario: &'static str| {
                    let card = summary();
                    let result = json!({"isError":true,"error":{"code":"not_configured","message":"Connect"},"sailry_content":{"version":1,"blocks":[Block::Card(card.clone())]}});
                    let parsed = Presentation::Content.content(&result).unwrap();
                    assert_eq!(parsed.blocks, vec![Block::Card(card)], "{scenario}");
                    assert_eq!(result["error"]["code"], "not_configured", "{scenario}");
                    assert!(result["isError"].as_bool().unwrap(), "{scenario}");
                    assert_eq!(
                        Presentation::Content.content(&json!({"output":result})),
                        Some(parsed),
                        "{scenario}"
                    );
                }) as fn(&'static str),
            ),
            (
                "rejects_forged_drafts_and_private_properties",
                (|scenario: &'static str| {
                    let mut card = summary();
                    for props in [
                        json!({"token":"private"}),
                        json!({"url":"https://example.test/authorize?code_challenge=private"}),
                        json!({"nested":{"configured":false}}),
                    ] {
                        card.props = props;
                        assert!(Presentation::Content.content(&json!({"sailry_content":{"version":1,"blocks":[Block::Card(card.clone())]}})).is_none(), "{scenario}");
                    }
                    assert!(Presentation::Content.content(&json!({"sailry_content":{"version":1,"blocks":[{"kind":"card","key":"connection","props":{},"fallback":{"label":"Connect"}}]}})).is_none(), "{scenario}");
                }),
            ),
            (
                "bounds_closed_schema_and_optional_capabilities",
                (|scenario: &'static str| {
                    let mut declaration: crate::plugin::card::Declaration = serde_json::from_value(json!({
                    "key":"connection","entry":"dev.sailry.platform/card.js",
                    "props_schema":{"type":"object","additionalProperties":false,"properties":{"configured":{"type":"boolean"},"payload":{"type":"string","maxLength":2048},"expiry":{"type":"integer"}},"required":["configured"]}
                })).unwrap();
                    let mut extension: crate::plugin::Extension = serde_json::from_value(json!({"api_version":"v1","actions":[],"desktop":{"resources":["dev.sailry.platform/card.js"]}})).unwrap();
                    assert!(declaration.valid(&extension), "{scenario}");
                    assert!(
                        declaration.accepts(&json!({"configured":false})),
                        "{scenario}"
                    );
                    assert!(
                        !declaration.accepts(&json!({"configured":false,"unknown":true})),
                        "{scenario}"
                    );
                    declaration.capabilities.qr = Some(crate::plugin::card::Qr {
                        payload: "/payload".into(),
                        expires_at: Some("/expiry".into()),
                    });
                    assert!(declaration.valid(&extension), "{scenario}");
                    declaration.capabilities.qr.as_mut().unwrap().payload = "/unknown".into();
                    assert!(!declaration.valid(&extension), "{scenario}");
                    declaration.capabilities.qr = None;
                    declaration.props_schema["additionalProperties"] = json!(true);
                    assert!(!declaration.valid(&extension), "{scenario}");
                    declaration.props_schema["additionalProperties"] = json!(false);
                    extension.desktop.as_mut().unwrap().resources.clear();
                    assert!(!declaration.valid(&extension), "{scenario}");
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}

fn content() -> Value {
    json!({"version":1,"blocks":[
        {"kind":"table","columns":["File","State"],"rows":[["中文 🙂","Ready"]]},
        {"kind":"diff","path":"hello.rs","text":"@@ -1 +1 @@\n-old\n+new\n"},
        {"kind":"image","index":2}
    ]})
}

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "extracts",
            (|scenario: &'static str| {
                let value = content();
                let native = json!({"sailry_content":value,"original":"retained"});
                let parsed = Presentation::Content.content(&native).unwrap();
                assert_eq!(parsed.blocks.len(), 3, "{scenario}");
                assert_eq!(
                    Presentation::Content.content(&json!({"output":native})),
                    Some(parsed),
                    "{scenario}"
                );
                assert!(
                    Presentation::Details.content(&native).is_none(),
                    "{scenario}"
                );
                assert!(
                    Presentation::Summary.content(&native).is_none(),
                    "{scenario}"
                );
                assert_eq!(native["original"], "retained", "{scenario}");
            }) as fn(&'static str),
        ),
        (
            "validates",
            (|scenario: &'static str| {
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
                        "{scenario}: change {change}"
                    );
                }
            }),
        ),
        (
            "business_fields_keep_declared_content",
            (|scenario: &'static str| {
                for result in [
                    json!({"sailry_content":content(),"error":"failed"}),
                    json!({"output":{"sailry_content":content(),"error":"failed"}}),
                    json!({"output":{"sailry_content":content()},"isError":true}),
                ] {
                    let original = result.clone();
                    assert_eq!(
                        Presentation::Content.content(&result).unwrap().blocks.len(),
                        3,
                        "{scenario}"
                    );
                    assert_eq!(result, original, "{scenario}");
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
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
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "native_and_mcp",
                (|scenario: &'static str| {
                    let content = parse(file()).unwrap();
                    let Block::File(file) = &content.blocks[0] else {
                        panic!("{scenario}: file expected");
                    };
                    assert_eq!(file.label(), "报告.html", "{scenario}");
                    assert_eq!(
                        Presentation::Content
                            .content(&json!({"output":{"sailry_content": content}})),
                        Some(content),
                        "{scenario}"
                    );
                    let document = json!({"kind":"file","path":"report.docx","name":"Report","mime":"application/vnd.openxmlformats-officedocument.wordprocessingml.document"});
                    assert!(parse(document.clone()).is_some(), "{scenario}");
                }) as fn(&'static str),
            ),
            (
                "rejects_unscoped_paths",
                (|scenario: &'static str| {
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
                        assert!(parse(block).is_none(), "{scenario}: {path}");
                    }
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
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
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "retains_literal_diagnostics",
                (|scenario: &'static str| {
                    let original = result();
                    for mut value in [
                        original.clone(),
                        json!({"output":original.clone()}),
                        json!({"structuredContent":original,"content":[{"type":"text","text":"retained"}]}),
                    ] {
                        value["isError"] = json!(true);
                        let content = Presentation::Content.content(&value).unwrap();
                        let Block::Text(text) = &content.blocks[0] else {
                            panic!("{scenario}");
                        };
                        assert_eq!(
                            text.read(&value),
                            Some("完整内容 🙂\n```rust\n"),
                            "{scenario}"
                        );
                        assert_eq!(text.notices[0].label("zh-CN"), "不完整", "{scenario}");
                        assert!(content.has_details(&value), "{scenario}");
                        assert!(
                            !serde_json::to_string(&content)
                                .unwrap()
                                .contains("完整内容"),
                            "{scenario}"
                        );
                        value["error"] = json!("failed before producing a result");
                        assert!(
                            Presentation::Content.content(&value).is_some(),
                            "{scenario}"
                        );
                    }
                }) as fn(&'static str),
            ),
            (
                "ignores_undeclared_structured_fields",
                (|scenario: &'static str| {
                    let text = Text {
                        path: "/stream".into(),
                        notices: vec![],
                    };
                    let value = json!({"structuredContent":{"stream":"business value"}});
                    assert!(text.read(&value).is_none(), "{scenario}");
                    assert!(
                        Presentation::Content.content(&value).is_none(),
                        "{scenario}"
                    );
                    let value =
                        json!({"structuredContent":{"stream":"business value"},"output":result()});
                    assert_eq!(
                        text.read(&value),
                        Some("完整内容 🙂\n```rust\n"),
                        "{scenario}"
                    );
                    assert!(
                        Presentation::Content.content(&value).is_some(),
                        "{scenario}"
                    );
                }),
            ),
            (
                "checks_references_and_limits",
                (|scenario: &'static str| {
                    for change in 0..5 {
                        let mut value = result();
                        match change {
                            0 => value["stream"] = json!(42),
                            1 => value["stream"] = json!("x".repeat(MAX_BYTES + 1)),
                            2 => value["sailry_content"]["blocks"][0]["path"] = json!("/missing"),
                            3 => value["sailry_content"]["blocks"][0]["path"] = json!("/bad~2"),
                            4 => {
                                value["sailry_content"]["blocks"][0]["notices"][0]["label"] =
                                    json!("")
                            }
                            _ => unreachable!(),
                        }
                        assert!(
                            Presentation::Content.content(&value).is_none(),
                            "{scenario}: change {change}"
                        );
                    }
                    let value = json!({"stream":"", "sailry_content":{"version":1,"blocks":[{"kind":"text","path":"/stream"}]}});
                    assert!(
                        !Presentation::Content
                            .content(&value)
                            .unwrap()
                            .has_details(&value),
                        "{scenario}"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
