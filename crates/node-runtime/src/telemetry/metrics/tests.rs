use super::*;
use chrono::{TimeZone, Utc};
use sailry_protocol::{
    Authentication, ErrorCode, ProviderId, WorkMode,
    conversation::{ModelApi, presets},
};
use serde_json::{Value, json};

fn window() -> aggregate::Window {
    aggregate::Window::new(
        1,
        Control::fixture(false, true).metadata(),
        Utc.timestamp_opt(1, 0).unwrap(),
    )
}

#[test]
fn aggregates_dimensions_and_totals() {
    let mut window = window();
    for elapsed in [Duration::from_millis(3), Duration::from_millis(7)] {
        window.add(Observation::Tool {
            tool: Tool::builtin("read_session").unwrap(),
            result: ToolResult::Success,
            error: None,
            elapsed,
        });
    }
    window.add(Observation::Tool {
        tool: Tool::builtin("read_session").unwrap(),
        result: ToolResult::Denied,
        error: Some(ErrorCode::PermissionDenied),
        elapsed: Duration::ZERO,
    });
    let batches = window.finish(Utc.timestamp_opt(901, 0).unwrap());
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].table, "tool_calls");
    assert_eq!(batches[0].data.len(), 2);
    let row = batches[0]
        .data
        .iter()
        .find(|row| row["result"] == "success")
        .unwrap();
    assert_eq!(row["count"], 2);
    assert_eq!(row["elapsed_us_total"], 10_000);
    assert_eq!(row["elapsed_us_max"], 7_000);
    assert_eq!(row["window_start"], "1970-01-01T00:00:01Z");
    assert_eq!(row["window_end"], "1970-01-01T00:15:01Z");
    assert!(uuid::Uuid::parse_str(row["event_id"].as_str().unwrap()).is_ok());
}

#[test]
fn limits_groups_and_saturates_counts() {
    let mut window = window();
    for retry in 0..aggregate::GROUP_LIMIT + 10 {
        window.add(Observation::Model {
            model: Model::capture(&provider(), "private-model"),
            result: RequestResult::Error,
            status: StatusClass::Unknown,
            error: Some(ErrorCode::Unavailable),
            retries: Some(retry as u64),
            tokens: Tokens {
                input: u64::MAX,
                ..Default::default()
            },
        });
    }
    window.add(Observation::Model {
        model: Model::capture(&provider(), "private-model"),
        result: RequestResult::Error,
        status: StatusClass::Unknown,
        error: Some(ErrorCode::Unavailable),
        retries: Some(0),
        tokens: Tokens {
            input: 1,
            ..Default::default()
        },
    });
    let batches = window.finish(Utc::now());
    assert_eq!(batches[0].data.len(), aggregate::GROUP_LIMIT);
    let first = batches[0]
        .data
        .iter()
        .find(|row| row["retries"] == 0)
        .unwrap();
    assert_eq!(first["count"], 2);
    assert_eq!(first["input"].as_u64(), Some(u64::MAX));
}

fn provider() -> sailry_protocol::conversation::Provider {
    serde_json::from_value(json!({
        "id": ProviderId::new(), "revision": 1, "name":"PRIVATE_NAME",
        "api":"responses", "authentication":"api_key", "endpoint":"https://private.invalid/secret",
        "enabled":true, "default_model":"PRIVATE_MODEL", "credential":null
    }))
    .unwrap()
}

#[test]
fn admits_only_static_builtin_models() {
    let mut provider = provider();
    let id = "fixed-preset".to_owned();
    let model = "fixed-model".to_owned();
    let allowed = vec![(id.clone(), vec![model.clone()])];
    provider.preset = Some(presets::PresetRef {
        source: presets::Source::Builtin,
        id,
        route: None,
    });
    assert_eq!(
        serde_json::to_value(Model::selected(&provider, &model, &allowed)).unwrap()["model"],
        model
    );
    assert_eq!(
        serde_json::to_value(Model::selected(&provider, "PRIVATE_MODEL", &allowed)).unwrap()["model"],
        "custom"
    );
    provider.preset.as_mut().unwrap().source = presets::Source::Package {
        name: "private-package".into(),
    };
    assert_eq!(
        serde_json::to_value(Model::selected(&provider, &model, &allowed)).unwrap()["model"],
        "custom"
    );
    provider.preset.as_mut().unwrap().source = presets::Source::Partner;
    assert_eq!(
        serde_json::to_value(Model::selected(&provider, &model, &allowed)).unwrap()["model"],
        "custom"
    );
}

