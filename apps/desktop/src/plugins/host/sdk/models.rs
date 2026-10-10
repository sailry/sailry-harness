use super::values::encode;
use gpui_shell::{HostError, HostValue};
use serde_json::{Value, json};

impl super::Host {
    pub(super) fn session_model_module(
        self: &std::sync::Arc<Self>,
        module: gpui_shell::HostModule,
    ) -> gpui_shell::HostModule {
        let resolve = self.clone();
        module.async_function("resolveSessionModel", move |args| {
            let effort = args
                .get(1)
                .filter(|value| !matches!(value, HostValue::Null))
                .map(|value| {
                    super::values::decode(value).and_then(|value| {
                        serde_json::from_value(value)
                            .map_err(|error| HostError::new(error.to_string()))
                    })
                })
                .transpose()?;
            let config = args
                .get(2)
                .filter(|value| !matches!(value, HostValue::Null))
                .map(|value| {
                    super::values::decode(value).and_then(|value| {
                        serde_json::from_value(value)
                            .map_err(|error| HostError::new(error.to_string()))
                    })
                })
                .transpose()?;
            resolve.read_scoped(
                sailry_protocol::Command::ResolvePluginModel {
                    model: args.string(0)?.into(),
                    effort,
                    config,
                },
                true,
            )
        })
    }
}

pub(super) fn resolve(
    settings: Value,
    catalog: Value,
    field: &str,
) -> Result<HostValue, HostError> {
    let selected = settings["values"][field]
        .as_str()
        .ok_or_else(|| HostError::new("configure"))?;
    let model = catalog["models"]
        .as_array()
        .and_then(|models| models.iter().find(|model| model["id"] == selected))
        .ok_or_else(|| HostError::new("configure"))?;
    let kind = model["kind"]
        .as_str()
        .ok_or_else(|| HostError::new("configure"))?;
    if kind != "provider" {
        return Err(HostError::new("configure"));
    }
    let mut player = json!({"model": selected, "kind": kind});
    let key = settings["values"][format!("{field}_effort")]
        .as_str()
        .unwrap_or("");
    if !key.is_empty() {
        let effort = model["efforts"]
            .as_array()
            .and_then(|values| {
                values.iter().find(|value| {
                    value.as_str() == Some(key)
                        || value["budget"]
                            .as_u64()
                            .is_some_and(|budget| budget.to_string() == key)
                })
            })
            .ok_or_else(|| HostError::new("configure"))?;
        player["effort"] = effort.clone();
    }
    encode(player)
}

#[cfg(test)]
mod configuration {
    use super::super::values::decode;
    use super::*;
    #[test]
    fn resolves_provider_and_preserves_effort() {
        let catalog =
            json!({"models":[{"id":"provider/model","kind":"provider","efforts":["none","high"]}]});
        let settings = json!({"values":{"opponent":"provider/model","opponent_effort":"high"}});
        let resolved = decode(&resolve(settings, catalog.clone(), "opponent").unwrap()).unwrap();
        assert_eq!(resolved["kind"], "provider");
        assert_eq!(resolved["effort"], "high");
        assert!(
            resolve(
                json!({"values":{"opponent":"missing"}}),
                catalog.clone(),
                "opponent"
            )
            .is_err()
        );
        assert!(
            resolve(
                json!({"values":{"opponent":"provider/model","opponent_effort":"max"}}),
                catalog,
                "opponent"
            )
            .is_err()
        );
    }
}
