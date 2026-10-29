use super::*;
use crate::plugins::tools::alias;
use goose_provider_types::conversation::message::ToolConfirmationRequest;
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
use sailry_link::CancellationToken;
use serde_json::{Value, json};

fn exposure(declared: &[(&str, &str)], registered: &[&str], claude: bool) -> Exposure {
    let declared: Vec<_> = declared
        .iter()
        .map(|(package, tool)| (alias(package, tool), (*tool).to_owned()))
        .collect();
    let mut registered: BTreeSet<_> = registered.iter().map(|name| (*name).to_owned()).collect();
    registered.extend(declared.iter().map(|(alias, _)| alias.clone()));
    Exposure::new(declared, registered, claude)
}

fn tool(name: impl Into<String>) -> Tool {
    Tool::new(name.into(), "description", serde_json::Map::new())
}

fn call(id: &str, name: impl Into<String>) -> Message {
    Message::assistant().with_tool_request(id, Ok(CallToolRequestParams::new(name.into())))
}

fn called(message: &Message) -> &str {
    let MessageContentBlock::ToolRequest(request) = &message.content[0] else {
        panic!("tool call expected")
    };
    request.tool_call.as_ref().unwrap().name.as_ref()
}

fn result(message: &Message) -> &CallToolResult {
    let MessageContentBlock::ToolResponse(response) = &message.content[0] else {
        panic!("tool result expected")
    };
    response.tool_result.as_ref().unwrap()
}

#[test]
fn unique_names_round_trip_exactly() {
    let names = exposure(&[("files", "read_file")], &["tool_search"], false);
    let tools = [tool(alias("files", "read_file")), tool("tool_search")];
    let sent = names.tools(&tools);
    assert_eq!(sent[0].name, "read_file");
    assert_eq!(sent[1], tools[1]);
    let history = [call("file", alias("files", "read_file"))];
    let mut sent = names.messages(&history).into_owned();
    assert_eq!(called(&sent[0]), "read_file");
    names.restore(&mut sent[0]);
    assert_eq!(sent, history);
    for unknown in ["READ_FILE", "read_file_suffix", "missing"] {
        let mut response = call("unknown", unknown);
        names.restore(&mut response);
        assert_eq!(called(&response), unknown);
    }
}

#[test]
fn reserves_deferred_and_internal_names() {
    let names = exposure(
        &[
            ("first", "read"),
            ("second", "read"),
            ("third", "install_plugin"),
        ],
        &["install_plugin"],
        false,
    );
    for (package, name) in [
        ("first", "read"),
        ("second", "read"),
        ("third", "install_plugin"),
    ] {
        let canonical = alias(package, name);
        assert_eq!(names.name(&canonical), canonical);
        // Filtering the current request cannot change the frozen decision.
        assert_eq!(names.tools(&[tool(canonical.clone())])[0].name, canonical);
    }
    let occupied = alias("first", "read");
    let names = exposure(&[("first", "read"), ("second", &occupied)], &[], false);
    assert_eq!(names.name(&alias("first", "read")), "read");
    assert_eq!(
        names.name(&alias("second", &occupied)),
        alias("second", &occupied)
    );
}

#[test]
fn captures_only_eligible_package_declarations() {
    let package = |name: &str| -> Info {
        serde_json::from_value(json!({
            "summary":{"name":name,"revision":1,"digest":"fixture","settings_revision":0,
                "enabled":true,"version":"0.1.0"},
            "extension":{"api_version":"v1","tools":[{"name":"read","operation":"files.read"}]}
        }))
        .unwrap()
    };
    let registration = |owner: &str| {
        let mut entry = crate::agent::title::bind("session".into(), CancellationToken::new());
        entry.tool = tool(alias(owner, "read"));
        entry.plugin = Some(owner.into());
        entry.deferred = true;
        entry
    };
    let plugins = [package("eligible"), package("unavailable")];
    let tools = [registration("eligible")];
    let names = Exposure::capture(&plugins, &tools, false);
    assert_eq!(names.name(&alias("eligible", "read")), "read");
    assert_eq!(
        names.name(&alias("unavailable", "read")),
        alias("unavailable", "read")
    );
    let history = [
        call("old", alias("unavailable", "read")),
        call("new", alias("eligible", "read")),
    ];
    let sent = names.messages(&history);
    assert_eq!(called(&sent[0]), called(&history[0]));
    assert_eq!(called(&sent[1]), "read");
    let tools = [registration("eligible"), registration("unavailable")];
    let names = Exposure::capture(&plugins, &tools, false);
    assert_eq!(
        names.name(&alias("eligible", "read")),
        alias("eligible", "read")
    );
}

