//! Test-only model boundary for one owned background window.
//! This is an opt-in acceptance guard, not a production tool facade.

use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const TOOLS: [&str; 7] = [
    "get_window_state",
    "click",
    "type_text",
    "press_key",
    "scroll",
    "set_value",
    "verify_state",
];
const OBSERVATIONS: [&str; 3] = ["list_apps", "get_accessibility_tree", "get_window_state"];
const RESPONSE_BYTES: usize = 64 * 1024 * 1024;
const MARKUP: &[u8] = b"<tool_call>";

struct Tool {
    action: &'static str,
    arguments: jsonschema::Validator,
}

pub(super) struct Scope {
    pid: u32,
    window: u32,
    model: String,
    tools: BTreeMap<String, Tool>,
    readonly: bool,
}

impl Scope {
    pub(super) fn new(pid: u32, window: u32, model: String) -> Self {
        Self::with_tools(pid, window, model, &TOOLS, false)
    }

    pub(super) fn readonly(pid: u32, window: u32, model: String) -> Self {
        Self::with_tools(pid, window, model, &OBSERVATIONS, true)
    }

    fn with_tools(
        pid: u32,
        window: u32,
        model: String,
        names: &[&'static str],
        readonly: bool,
    ) -> Self {
        let definitions = super::super::driver::catalog();
        let tools = names
            .iter()
            .copied()
            .map(|action| {
                let definition = definitions
                    .iter()
                    .find(|definition| definition["name"] == action)
                    .expect("native acceptance tool");
                let arguments = jsonschema::validator_for(&definition["inputSchema"])
                    .expect("native argument schema");
                (action.to_owned(), Tool { action, arguments })
            })
            .collect();
        Self {
            pid,
            window,
            model,
            tools,
            readonly,
        }
    }

    pub(super) fn preserves_request(&self) -> bool {
        self.readonly
    }

    pub(super) fn filter_request(&self, body: &mut Value) -> Result<(), &'static str> {
        self.check_identity()?;
        let fields = body
            .as_object_mut()
            .ok_or("model request is not an object")?;
        if fields.get("model").and_then(Value::as_str) != Some(self.model.as_str()) {
            return Err("model request is outside the selected model");
        }
        if fields.get("stream") != Some(&Value::Bool(true)) {
            return Err("scoped model request must stream");
        }
        if self.readonly {
            if fields
                .get("reasoning")
                .and_then(|reasoning| reasoning["effort"].as_str())
                != Some("high")
            {
                return Err("read-only acceptance requires high effort");
            }
            let definitions = super::super::driver::catalog();
            for tool in fields
                .get("tools")
                .and_then(Value::as_array)
                .ok_or("model tool declarations are invalid")?
            {
                if tool["type"] == "function" && tool.get("strict") != Some(&Value::Bool(false)) {
                    return Err("read-only acceptance requires non-strict function tools");
                }
                if let Some(definition) = definitions
                    .iter()
                    .find(|definition| definition["name"] == tool["name"])
                {
                    if definition["annotations"]["readOnlyHint"] != true {
                        return Err("read-only acceptance advertised a native write");
                    }
                    let parameters = &tool["parameters"];
                    if parameters["properties"].get("session").is_some()
                        || parameters["required"]
                            .as_array()
                            .is_some_and(|required| required.iter().any(|field| field == "session"))
                    {
                        return Err("bound native tool advertised a session selector");
                    }
                }
            }
            return Ok(());
        }
        if let Some(tools) = fields.get_mut("tools") {
            tools
                .as_array_mut()
                .ok_or("model tool declarations are invalid")?
                .retain(|tool| self.declared_function(tool));
        }
        let advertised: BTreeSet<_> = fields
            .get("tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        match fields.get("tool_choice") {
            None | Some(Value::Null) => Ok(()),
            Some(Value::String(choice)) if matches!(choice.as_str(), "auto" | "none") => Ok(()),
            Some(Value::String(choice)) if choice == "required" && !advertised.is_empty() => Ok(()),
            Some(choice) if choice["type"] == "function" => {
                if self.declared_function(choice)
                    && choice["name"]
                        .as_str()
                        .is_some_and(|name| advertised.contains(name))
                {
                    Ok(())
                } else {
                    Err("model tool choice is outside the available scope")
                }
            }
            Some(choice) if choice["type"] == "allowed_tools" => {
                let choices = choice["tools"]
                    .as_array()
                    .ok_or("model tool choice is invalid")?;
                if matches!(choice["mode"].as_str(), Some("auto" | "required"))
                    && !choices.is_empty()
                    && choices.iter().all(|tool| {
                        self.declared_function(tool)
                            && tool["name"]
                                .as_str()
                                .is_some_and(|name| advertised.contains(name))
                    })
                {
                    Ok(())
                } else {
                    Err("model tool choice is outside the available scope")
                }
            }
            _ => Err("model tool choice is outside the available scope"),
        }
    }

