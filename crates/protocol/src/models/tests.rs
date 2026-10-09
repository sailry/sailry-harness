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
fn requires_full_asset_pins() {
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
            ErrorCode::InvalidRequest
        );
    }
    definition(transformer()).validate().unwrap();
}

#[test]
fn rejects_path_traversal_and_urls() {
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
        assert!(definition.validate().is_err(), "{path}");
    }
    for repository in [
        "../model",
        "example/../model",
        "https://example.com/model",
        "example/model/extra",
    ] {
        let mut definition = definition(transformer());
        definition.source.repository = repository.into();
        assert!(definition.validate().is_err(), "{repository}");
    }
}

#[test]
fn binds_each_format_to_its_assets() {
    for format in [
        transformer(),
        Format::Static { dimension: 256 },
        Format::SenseVoice {},
    ] {
        let mut missing = definition(format);
        missing.validate().unwrap();
        missing.assets.pop();
        assert!(missing.validate().is_err());
        let mut duplicate = definition(format);
        duplicate.assets.push(duplicate.assets[0].clone());
        assert!(duplicate.validate().is_err());
        let mut unexpected = definition(format);
        unexpected.assets[0].name = "unexpected.bin".into();
        assert!(unexpected.validate().is_err());
    }
    let mut speech = definition(Format::SenseVoice {});
    speech.format = transformer();
    assert!(speech.validate().is_err());
    assert_eq!(Format::SenseVoice {}.kind(), Kind::Speech);
    assert_eq!(transformer().kind(), Kind::Embedding);
    assert_eq!(Format::Static { dimension: 256 }.kind(), Kind::Embedding);
}

#[test]
fn bounds_dimensions_and_file_sizes() {
    for dimension in [0, 4097, u16::MAX] {
        assert!(
            definition(Format::Transformer {
                dimension,
                pooling: Pooling::Mean,
                token_types: true
            })
            .validate()
            .is_err()
        );
        assert!(definition(Format::Static { dimension }).validate().is_err());
    }
    for bytes in [0, MAX_ASSET_BYTES + 1] {
        let mut definition = definition(transformer());
        definition.assets[0].bytes = bytes;
        assert!(definition.validate().is_err());
    }
    for digest in ["b".repeat(63), "b".repeat(65), "z".repeat(64)] {
        let mut definition = definition(transformer());
        definition.assets[0].sha256 = digest;
        assert!(definition.validate().is_err());
    }
}

#[test]
fn roundtrips_hubs_and_formats() {
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
                assert_eq!(json["format"], serde_json::json!({"kind": "sense_voice"}));
            }
            assert_eq!(
                serde_json::from_value::<Definition>(json.clone()).unwrap(),
                definition
            );
            let mut invalid = json;
            invalid["format"]["unknown"] = serde_json::json!(true);
            assert!(serde_json::from_value::<Definition>(invalid).is_err());
        }
    }
}

#[test]
fn preserves_optional_input_prefixes() {
    let mut model = definition(transformer());
    let plain = serde_json::to_value(&model).unwrap();
    assert!(plain.get("query_prefix").is_none());
    assert!(plain.get("document_prefix").is_none());
    assert_eq!(serde_json::from_value::<Definition>(plain).unwrap(), model);
    model.query_prefix = Some("query: ".into());
    model.document_prefix = Some("passage: ".into());
    model.validate().unwrap();
    let value = serde_json::to_value(&model).unwrap();
    assert_eq!(value["query_prefix"], "query: ");
    assert_eq!(value["document_prefix"], "passage: ");
    assert_eq!(serde_json::from_value::<Definition>(value).unwrap(), model);
    for prefix in ["a".repeat(MAX_PREFIX_BYTES + 1), "prefix\0text".into()] {
        model.query_prefix = Some(prefix.clone());
        assert!(model.validate().is_err());
        model.query_prefix = None;
        model.document_prefix = Some(prefix);
        assert!(model.validate().is_err());
        model.document_prefix = None;
    }
    let mut speech = definition(Format::SenseVoice {});
    speech.query_prefix = Some("query: ".into());
    assert!(speech.validate().is_err());
}