#[test]
fn payloads_have_no_content_fields() {
    let mut window = window();
    window.add(Observation::Launch);
    window.add(Observation::Turn {
        result: TurnResult::Cancelled,
        mode: WorkMode::Plan,
        subagent: true,
        error: Some(ErrorCode::Cancelled),
    });
    window.add(Observation::Model {
        model: Model::capture(&provider(), "PRIVATE_MODEL"),
        result: RequestResult::Error,
        status: StatusClass::from(Some(401)),
        error: Some(ErrorCode::PermissionDenied),
        retries: None,
        tokens: Tokens {
            input: 10,
            output: 3,
            cache_read: 2,
            cache_write: 1,
        },
    });
    window.add(Observation::Tool {
        tool: Tool::mcp(),
        result: ToolResult::Success,
        error: None,
        elapsed: Duration::ZERO,
    });
    window.add(Observation::Search {
        driver: Some(sailry_protocol::search::Driver::Exa),
        result: RequestResult::Success,
        error: None,
        elapsed: Duration::from_millis(2),
    });
    let value = serde_json::to_value(window.finish(Utc::now())).unwrap();
    let encoded = value.to_string();
    for forbidden in [
        "PRIVATE_",
        "private.invalid",
        "provider_id",
        "session_id",
        "turn_id",
        "arguments",
        "url",
        "path",
        "message",
    ] {
        assert!(
            !encoded.contains(forbidden),
            "payload contains a forbidden dimension"
        );
    }
    assert_eq!(value.as_array().unwrap().len(), 5);
    let models = value
        .as_array()
        .unwrap()
        .iter()
        .find(|batch| batch["table"] == "model_requests")
        .unwrap();
    assert_eq!(models["data"][0]["api"], "responses");
    assert_eq!(models["data"][0]["authentication"], "api_key");
    assert_eq!(models["data"][0]["http_status_class"], "4xx");
    assert_eq!(models["data"][0]["cache_write"], 1);
}

