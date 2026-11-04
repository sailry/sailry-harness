use super::*;
use crate::ErrorCode;

fn definition(format: Format) -> Definition {
    let source = Source {
        hub: Hub::HuggingFace,
        repository: "example/model".into(),
        revision: "main".into(),
    };
    let names: &[&str] = match format {
        Format::Transformer { .. } => &["config.json", "model.onnx", "tokenizer.json"],
        Format::Static { .. } => &["config.json", "model.safetensors", "tokenizer.json"],
        Format::SenseVoice {} => &["model.onnx", "tokens.txt"],
        Format::Decision {} => &["model.gguf"],
        Format::PaddleOcr {
            orientation: false, ..
        } => &["detector.onnx", "recognizer.onnx", "dictionary.txt"],
        Format::PaddleOcr {
            orientation: true, ..
        } => &[
            "detector.onnx",
            "recognizer.onnx",
            "dictionary.txt",
            "orientation.onnx",
        ],
    };
    Definition {
        source: source.clone(),
        name: "Example model".into(),
        license: "Apache-2.0".into(),
        query_prefix: None,
        document_prefix: None,
        format,
        assets: names
            .iter()
            .map(|name| Asset {
                source: Source {
                    revision: "a".repeat(40),
                    ..source.clone()
                },
                path: format!("onnx/{name}"),
                name: (*name).into(),
                bytes: 128,
                sha256: "b".repeat(64),
            })
            .collect(),
    }
}

