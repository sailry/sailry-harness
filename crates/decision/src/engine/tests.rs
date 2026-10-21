use super::*;

mod coexist;
mod native;

#[test]
fn rejects_other_asset_layouts() {
    let definition = fixture();
    assert!(matches!(
        Engine::load(&definition, Path::new("."), false),
        Err(Error::Invalid(_))
    ));
}

fn fixture() -> Definition {
    Definition {
        name: "Fixture".into(),
        license: "Apache-2.0".into(),
        format: sailry_protocol::models::Format::Static { dimension: 1 },
        source: sailry_protocol::models::Source {
            hub: sailry_protocol::models::Hub::HuggingFace,
            repository: "fixture/model".into(),
            revision: "1".repeat(40),
        },
        query_prefix: None,
        document_prefix: None,
        assets: Vec::new(),
    }
}
