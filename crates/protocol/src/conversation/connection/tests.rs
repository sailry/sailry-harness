use super::*;
use crate::conversation::{ModelApi, Provider};
use serde_json::json;

fn provider() -> Provider {
    Provider {
        id: crate::ProviderId::new(),
        revision: 1,
        name: "Connection fixture".into(),
        api: ModelApi::ChatCompletions,
        authentication: crate::Authentication::ApiKey,
        endpoint: "https://example.test/v1".into(),
        options: None,
        oauth: None,
        enabled: true,
        models: vec![],
        default_model: String::new(),
        credential: None,
    }
}

#[test]
fn preserves_azure_wire_format() {
    let options = Options::AzureOpenAi {
        api_version: "2024-10-21".into(),
    };
    let body = json!({"kind": "azure_open_ai", "api_version": "2024-10-21"});
    assert_eq!(serde_json::to_value(&options).unwrap(), body);
    assert_eq!(serde_json::from_value::<Options>(body).unwrap(), options);
    assert_eq!(crate::VERSION, 1);
}

#[test]
fn round_trips_hosted_products() {
    for (product, name) in [(Product::Metered, "metered"), (Product::Coding, "coding")] {
        let options = Options::Hosted { product };
        let body = json!({"kind": "hosted", "product": name});
        assert_eq!(serde_json::to_value(&options).unwrap(), body);
        assert_eq!(serde_json::from_value::<Options>(body).unwrap(), options);
        let mut provider = provider();
        provider.options = Some(options);
        let body = serde_json::to_value(&provider).unwrap();
        assert_eq!(body["options"]["product"], name);
        assert_eq!(serde_json::from_value::<Provider>(body).unwrap(), provider);
    }
}

#[test]
fn keeps_unset_options_omitted() {
    let provider = provider();
    let mut body = serde_json::to_value(&provider).unwrap();
    assert!(body.get("options").is_none());
    assert_eq!(
        serde_json::from_value::<Provider>(body.clone()).unwrap(),
        provider
    );
    body["options"] = serde_json::Value::Null;
    assert_eq!(serde_json::from_value::<Provider>(body).unwrap(), provider);
}

#[test]
fn requires_an_explicit_hosted_product() {
    for body in [
        json!({"kind": "hosted"}),
        json!({"kind": "hosted", "product": null}),
        json!({"kind": "hosted", "product": "subscription"}),
    ] {
        assert!(serde_json::from_value::<Options>(body).is_err());
    }
}
