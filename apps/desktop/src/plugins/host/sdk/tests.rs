use super::*;
mod git;
mod notifications;
mod sessions;
mod transactions;
use crate::plugins::fixture::Fixture;
use gpui_shell::HostArguments;

mod changes;
mod conversation;

fn args(values: Value) -> HostArguments {
    let HostValue::Array(values) = encode(values).unwrap() else {
        panic!("argument array expected")
    };
    HostArguments::new(values)
}

fn call(module: &HostModule, name: &str, arguments: Value) -> Value {
    decode(&module.call(name, &args(arguments)).unwrap()).unwrap()
}

fn complete(fixture: &Fixture, module: &HostModule, name: &str, arguments: Value) -> Value {
    decode(
        &fixture
            .runtime
            .block_on(module.begin(name, &args(arguments)).unwrap())
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn storage_uses_scoped_node_receipts_on_both_connections() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (package, _server) =
            fixture.game_plugin("city-trader", &["player_1_model", "player_2_model"]);
        let host = Host::new(
            fixture.binding.client.clone(),
            Context {
                invocation: None,
                turn: None,
                surface: Default::default(),
                package: package.summary.reference(),
                worktree: Some(fixture.session.worktree),
                session: Some(fixture.session.id),
            },
            fixture.runtime.handle().clone(),
            false,
            None,
        );
        let module = host.sdk();
        module.validate().unwrap();
        let entry = complete(&fixture, &module, "getValue", json!(["prefs"]));
        assert_eq!(
            entry,
            json!({"key":"prefs","revision":"0","value":null,"present":false})
        );
        let id = call(
            &module,
            "setValue",
            json!(["prefs",{"enabled":true,"amount":3},entry["revision"]]),
        );
        let first = complete(&fixture, &module, "completeRequest", json!([id]));
        assert_eq!(first["Ok"]["data"]["revision"], "1");
        assert_eq!(first["Ok"]["data"]["value"]["amount"], 3);
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([id])),
            first
        );
        call(&module, "forgetRequest", json!([id]));
        assert!(host.requests.lock().unwrap().is_empty());
        assert_eq!(
            complete(&fixture, &module, "listKeys", json!([]))["keys"],
            json!(["prefs"])
        );
        let stale = call(&module, "setValue", json!(["prefs", null, "0"]));
        let fault = complete(&fixture, &module, "completeRequest", json!([stale]));
        assert_eq!(fault["Err"]["code"], "revision_conflict");
        call(&module, "forgetRequest", json!([stale]));
        let id = call(&module, "deleteValue", json!(["prefs", "1"]));
        let deleted = complete(&fixture, &module, "completeRequest", json!([id]));
        assert_eq!(deleted["Ok"]["data"]["revision"], "2");
        assert_eq!(deleted["Ok"]["data"]["present"], false);
        call(&module, "forgetRequest", json!([id]));
        assert_eq!(
            complete(&fixture, &module, "listKeys", json!([]))["keys"],
            json!([])
        );
        let id = call(&module, "setValue", json!(["prefs", null, "2"]));
        let stored = complete(&fixture, &module, "completeRequest", json!([id]));
        assert_eq!(stored["Ok"]["data"]["present"], true);
        assert_eq!(
            complete(&fixture, &module, "getValue", json!(["prefs"])),
            stored["Ok"]["data"]
        );
        call(&module, "forgetRequest", json!([id]));
        let indexed = call(
            &module,
            "indexedValue",
            json!(["prefs",{"enabled":true},{"fields":["literal",""],"tags":["opaque"],"order":1},"3"]),
        );
        let indexed_result = complete(&fixture, &module, "completeRequest", json!([indexed]));
        assert_eq!(indexed_result["Ok"]["data"]["revision"], "4");
        call(&module, "forgetRequest", json!([indexed]));
        let page = complete(
            &fixture,
            &module,
            "searchValues",
            json!([{"terms":["literal"],"all":["opaque"],"limit":1}]),
        );
        assert_eq!(page["entries"][0]["key"], "prefs");
        assert_eq!(page["entries"][0]["revision"], "4");
        assert!(page["now_ms"].as_u64().unwrap() > 0);
        let ordinary = call(&module, "setValue", json!(["prefs", null, "4"]));
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([ordinary]))["Ok"]["data"]["revision"],
            "5"
        );
        call(&module, "forgetRequest", json!([ordinary]));
        assert_eq!(
            complete(
                &fixture,
                &module,
                "searchValues",
                json!([{"terms":["literal"]}])
            )["entries"],
            json!([])
        );
        let denied = call(
            &module,
            "prepareRequest",
            json!([{"kind":"read_file","data":{
                "worktree":fixture.session.worktree,"path":"missing.txt"
            }}]),
        );
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([denied]))["Err"]["code"],
            "permission_denied"
        );
        call(&module, "forgetRequest", json!([denied]));
        host.close();
        assert!(module.begin("getValue", &args(json!(["prefs"]))).is_err());
        fixture.close();
    }
}

#[test]
fn storage_revision_survives_javascript_precision_limit() {
    let output = public_output(Output::PluginValue(
        sailry_protocol::plugin::storage::Entry {
            key: "prefs".into(),
            revision: i64::MAX as u64,
            value: Value::Null,
            present: true,
        },
    ))
    .unwrap();
    assert_eq!(
        decode(&encode(output).unwrap()).unwrap()["data"]["revision"],
        "9223372036854775807"
    );
}

#[test]
fn settings_revisions_remain_exact() {
    let package = sailry_protocol::plugin::Reference {
        name: "example".into(),
        digest: "digest".into(),
        settings_revision: 9007199254740993,
    };
    let output = public_output(Output::PluginSettings(
        sailry_protocol::plugin::settings::State {
            package: package.clone(),
            values: Default::default(),
            configured: vec![],
            keepable: vec![],
            ready: true,
        },
    ))
    .unwrap();
    assert_eq!(
        decode(&encode(output).unwrap()).unwrap()["data"]["package"]["settings_revision"],
        "9007199254740993"
    );
    let command = values::command(json!({"kind":"read_plugin_settings","data":{"package":{
        "name":"example","digest":"digest","settings_revision":"9007199254740993"
    }}}))
    .unwrap();
    assert_eq!(
        command,
        Command::ReadPluginSettings {
            package: package.clone()
        }
    );
    for revision in [
        json!(9007199254740992u64),
        json!("01"),
        json!("-1"),
        json!("1.0"),
    ] {
        assert!(
            values::command(json!({"kind":"read_plugin_settings","data":{"package":{
                "name":"example","digest":"digest","settings_revision":revision
            }}}))
            .is_err()
        );
    }
    for output in [Output::PluginSecret(None), Output::ProviderKey(None)] {
        assert!(public_output(output).is_err());
    }
}