    /// Return the sole validated completed response, including its usage.
    /// No buffered bytes may reach ADK until this succeeds.
    pub(super) async fn validate_response(&self, bytes: &[u8]) -> Result<Value, &'static str> {
        self.check_identity()?;
        if bytes.len() > RESPONSE_BYTES {
            return Err("model response exceeds the scope buffer limit");
        }
        std::str::from_utf8(bytes).map_err(|_| "model response is not UTF-8")?;
        // The pinned parser slices a UTF-8 BOM at byte one. Reject it before
        // parsing or forwarding so neither this guard nor the SDK can panic.
        if bytes.starts_with(b"\xEF\xBB\xBF") {
            return Err("model SSE BOM is unsupported");
        }
        let mut events =
            futures::stream::iter([Ok::<_, std::convert::Infallible>(bytes)]).eventsource();
        let mut completed = None;
        let mut ended = false;
        let mut tails = BTreeMap::new();
        while let Some(event) = events.next().await {
            let event = event.map_err(|_| "model SSE stream is invalid")?;
            if event.event == "keepalive" {
                continue;
            }
            if event.data.trim().is_empty() {
                continue;
            }
            if event.data == "[DONE]" {
                if completed.is_none() || ended {
                    return Err("model stream ended outside its completed response");
                }
                ended = true;
                continue;
            }
            if completed.is_some() {
                return Err("model stream has more than one terminal response");
            }
            let event: Value =
                serde_json::from_str(&event.data).map_err(|_| "model SSE data is not JSON")?;
            let kind = event["type"]
                .as_str()
                .ok_or("model SSE event type is missing")?;
            match kind {
                "response.completed" => {
                    let response = event
                        .get("response")
                        .ok_or("completed response is missing")?;
                    if response["status"] != "completed" {
                        return Err("model response is not completed");
                    }
                    self.validate_output(response, true)?;
                    completed = Some(response.clone());
                }
                "response.incomplete" | "response.failed" | "response.error" | "error" => {
                    return Err("model response did not complete safely");
                }
                "response.created" | "response.in_progress" => {
                    if let Some(response) = event.get("response") {
                        self.validate_output(response, false)?;
                    }
                }
                "response.output_item.added" | "response.output_item.done" => {
                    self.validate_item(
                        event.get("item").ok_or("model output item is missing")?,
                        false,
                    )?;
                }
                "response.content_part.added"
                | "response.content_part.done"
                | "response.reasoning_summary_part.added"
                | "response.reasoning_summary_part.done" => {
                    validate_text_part(event.get("part").ok_or("model text part is missing")?)?;
                }
                "response.output_text.delta"
                | "response.refusal.delta"
                | "response.reasoning_summary_text.delta"
                | "response.reasoning_text.delta" => {
                    validate_delta(&event, &mut tails)?;
                }
                "response.output_text.done"
                | "response.reasoning_summary_text.done"
                | "response.reasoning_text.done" => {
                    reject_markup(event["text"].as_str().ok_or("model text is missing")?)?;
                }
                "response.refusal.done" => {
                    reject_markup(
                        event["refusal"]
                            .as_str()
                            .ok_or("model refusal is missing")?,
                    )?;
                }
                "response.function_call_arguments.delta"
                | "response.function_call_arguments.done"
                | "response.output_text.annotation.added" => {}
                _ => return Err("model event is outside the permitted scope"),
            }
        }
        completed.ok_or("model stream has no completed response")
    }

    fn check_identity(&self) -> Result<(), &'static str> {
        if self.pid == 0 || self.window == 0 || self.model.is_empty() {
            Err("test scope identity is invalid")
        } else {
            Ok(())
        }
    }

    fn declared_function(&self, tool: &Value) -> bool {
        tool["type"] == "function"
            && tool["name"]
                .as_str()
                .is_some_and(|name| self.tools.contains_key(name))
    }

    fn validate_output(&self, response: &Value, final_output: bool) -> Result<(), &'static str> {
        let output = response["output"]
            .as_array()
            .ok_or("model output is missing")?;
        let mut calls = BTreeSet::new();
        let mut items = BTreeSet::new();
        for item in output {
            if let Some(id) = item["id"].as_str()
                && (id.is_empty() || !items.insert(id))
            {
                return Err("model output item identity is duplicated");
            }
            if item["type"] == "function_call" && final_output {
                let id = item["call_id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or("model function call identity is missing")?;
                if !calls.insert(id) {
                    return Err("model function call identity is duplicated");
                }
            }
            self.validate_item(item, final_output)?;
        }
        Ok(())
    }

    fn validate_item(&self, item: &Value, final_output: bool) -> Result<(), &'static str> {
        match item["type"].as_str() {
            Some("function_call") => {
                let name = item["name"]
                    .as_str()
                    .ok_or("model function name is missing")?;
                if self.readonly && name == "set_session_title" {
                    if final_output {
                        let arguments: Value = serde_json::from_str(
                            item["arguments"]
                                .as_str()
                                .ok_or("model title arguments are missing")?,
                        )
                        .map_err(|_| "model title arguments are not JSON")?;
                        if !arguments.as_object().is_some_and(|arguments| {
                            arguments.len() == 1
                                && arguments.get("title").and_then(Value::as_str).is_some_and(
                                    |title| {
                                        !title.trim().is_empty()
                                            && !title.chars().any(char::is_control)
                                    },
                                )
                        }) {
                            return Err("model title arguments are invalid");
                        }
                    }
                    return Ok(());
                }
                let tool = self
                    .tools
                    .get(name)
                    .ok_or("model function is outside the permitted scope")?;
                if final_output {
                    let encoded = item["arguments"]
                        .as_str()
                        .filter(|arguments| !arguments.trim().is_empty())
                        .ok_or("model final function arguments are missing")?;
                    let arguments: Value = serde_json::from_str(encoded)
                        .map_err(|_| "model final function arguments are not JSON")?;
                    self.validate_arguments(tool, &arguments)?;
                }
                Ok(())
            }
            Some("message") => {
                if item["role"] != "assistant" {
                    return Err("model output message role is invalid");
                }
                for part in item["content"]
                    .as_array()
                    .ok_or("model message content is missing")?
                {
                    validate_text_part(part)?;
                }
                Ok(())
            }
            Some("reasoning") => {
                for field in ["summary", "content"] {
                    if let Some(parts) = item.get(field) {
                        for part in parts
                            .as_array()
                            .ok_or("model reasoning content is invalid")?
                        {
                            validate_text_part(part)?;
                        }
                    }
                }
                Ok(())
            }
            _ => Err("model output item is outside the permitted scope"),
        }
    }

    fn validate_arguments(&self, tool: &Tool, arguments: &Value) -> Result<(), &'static str> {
        if !tool.arguments.is_valid(arguments) {
            return Err("model tool arguments do not match the native schema");
        }
        if self.readonly {
            if arguments.get("session").is_some() {
                return Err("model cannot select a host-bound session");
            }
            if tool.action == "get_window_state" {
                if arguments["pid"].as_u64() != Some(u64::from(self.pid)) {
                    return Err("read-only observation pid does not match the owned fixture");
                }
                if arguments["window_id"].as_u64() != Some(u64::from(self.window)) {
                    return Err("read-only observation window_id does not match the owned fixture");
                }
                match arguments.get("include_accessibility_tree") {
                    None => return Err("read-only observation omitted include_accessibility_tree"),
                    Some(Value::Bool(true)) => {}
                    _ => {
                        return Err("read-only observation include_accessibility_tree is not true");
                    }
                }
                match arguments.get("include_screenshot") {
                    None => return Err("read-only observation omitted include_screenshot"),
                    Some(Value::Bool(false)) => {}
                    _ => return Err("read-only observation include_screenshot is not false"),
                }
                if arguments.get("screenshot_out_file").is_some() {
                    return Err("read-only observation requested screenshot_out_file");
                }
            }
            return Ok(());
        }
        let native_target = if tool.action == "click" {
            arguments.get("target")
        } else {
            None
        };
        let target = native_target.unwrap_or(arguments);
        if native_target.is_some_and(|target| target["kind"] != "window")
            || target["pid"].as_u64() != Some(u64::from(self.pid))
            || arguments.get("session").is_some()
            || arguments
                .get("scope")
                .is_some_and(|scope| scope != "window")
            || target
                .get("window_id")
                .is_some_and(|window| window.as_u64() != Some(u64::from(self.window)))
            || (target.get("window_id").is_none() && arguments.get("element_token").is_none())
        {
            return Err("model tool is outside the selected window");
        }
        background(arguments)?;
        if tool.action == "press_key" {
            let mut keys = arguments["modifiers"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            keys.push(arguments["key"].clone());
            validate_keys(&Value::Array(keys))?;
        }
        Ok(())
    }
}

