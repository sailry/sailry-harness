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

pub(super) fn command(player: Value, turn: Value) -> Result<Value, HostError> {
    let model = player["model"]
        .as_str()
        .ok_or_else(|| HostError::new("configure"))?;
    let instructions = turn["instructions"]
        .as_str()
        .ok_or_else(|| HostError::new("invalid model instructions"))?;
    let choices = turn["choices"]
        .as_array()
        .ok_or_else(|| HostError::new("invalid model choices"))?;
    match player["kind"].as_str() {
        Some("provider") => Ok(json!({"kind":"generate_plugin_text","data":{
            "model":model,"effort":player["effort"],
            "prompt":format!("{instructions} Return ONLY {{\"move\":id}} using one offered integer id. No explanation.\n{}", json!({"state":turn["state"],"choices":choices}))
        }})),
        _ => Err(HostError::new("configure")),
    }
}

pub(super) fn select(result: Value, player: Value, moves: Value) -> Result<Value, HostError> {
    let invalid = || HostError::new("invalidResponse");
    if result["Err"]["message"] == "plugin model request timed out" {
        return Err(HostError::new("timedOut"));
    }
    if !result["Err"].is_null() {
        return Err(HostError::new(match result["Err"]["code"].as_str() {
            Some("not_configured") => "configure",
            Some("outcome_unknown") => "unconfirmed",
            _ => "failed",
        }));
    }
    let output = &result["Ok"];
    let (index, tokens) = match player["kind"].as_str() {
        Some("provider") if output["kind"] == "plugin_text" => {
            let text = output["data"]["text"].as_str().ok_or_else(invalid)?.trim();
            let text = text
                .strip_prefix("```json")
                .or_else(|| text.strip_prefix("```"))
                .map(|text| text.strip_suffix("```").unwrap_or(text).trim())
                .unwrap_or(text);
            let reply: Value = serde_json::from_str(text).map_err(|_| invalid())?;
            let index = reply["move"]
                .as_u64()
                .and_then(|id| usize::try_from(id).ok())
                .ok_or_else(invalid)?;
            (index, output["data"]["tokens"].as_u64().unwrap_or(0))
        }
        _ => return Err(invalid()),
    };
    let moves = moves.as_array().ok_or_else(invalid)?;
    Ok(json!({"move":moves.get(index).ok_or_else(invalid)?,"tokens":tokens}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builds_provider_commands() {
        let state = json!({"visible":["card"]});
        let choices = json!([{"id":0,"kind":"pass"}]);
        let request = command(
            json!({"model":"provider/model","kind":"provider","effort":"high"}),
            json!({"instructions":"Choose a move","state":state,"choices":choices}),
        )
        .unwrap();
        assert_eq!(request["kind"], "generate_plugin_text");
        assert_eq!(request["data"]["model"], "provider/model");
        assert_eq!(request["data"]["effort"], "high");
        let prompt = request["data"]["prompt"].as_str().unwrap();
        assert!(prompt.starts_with("Choose a move"));
        let turn: Value = serde_json::from_str(prompt.lines().last().unwrap()).unwrap();
        assert_eq!(turn["state"], state);
        assert_eq!(turn["choices"], choices);
    }

    #[test]
    fn validates_provider_responses_and_faults() {
        let player = json!({"model":"provider/model","kind":"provider"});
        let moves = json!(["pass"]);
        assert_eq!(select(json!({"Ok":{"kind":"plugin_text","data":{"text":"```json\n{\"move\":0}\n```","tokens":5}}}),player.clone(),moves.clone()).unwrap(),json!({"move":"pass","tokens":5}));
        for value in [json!(-1), json!("01"), json!(0.5), json!(1), Value::Null] {
            assert!(
                select(
                    json!({"Ok":{"kind":"plugin_text","data":{"text":json!({"move":value}).to_string()}}}),
                    player.clone(),
                    moves.clone()
                )
                .is_err()
            );
        }
        assert_eq!(
            select(json!({"Err":{"code":"not_configured"}}), player, moves)
                .unwrap_err()
                .message(),
            "configure"
        );
    }
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