fn transformer() -> Format {
    Format::Transformer {
        dimension: 384,
        pooling: Pooling::Cls,
        token_types: false,
    }
}

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "binds_ocr_roles_and_family",
            (|scenario: &'static str| {
                for family in [PaddleFamily::V5, PaddleFamily::V6] {
                    for orientation in [false, true] {
                        let mut selected = definition(Format::PaddleOcr {
                            family,
                            orientation,
                        });
                        selected.validate().unwrap();
                        assert_eq!(selected.format.kind(), Kind::Ocr, "{scenario}");
                        assert_eq!(selected.format.dimension(), None, "{scenario}");
                        let encoded = serde_json::to_vec(&selected).unwrap();
                        assert_eq!(
                            serde_json::from_slice::<Definition>(&encoded).unwrap(),
                            selected,
                            "{scenario}"
                        );
                        selected.query_prefix = Some("query: ".into());
                        assert!(selected.validate().is_err(), "{scenario}");
                        selected.query_prefix = None;
                        selected.assets.pop();
                        assert!(selected.validate().is_err(), "{scenario}");
                    }
                }
            }) as fn(&'static str),
        ),
        (
            "requires_full_asset_pins",
            (|scenario: &'static str| {
                for pin in [
                    "main".into(),
                    "a".repeat(39),
                    "a".repeat(41),
                    "g".repeat(40),
                ] {
                    let mut definition = definition(transformer());
                    definition.assets[0].source.revision = pin;
                    assert_eq!(
                        definition.validate().unwrap_err().code,
                        ErrorCode::InvalidRequest,
                        "{scenario}"
                    );
                }
                definition(transformer()).validate().unwrap();
            }),
        ),
        (
            "rejects_path_traversal_and_urls",
            (|scenario: &'static str| {
                for path in [
                    "",
                    "../model.onnx",
                    "onnx/../model.onnx",
                    "/model.onnx",
                    "onnx//model.onnx",
                    "onnx\\model.onnx",
                    "https://example.com/model.onnx",
                    "model.onnx?x=1",
                ] {
                    let mut definition = definition(transformer());
                    definition.assets[0].path = path.into();
                    assert!(definition.validate().is_err(), "{scenario}: {path}");
                }
                for repository in [
                    "../model",
                    "example/../model",
                    "https://example.com/model",
                    "example/model/extra",
                ] {
                    let mut definition = definition(transformer());
                    definition.source.repository = repository.into();
                    assert!(definition.validate().is_err(), "{scenario}: {repository}");
                }
            }),
        ),
        (
            "binds_each_format_to_its_assets",
            (|scenario: &'static str| {
                for format in [
                    transformer(),
                    Format::Static { dimension: 256 },
                    Format::SenseVoice {},
                    Format::Decision {},
                ] {
                    let mut missing = definition(format);
                    missing.validate().unwrap();
                    missing.assets.pop();
                    assert!(missing.validate().is_err(), "{scenario}");
                    let mut duplicate = definition(format);
                    duplicate.assets.push(duplicate.assets[0].clone());
                    assert!(duplicate.validate().is_err(), "{scenario}");
                    let mut unexpected = definition(format);
                    unexpected.assets[0].name = "unexpected.bin".into();
                    assert!(unexpected.validate().is_err(), "{scenario}");
                }
                let mut speech = definition(Format::SenseVoice {});
                speech.format = transformer();
                assert!(speech.validate().is_err(), "{scenario}");
                assert_eq!(Format::SenseVoice {}.kind(), Kind::Speech, "{scenario}");
                assert_eq!(transformer().kind(), Kind::Embedding, "{scenario}");
                assert_eq!(
                    Format::Static { dimension: 256 }.kind(),
                    Kind::Embedding,
                    "{scenario}"
                );
            }),
        ),
        (
            "bounds_dimensions_and_file_sizes",
            (|scenario: &'static str| {
                for dimension in [0, 4097, u16::MAX] {
                    assert!(
                        definition(Format::Transformer {
                            dimension,
                            pooling: Pooling::Mean,
                            token_types: true
                        })
                        .validate()
                        .is_err(),
                        "{scenario}"
                    );
                    assert!(
                        definition(Format::Static { dimension }).validate().is_err(),
                        "{scenario}"
                    );
                }
                for bytes in [0, MAX_ASSET_BYTES + 1] {
                    let mut definition = definition(transformer());
                    definition.assets[0].bytes = bytes;
                    assert!(definition.validate().is_err(), "{scenario}");
                }
                for digest in ["b".repeat(63), "b".repeat(65), "z".repeat(64)] {
                    let mut definition = definition(transformer());
                    definition.assets[0].sha256 = digest;
                    assert!(definition.validate().is_err(), "{scenario}");
                }
            }),
        ),
        (
            "roundtrips_hubs_and_formats",
            (|scenario: &'static str| {
                for hub in [Hub::HuggingFace, Hub::ModelScope] {
                    for format in [
                        transformer(),
                        Format::Static { dimension: 256 },
                        Format::SenseVoice {},
                    ] {
                        let mut definition = definition(format);
                        definition.source.hub = hub;
                        for asset in &mut definition.assets {
                            asset.source.hub = hub;
                        }
                        let json = serde_json::to_value(&definition).unwrap();
                        if matches!(format, Format::SenseVoice {}) {
                            assert_eq!(
                                json["format"],
                                serde_json::json!({"kind": "sense_voice"}),
                                "{scenario}"
                            );
                        }
                        assert_eq!(
                            serde_json::from_value::<Definition>(json.clone()).unwrap(),
                            definition,
                            "{scenario}"
                        );
                        let mut invalid = json;
                        invalid["format"]["unknown"] = serde_json::json!(true);
                        assert!(
                            serde_json::from_value::<Definition>(invalid).is_err(),
                            "{scenario}"
                        );
                    }
                }
            }),
        ),
        (
            "preserves_optional_input_prefixes",
            (|scenario: &'static str| {
                let mut model = definition(transformer());
                let plain = serde_json::to_value(&model).unwrap();
                assert!(plain.get("query_prefix").is_none(), "{scenario}");
                assert!(plain.get("document_prefix").is_none(), "{scenario}");
                assert_eq!(
                    serde_json::from_value::<Definition>(plain).unwrap(),
                    model,
                    "{scenario}"
                );
                model.query_prefix = Some("query: ".into());
                model.document_prefix = Some("passage: ".into());
                model.validate().unwrap();
                let value = serde_json::to_value(&model).unwrap();
                assert_eq!(value["query_prefix"], "query: ", "{scenario}");
                assert_eq!(value["document_prefix"], "passage: ", "{scenario}");
                assert_eq!(
                    serde_json::from_value::<Definition>(value).unwrap(),
                    model,
                    "{scenario}"
                );
                for prefix in ["a".repeat(MAX_PREFIX_BYTES + 1), "prefix\0text".into()] {
                    model.query_prefix = Some(prefix.clone());
                    assert!(model.validate().is_err(), "{scenario}");
                    model.query_prefix = None;
                    model.document_prefix = Some(prefix);
                    assert!(model.validate().is_err(), "{scenario}");
                    model.document_prefix = None;
                }
                let mut speech = definition(Format::SenseVoice {});
                speech.query_prefix = Some("query: ".into());
                assert!(speech.validate().is_err(), "{scenario}");
            }),
        ),
        (
            "permits_default_discovery_and_bounds_queries",
            (|scenario: &'static str| {
                let mut search = Search {
                    hub: Hub::HuggingFace,
                    kind: Kind::Embedding,
                    query: String::new(),
                    cursor: None,
                };
                search.validate().unwrap();
                for query in ["a".repeat(513), "query\0text".into()] {
                    search.query = query;
                    assert_eq!(
                        search.validate().unwrap_err().code,
                        ErrorCode::InvalidRequest,
                        "{scenario}"
                    );
                }
                search.query.clear();
                search.cursor = Some("a".repeat(8193));
                assert_eq!(
                    search.validate().unwrap_err().code,
                    ErrorCode::InvalidRequest,
                    "{scenario}"
                );
            }),
        ),
        (
            "binds_decisions_to_one_gguf",
            (|scenario: &'static str| {
                let mut model = definition(Format::Decision {});
                model.validate().unwrap();
                assert_eq!(model.format.kind(), Kind::Decision, "{scenario}");
                model.query_prefix = Some("query".into());
                assert!(model.validate().is_err(), "{scenario}");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