fn background(arguments: &Value) -> Result<(), &'static str> {
    match arguments.get("delivery_mode") {
        None => Ok(()),
        Some(Value::String(mode)) if mode == "background" => Ok(()),
        _ => Err("model input requires background delivery"),
    }
}

fn validate_keys(keys: &Value) -> Result<(), &'static str> {
    let keys = keys.as_array().ok_or("model key chord is invalid")?;
    let keys: Vec<_> = keys
        .iter()
        .map(|key| {
            key.as_str().map(|key| {
                if key.chars().count() == 1 {
                    key.to_ascii_lowercase()
                } else {
                    key.trim().to_ascii_lowercase()
                }
            })
        })
        .collect::<Option<_>>()
        .ok_or("model key chord is invalid")?;
    if keys.iter().any(|key| {
        matches!(
            key.as_str(),
            "cmd"
                | "command"
                | "meta"
                | "super"
                | "win"
                | "windows"
                | "option"
                | "alt"
                | "fn"
                | "function"
        ) || key.contains("clipboard")
            || key.contains("volume")
            || key.contains("brightness")
            || key.contains("mission")
            || key.contains("launch")
            || key.contains("media")
            || key
                .strip_prefix('f')
                .and_then(|key| key.parse::<u8>().ok())
                .is_some()
            || matches!(
                key.as_str(),
                "power" | "sleep" | "eject" | "copy" | "paste" | "cut"
            )
    }) {
        return Err("model key chord is outside target editing");
    }
    let control = keys
        .iter()
        .any(|key| matches!(key.as_str(), "ctrl" | "control"));
    if control
        && keys.iter().any(|key| {
            matches!(
                key.as_str(),
                "space"
                    | " "
                    | "left"
                    | "right"
                    | "up"
                    | "down"
                    | "arrowleft"
                    | "arrowright"
                    | "arrowup"
                    | "arrowdown"
                    | "f2"
                    | "f3"
            )
        })
    {
        return Err("model key chord is outside target editing");
    }
    Ok(())
}