#[test]
fn uses_only_table_fields() {
    let observations = [
        Observation::Launch,
        Observation::Turn {
            result: TurnResult::Completed,
            mode: WorkMode::Code,
            subagent: false,
            error: None,
        },
        Observation::Tool {
            tool: Tool::mcp(),
            result: ToolResult::Success,
            error: None,
            elapsed: Duration::ZERO,
        },
        Observation::Model {
            model: Model::capture(&provider(), "PRIVATE_MODEL"),
            result: RequestResult::Success,
            status: StatusClass::Unknown,
            error: None,
            retries: None,
            tokens: Tokens::default(),
        },
        Observation::Search {
            driver: Some(sailry_protocol::search::Driver::Exa),
            result: RequestResult::Success,
            error: None,
            elapsed: Duration::ZERO,
        },
    ];
    let common = [
        "installation_id",
        "release",
        "channel",
        "application",
        "os",
        "arch",
        "window_start",
        "window_end",
        "event_id",
        "count",
    ];
    for observation in observations {
        let mut window = window();
        window.add(observation);
        let batches = window.finish(Utc::now());
        let batch = &batches[0];
        let fields: &[&str] = match batch.table {
            "app_launches" => &[],
            "turns" => &["result", "mode", "subagent", "error_code"],
            "tool_calls" => &[
                "tool",
                "result",
                "error_code",
                "elapsed_us_total",
                "elapsed_us_max",
            ],
            "model_requests" => &[
                "api",
                "authentication",
                "model",
                "result",
                "http_status_class",
                "error_code",
                "retries",
                "input",
                "output",
                "cache_read",
                "cache_write",
            ],
            "search_requests" => &["driver", "result", "error_code", "elapsed_us_total"],
            _ => panic!("unknown metrics table"),
        };
        let expected: std::collections::BTreeSet<_> =
            common.iter().chain(fields.iter()).copied().collect();
        let actual: std::collections::BTreeSet<_> = batch.data[0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(actual, expected, "unexpected fields for {}", batch.table);
        if batch.table == "model_requests" {
            for field in ["input", "output", "cache_read", "cache_write"] {
                assert_eq!(batch.data[0][field], 0);
            }
        }
    }
}

#[test]
fn drops_stale_and_disabled_observations() {
    let control = Control::fixture(false, true);
    let epoch = control.epoch();
    let mut window = None;
    add(
        &control,
        &mut window,
        Envelope {
            epoch,
            observation: Observation::Launch,
        },
    );
    assert!(window.is_some());
    control.fixture_set(false, false);
    add(
        &control,
        &mut window,
        Envelope {
            epoch,
            observation: Observation::Launch,
        },
    );
    let batches = window.take().unwrap().finish(Utc::now());
    assert_eq!(batches[0].data[0]["count"], 1);
    control.fixture_set(false, true);
    add(
        &control,
        &mut window,
        Envelope {
            epoch,
            observation: Observation::Launch,
        },
    );
    assert!(window.is_none());
    add(
        &control,
        &mut window,
        Envelope {
            epoch: control.epoch(),
            observation: Observation::Launch,
        },
    );
    assert_eq!(window.unwrap().finish(Utc::now())[0].data[0]["count"], 1);
}

#[test]
fn queue_never_waits_for_capacity() {
    let (records, mut received) = mpsc::channel(1);
    let (shutdown, _closing) = mpsc::channel(1);
    let metrics = Metrics {
        control: Control::fixture(false, true),
        records,
        clear: Arc::new(Notify::new()),
        shutdown,
    };
    metrics.record(Observation::Launch);
    metrics.record(Observation::Launch);
    assert!(received.try_recv().is_ok());
    assert!(received.try_recv().is_err());
}

#[test]
fn rejects_stale_capture_epochs() {
    let (records, mut received) = mpsc::channel(1);
    let (shutdown, _closing) = mpsc::channel(1);
    let control = Control::fixture(false, true);
    let epoch = control.epoch();
    let metrics = Metrics {
        control: control.clone(),
        records,
        clear: Arc::new(Notify::new()),
        shutdown,
    };
    control.fixture_set(false, false);
    control.fixture_set(false, true);
    metrics.record_at(epoch, Observation::Launch);
    assert!(received.try_recv().is_err());
    metrics.record_at(control.epoch(), Observation::Launch);
    assert_eq!(received.try_recv().unwrap().epoch, control.epoch());
}

#[test]
fn excludes_dynamic_builtin_names() {
    assert!(Tool::builtin("PRIVATE_TOOL").is_none());
    for name in ["manage_mcp", "manage_skills"] {
        assert_eq!(
            serde_json::to_value(Tool::capture(name, &[]).unwrap()).unwrap(),
            json!({"source":"builtin","name":name})
        );
    }
    assert_eq!(
        serde_json::to_value(Tool::mcp()).unwrap(),
        json!({"source":"mcp"})
    );
    for (api, auth) in [
        (ModelApi::Responses, Authentication::ApiKey),
        (ModelApi::Anthropic, Authentication::ClaudeOAuth),
        (ModelApi::Plugin, Authentication::Plugin),
    ] {
        let mut provider = provider();
        provider.api = api;
        provider.authentication = auth;
        assert_eq!(
            serde_json::to_value(Model::capture(&provider, "PRIVATE_MODEL")).unwrap()["model"],
            Value::String("custom".into())
        );
    }
}

#[test]
fn uses_captured_package_tool_identifiers() {
    let mut package:sailry_protocol::plugin::Info=serde_json::from_value(json!({
        "summary":{"name":"fixture-tools","revision":1,"digest":"synthetic","settings_revision":0,"enabled":true,"version":"0.1.0","description":"PRIVATE_DESCRIPTION"},
        "extension":{"api_version":"v1","instructions":"PRIVATE_INSTRUCTIONS","tools":[{"name":"lookup","operation":"web.search","description":"PRIVATE_TOOL_DESCRIPTION"}]}
    })).unwrap();
    let alias = crate::plugins::tools::alias("fixture-tools", "lookup");
    for origin in [
        sailry_protocol::plugin::Origin::Bundled,
        serde_json::from_value(json!({
            "kind":"online", "path":"fixture-tools",
            "source":{"repository":"https://github.com/sailry/sailry-plugins.git", "git_ref":"main", "commit":"0".repeat(40)}
        })).unwrap(),
    ] {
        package.origin = Some(origin);
        let identity = Tool::capture(&alias, std::slice::from_ref(&package)).unwrap();
        assert_eq!(
            serde_json::to_value(identity).unwrap(),
            json!({"source":"package","package":"fixture-tools","version":"0.1.0","name":"lookup"})
        );
    }
    assert!(Tool::capture("PRIVATE_UNREGISTERED_TOOL", &[package]).is_none());
    assert_eq!(
        serde_json::to_value(Tool::capture("mcp_PRIVATE_SERVER_PRIVATE_TOOL", &[]).unwrap())
            .unwrap(),
        json!({"source":"mcp"})
    );
}

#[test]
fn recognizes_frozen_bundled_tools() {
    let package = crate::plugins::Host::new(None)
        .inspect_bundled("files", sailry_link::CancellationToken::new())
        .unwrap();
    assert!(package.origin.is_none());
    let alias = crate::plugins::tools::alias("files", "read_file");
    assert_eq!(
        serde_json::to_value(Tool::capture(&alias, std::slice::from_ref(&package)).unwrap())
            .unwrap(),
        json!({"source":"package", "package":"files", "version":package.summary.version, "name":"read_file"})
    );
    let mut changed = package;
    changed.summary.digest = "PRIVATE_DIGEST".into();
    assert_eq!(
        serde_json::to_value(Tool::capture(&alias, &[changed]).unwrap()).unwrap(),
        json!({"source":"third_party"})
    );
}

#[test]
fn groups_private_package_tools() {
    let mut window = window();
    for origin in [
        Value::Null,
        json!({"kind":"directory"}),
        json!({"kind":"archive"}),
        json!({
            "kind":"online", "path":"web-search",
            "source":{"repository":"https://github.com/private/plugins.git", "git_ref":"main", "commit":"0".repeat(40)}
        }),
        json!({
            "kind":"online", "path":"web-search",
            "source":{"repository":"https://github.com/sailry/sailry-plugins.git", "git_ref":"private", "commit":"0".repeat(40)}
        }),
    ] {
        // Even an official package name is private without public provenance.
        let package: sailry_protocol::plugin::Info = serde_json::from_value(json!({
            "origin":origin,
            "summary":{"name":"web-search", "revision":1, "digest":"PRIVATE_DIGEST", "settings_revision":0, "enabled":true, "version":"0.1.0+PRIVATE_VERSION"},
            "extension":{"api_version":"v1", "tools":[{"name":"private_lookup", "operation":"web.search", "description":"PRIVATE_DESCRIPTION"}]}
        })).unwrap();
        let alias = crate::plugins::tools::alias("web-search", "private_lookup");
        window.add(Observation::Tool {
            tool: Tool::capture(&alias, &[package]).unwrap(),
            result: ToolResult::Success,
            error: None,
            elapsed: Duration::ZERO,
        });
    }
    let batches = serde_json::to_value(window.finish(Utc::now())).unwrap();
    assert_eq!(batches[0]["data"].as_array().unwrap().len(), 1);
    assert_eq!(batches[0]["data"][0]["count"], 5);
    assert_eq!(
        batches[0]["data"][0]["tool"],
        json!({"source":"third_party"})
    );
    let encoded = batches.to_string();
    for private in [
        "PRIVATE_",
        "web-search",
        "private_lookup",
        "private/plugins",
    ] {
        assert!(!encoded.contains(private));
    }
}