#[test]
fn reserves_only_exact_internal_namespace() {
    for historical in [
        alias("unavailable", "read"),
        crate::agent::mcp::alias("a", "b", "read"),
    ] {
        let names = exposure(&[("current", &historical)], &[], false);
        assert_eq!(
            names.name(&alias("current", &historical)),
            alias("current", &historical)
        );
        let history = [
            call("old", historical.clone()),
            call("new", alias("current", &historical)),
        ];
        assert_eq!(names.messages(&history).as_ref(), history);
    }
    for name in [
        "plugin_custom_read",
        "plugin_0123456789abcde_read",
        "plugin_0123456789abcdeF_read",
        "plugin_0123456789abcdef_",
        "mcp_custom_read",
        "mcp_read_0123456789abcdef0123456",
        "mcp_read_0123456789abcdef0123456F",
        "mcp__0123456789abcdef01234567",
    ] {
        let names = exposure(&[("current", name)], &[], false);
        assert_eq!(names.name(&alias("current", name)), name);
    }
}

#[test]
fn preserves_body_arguments_and_confirmation_prompts() {
    let canonical = alias("files", "read_file");
    let names = exposure(&[("files", "read_file")], &[], false);
    let text = format!("User, skill, file, command and MCP text: {canonical}");
    let mut request = call("file", canonical.clone());
    let MessageContentBlock::ToolRequest(block) = &mut request.content[0] else {
        unreachable!()
    };
    block.tool_call.as_mut().unwrap().arguments =
        Some(serde_json::from_value(json!({"path":canonical})).unwrap());
    let mut confirmation = Message::assistant();
    confirmation
        .content
        .push(MessageContentBlock::ToolConfirmationRequest(
            ToolConfirmationRequest {
                id: "confirm".into(),
                tool_name: canonical.clone(),
                arguments: serde_json::from_value(json!({"text":text})).unwrap(),
                prompt: Some(text.clone()),
            },
        ));
    let history = [
        Message::user().with_text(text.clone()),
        Message::assistant()
            .with_text(text.clone())
            .with_thinking(text.clone(), "signature"),
        request,
        Message::user().with_tool_response(
            "file",
            Ok(CallToolResult::success(vec![ContentBlock::text(text)])),
        ),
        confirmation,
    ];
    let mut sent = names.messages(&history).into_owned();
    assert_eq!(&sent[..2], &history[..2]);
    assert_eq!(sent[3], history[3]);
    let MessageContentBlock::ToolRequest(block) = &sent[2].content[0] else {
        unreachable!()
    };
    assert_eq!(
        block
            .tool_call
            .as_ref()
            .unwrap()
            .arguments
            .as_ref()
            .unwrap()["path"],
        canonical
    );
    let MessageContentBlock::ToolConfirmationRequest(block) = &sent[4].content[0] else {
        unreachable!()
    };
    assert_eq!(block.tool_name, "read_file");
    names.restore(&mut sent[2]);
    names.restore(&mut sent[4]);
    assert_eq!(sent, history);
}

#[test]
fn projects_only_owned_discovery_fields() {
    let canonical = alias("files", "read_file");
    let names = exposure(&[("files", "read_file")], &[], false);
    let value =
        json!({"tools":[{"name":canonical,"description":canonical}],"sailry_context":canonical});
    let mut raw = CallToolResult::structured(value.clone());
    raw.content.push(ContentBlock::text(canonical.clone()));
    let history = [
        call("discovery", "tool_search"),
        Message::user().with_tool_response("discovery", Ok(raw.clone())),
        call("mcp", "mcp_peer_lookup"),
        Message::user().with_tool_response("mcp", Ok(raw.clone())),
    ];
    let sent = names.messages(&history);
    let projected = result(&sent[1]);
    let body = projected.structured_content.as_ref().unwrap();
    assert_eq!(body["tools"][0]["name"], "read_file");
    assert_eq!(body["tools"][0]["description"], canonical);
    assert_eq!(body["sailry_context"], canonical);
    assert_eq!(
        serde_json::from_str::<Value>(&projected.content[0].as_text().unwrap().text).unwrap(),
        *body
    );
    assert_eq!(projected.content[1], raw.content[1]);
    assert_eq!(result(&sent[3]), &raw);
    assert_eq!(result(&history[1]), &raw);
    assert_eq!(names.messages(&sent).as_ref(), sent.as_ref());
}

#[test]
fn reserves_claude_wire_names() {
    let names = exposure(&[("first", "read"), ("second", "Read")], &[], true);
    for (package, name) in [("first", "read"), ("second", "Read")] {
        assert_eq!(names.name(&alias(package, name)), alias(package, name));
    }
    let names = exposure(&[("first", "read")], &[], true);
    assert_eq!(names.name(&alias("first", "read")), "Read");
    let mut response = call("read", "Read");
    names.restore(&mut response);
    assert_eq!(called(&response), alias("first", "read"));
    let mut response = call("other", "READ");
    names.restore(&mut response);
    assert_eq!(called(&response), "READ");
}

#[test]
fn summary_keeps_identity_without_registration() {
    let registry = Registry::default();
    assert!(registry.get(crate::agent::model::Purpose::Main).is_err());
    let names = registry
        .get(crate::agent::model::Purpose::Compaction)
        .unwrap();
    let history = [call("file", alias("files", "read_file"))];
    assert_eq!(names.messages(&history).as_ref(), history);
    let tools = [tool(alias("files", "read_file"))];
    assert_eq!(names.tools(&tools).as_ref(), tools);
}