fn validate_text_part(part: &Value) -> Result<(), &'static str> {
    let field = match part["type"].as_str() {
        Some("output_text" | "summary_text" | "reasoning_text") => "text",
        Some("refusal") => "refusal",
        _ => return Err("model text part is outside the permitted scope"),
    };
    reject_markup(part[field].as_str().ok_or("model text part is invalid")?)
}

fn reject_markup(text: &str) -> Result<(), &'static str> {
    if text
        .as_bytes()
        .windows(MARKUP.len())
        .any(|bytes| bytes == MARKUP)
    {
        Err("model tool markup is outside the permitted scope")
    } else {
        Ok(())
    }
}

fn validate_delta(
    event: &Value,
    tails: &mut BTreeMap<(String, u64, String), Vec<u8>>,
) -> Result<(), &'static str> {
    let item = event["item_id"]
        .as_str()
        .ok_or("model text item identity is missing")?;
    let index = event
        .get("content_index")
        .or_else(|| event.get("summary_index"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let kind = event["type"]
        .as_str()
        .ok_or("model text event type is missing")?;
    let delta = event["delta"]
        .as_str()
        .ok_or("model text delta is missing")?;
    let tail = tails
        .entry((item.to_owned(), index, kind.to_owned()))
        .or_default();
    let mut combined = std::mem::take(tail);
    combined.extend_from_slice(delta.as_bytes());
    if combined.windows(MARKUP.len()).any(|bytes| bytes == MARKUP) {
        return Err("model tool markup is outside the permitted scope");
    }
    *tail = combined[combined.len().saturating_sub(MARKUP.len() - 1)..].to_vec();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scope() -> Scope {
        Scope::new(10, 20, "isolated-model".into())
    }
    fn call(name: &str, arguments: Value) -> Value {
        json!({"type":"function_call","id":"item-1","call_id":"call-1","name":name,"arguments":arguments.to_string()})
    }
    fn completed(output: Vec<Value>) -> Value {
        json!({"type":"response.completed","response":{"status":"completed","output":output,"usage":{"total_tokens":5}}})
    }
    fn sse(events: &[Value]) -> Vec<u8> {
        events
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect::<String>()
            .into_bytes()
    }

    mod readonly {
        use super::*;

        fn scope() -> Scope {
            Scope::readonly(10, 20, "isolated-model".into())
        }

        fn declaration(name: &str) -> Value {
            let definition = super::super::super::super::driver::catalog()
                .into_iter()
                .find(|definition| definition["name"] == name)
                .unwrap();
            let mut parameters = definition["inputSchema"].clone();
            parameters["properties"]
                .as_object_mut()
                .unwrap()
                .remove("session");
            if let Some(required) = parameters.get_mut("required").and_then(Value::as_array_mut) {
                required.retain(|field| field != "session");
            }
            json!({"type":"function","name":name,"description":definition["description"],
                "parameters":parameters,"strict":false})
        }

        fn request() -> Value {
            json!({"model":"isolated-model","stream":true,"reasoning":{"effort":"high"},
                "input":[{"role":"user","content":"Observe my fixture"}],
                "tools":[declaration("list_apps"),declaration("get_accessibility_tree"),
                    declaration("get_window_state"),declaration("get_agent_cursor_state"),
                    {"type":"function","name":"set_session_title","parameters":{"type":"object"},"strict":false}]})
        }

        #[test]
        fn preserves_request() {
            let mut body = request();
            let original = body.clone();
            scope().filter_request(&mut body).unwrap();
            assert_eq!(body, original);
            assert!(scope().preserves_request());
        }

        #[test]
        fn requires_non_strict_functions() {
            let mut body = request();
            body["tools"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"web_search"}));
            let original = body.clone();
            scope().filter_request(&mut body).unwrap();
            assert_eq!(body, original);

            for index in 0..original["tools"].as_array().unwrap().len() - 1 {
                for strict in [None, Some(true)] {
                    let mut body = original.clone();
                    let tool = body["tools"][index].as_object_mut().unwrap();
                    if let Some(strict) = strict {
                        tool.insert("strict".into(), json!(strict));
                    } else {
                        tool.remove("strict");
                    }
                    let unchanged = body.clone();
                    assert_eq!(
                        scope().filter_request(&mut body).unwrap_err(),
                        "read-only acceptance requires non-strict function tools",
                    );
                    assert_eq!(body, unchanged);
                }
            }
        }

        #[test]
        fn refuses_native_writes_and_session_selectors() {
            let mut body = request();
            body["tools"]
                .as_array_mut()
                .unwrap()
                .push(declaration("click"));
            assert!(scope().filter_request(&mut body).is_err());
            let mut body = request();
            body["tools"][0]["parameters"]["properties"]["session"] = json!({"type":"string"});
            assert!(scope().filter_request(&mut body).is_err());
        }

        #[tokio::test]
        async fn accepts_observations_and_isolated_title() {
            for (name, arguments) in [
                ("list_apps", json!({})),
                ("get_accessibility_tree", json!({})),
                (
                    "get_window_state",
                    json!({"pid":10,"window_id":20,
                    "include_accessibility_tree":true,"include_screenshot":false}),
                ),
                ("set_session_title", json!({"title":"Fixture observation"})),
            ] {
                let output = call(name, arguments);
                let response = scope()
                    .validate_response(&sse(&[completed(vec![output.clone()])]))
                    .await
                    .unwrap();
                assert_eq!(response["output"], json!([output]));
            }
        }

        #[tokio::test]
        async fn refuses_foreign_windows_screenshots_and_writes() {
            for (name, arguments) in [
                (
                    "get_window_state",
                    json!({"pid":11,"window_id":20,
                    "include_accessibility_tree":true,"include_screenshot":false}),
                ),
                (
                    "get_window_state",
                    json!({"pid":10,"window_id":21,
                    "include_accessibility_tree":true,"include_screenshot":false}),
                ),
                (
                    "get_window_state",
                    json!({"pid":10,"window_id":20,
                    "include_accessibility_tree":true,"include_screenshot":true}),
                ),
                (
                    "get_window_state",
                    json!({"pid":10,"window_id":20,
                    "include_accessibility_tree":true,"include_screenshot":false,
                    "screenshot_out_file":"capture.png"}),
                ),
                ("list_apps", json!({"session":"foreign"})),
                ("click", json!({"pid":10,"window_id":20,"x":1,"y":1})),
                ("get_desktop_state", json!({})),
            ] {
                assert!(
                    scope()
                        .validate_response(&sse(&[completed(vec![call(name, arguments)])]))
                        .await
                        .is_err()
                );
            }
        }

        #[tokio::test]
        async fn reports_refusal_reasons() {
            for (arguments, reason) in [
                (
                    json!({"pid":11,"window_id":20,
                        "include_accessibility_tree":true,"include_screenshot":false}),
                    "read-only observation pid does not match the owned fixture",
                ),
                (
                    json!({"pid":10,"window_id":21,
                        "include_accessibility_tree":true,"include_screenshot":false}),
                    "read-only observation window_id does not match the owned fixture",
                ),
                (
                    json!({"pid":10,"window_id":20,"include_screenshot":false}),
                    "read-only observation omitted include_accessibility_tree",
                ),
                (
                    json!({"pid":10,"window_id":20,
                        "include_accessibility_tree":false,"include_screenshot":false}),
                    "read-only observation include_accessibility_tree is not true",
                ),
                (
                    json!({"pid":10,"window_id":20,"include_accessibility_tree":true}),
                    "read-only observation omitted include_screenshot",
                ),
                (
                    json!({"pid":10,"window_id":20,
                        "include_accessibility_tree":true,"include_screenshot":true}),
                    "read-only observation include_screenshot is not false",
                ),
                (
                    json!({"pid":10,"window_id":20,
                        "include_accessibility_tree":true,"include_screenshot":false,
                        "screenshot_out_file":"capture.png"}),
                    "read-only observation requested screenshot_out_file",
                ),
            ] {
                assert_eq!(
                    scope()
                        .validate_response(&sse(&[completed(vec![call(
                            "get_window_state",
                            arguments
                        )])]))
                        .await
                        .unwrap_err(),
                    reason,
                );
            }
        }
    }

    #[test]
    fn filters_without_changing_native_declarations() {
        let allowed =
            json!({"type":"function","name":"get_window_state","parameters":{"type":"object"}});
        let mut body = json!({"model":"isolated-model","stream":true,"input":[{"role":"user","content":"task"}],
            "tools":[allowed.clone(),{"type":"web_search"},{"type":"function","name":"bring_to_front"}]});
        let input = body["input"].clone();
        scope().filter_request(&mut body).unwrap();
        assert_eq!(body["tools"], json!([allowed]));
        assert_eq!(body["input"], input);
    }

    #[tokio::test]
    async fn accepts_original_owned_window_call() {
        let output = call(
            "get_window_state",
            json!({"pid":10,"window_id":20,"include_screenshot":false}),
        );
        let response = scope()
            .validate_response(&sse(&[completed(vec![output.clone()])]))
            .await
            .unwrap();
        assert_eq!(response["output"], json!([output]));
        assert_eq!(response["usage"]["total_tokens"], 5);
    }

    #[tokio::test]
    async fn accepts_native_click_target_forms() {
        for arguments in [
            json!({"target":{"kind":"window","pid":10,"window_id":20},"delivery_mode":"background","x":10,"y":20}),
            json!({"pid":10,"window_id":20,"scope":"window","delivery_mode":"background","x":10,"y":20}),
        ] {
            let output = call("click", arguments);
            let response = scope()
                .validate_response(&sse(&[completed(vec![output.clone()])]))
                .await
                .unwrap();
            assert_eq!(response["output"], json!([output]));
        }
    }

    #[tokio::test]
    async fn refuses_outside_target_and_foreground_input() {
        for arguments in [
            json!({"target":{"kind":"window","pid":11,"window_id":20},"delivery_mode":"background","x":10,"y":20}),
            json!({"target":{"kind":"window","pid":10,"window_id":21},"delivery_mode":"background","x":10,"y":20}),
            json!({"target":{"kind":"window","pid":10,"window_id":20},"x":10,"y":20,"delivery_mode":"foreground"}),
            json!({"target":{"kind":"desktop","display_id":"primary"},"delivery_mode":"foreground","x":10,"y":20}),
            json!({"pid":11,"window_id":20,"scope":"window","delivery_mode":"background","x":10,"y":20}),
            json!({"pid":10,"window_id":21,"scope":"window","delivery_mode":"background","x":10,"y":20}),
        ] {
            assert!(
                scope()
                    .validate_response(&sse(&[completed(vec![call("click", arguments)])]))
                    .await
                    .is_err()
            );
        }
        for name in ["computer_batch", "bring_to_front", "launch_app", "shell"] {
            assert!(
                scope()
                    .validate_response(&sse(&[completed(vec![call(name, json!({}))])]))
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn rejects_incomplete_or_duplicate_terminals() {
        let valid = completed(vec![call(
            "get_window_state",
            json!({"pid":10,"window_id":20}),
        )]);
        assert!(
            scope()
                .validate_response(&sse(&[valid.clone(), valid]))
                .await
                .is_err()
        );
        assert!(
            scope()
                .validate_response(&sse(&[json!({"type":"response.incomplete"})]))
                .await
                .is_err()
        );
        assert!(scope().validate_response(b"data: {}\n\n").await.is_err());
    }
}
