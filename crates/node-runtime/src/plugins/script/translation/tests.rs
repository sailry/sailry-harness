use super::*;
use serde_json::json;
use std::collections::BTreeMap;

fn bundle(source: &str) -> Bundle {
    Bundle {
        entry: "dev.sailry.platform/adapter.js".into(),
        files: BTreeMap::from([("dev.sailry.platform/adapter.js".into(), source.into())]),
    }
}

#[test]
fn loads_only_the_pure_adapter_helper() {
    let mut session = Session::new(bundle(
        "import { version, parseJson } from 'sailry/adapter'; export function decode(input) {return {version, value:parseJson(input), io:[typeof fetch,typeof __sailryDispatch]};}",
    ), CancellationToken::new()).unwrap();
    assert_eq!(
        session
            .call("decode", &json!("{\"text\":\"fixture\"}"), 4096)
            .unwrap(),
        json!({"version":"v1","value":{"text":"fixture"},"io":["undefined","undefined"]})
    );
    assert!(session.call("decode", &json!("not JSON"), 4096).is_err());
    let mut replacement = bundle("export function prepare() {return null}");
    replacement.files.insert(
        "sailry/adapter".into(),
        "export const version='other';".into(),
    );
    assert!(Session::new(replacement, CancellationToken::new()).is_err());
}

#[test]
fn preserves_stream_state_without_host_access() {
    let mut bundle = bundle(
        r#"import { label } from './label.js';
        let count = 0;
        export function prepare(input) {
          return {label, input, unavailable: [typeof fetch, typeof process,
            typeof console, typeof __sailryDispatch, typeof require]};
        }
        export async function decode(input) { return {count: ++count, input}; }"#,
    );
    bundle.files.insert(
        "dev.sailry.platform/label.js".into(),
        "export const label = 'example';".into(),
    );
    let mut session = Session::new(bundle, CancellationToken::new()).unwrap();
    let prepared = session
        .call("prepare", &json!({"model":"example"}), 4096)
        .unwrap();
    assert_eq!(
        prepared,
        json!({"label":"example","input":{"model":"example"},
        "unavailable":["undefined","undefined","undefined","undefined","undefined"]})
    );
    assert_eq!(
        session.call("decode", &json!(1), 4096).unwrap(),
        json!({"count":1,"input":1})
    );
    assert_eq!(
        session.call("decode", &json!(2), 4096).unwrap(),
        json!({"count":2,"input":2})
    );
}

#[test]
fn refuses_sdk_and_external_modules() {
    for module in [
        "sailry/sdk",
        "sailry/models",
        "node:fs",
        "https://example.test/code.js",
    ] {
        let result = Session::new(
            bundle(&format!(
                "import {{x}} from '{module}'; export function prepare() {{return x;}}"
            )),
            CancellationToken::new(),
        );
        assert!(result.is_err());
    }
    let mut replacement = bundle("export function prepare() {return null}");
    replacement
        .files
        .insert("sailry/sdk".into(), "export const x=1".into());
    assert!(Session::new(replacement, CancellationToken::new()).is_err());
}

#[test]
fn does_not_expose_thrown_values() {
    let private = "private-authentication-exchange";
    let mut session = Session::new(
        bundle("export function exchange(input) {throw new Error(input.token)}"),
        CancellationToken::new(),
    )
    .unwrap();
    let error = session
        .call("exchange", &json!({"token":private}), 4096)
        .unwrap_err();
    assert!(!serde_json::to_string(&error).unwrap().contains(private));
    assert!(!format!("{error:?}").contains(private));
    assert!(session.call("exchange", &json!(null), 4096).is_err());
}

#[test]
fn interrupts_execution_and_retained_sessions() {
    let stop = CancellationToken::new();
    let mut session =
        Session::new(bundle("export function decode() {return 1}"), stop.clone()).unwrap();
    stop.cancel();
    assert_eq!(
        session.call("decode", &json!(null), 4096).unwrap_err().code,
        ErrorCode::Cancelled
    );
    assert!(
        Session::bounded(
            bundle("while(true) {}"),
            CancellationToken::new(),
            Duration::ZERO
        )
        .is_err()
    );
}

#[test]
fn preserves_values_within_caller_budget() {
    let mut session = Session::new(
        bundle("export function prepare(input) {return input}"),
        CancellationToken::new(),
    )
    .unwrap();
    let input = json!({"text":"x".repeat(8192)});
    assert_eq!(session.call("prepare", &input, 16384).unwrap(), input);
    assert!(session.call("prepare", &input, 4096).is_err());
    let mut session = Session::new(
        bundle("export function decode() {return 'x'.repeat(8192)}"),
        CancellationToken::new(),
    )
    .unwrap();
    assert!(session.call("decode", &json!(null), 4096).is_err());
}

#[test]
fn separates_model_callbacks_from_authentication() {
    let mut model = Session::model(
        bundle("export function decode(){while(true){}}"),
        CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(model.timeout, Duration::from_millis(50));
    let error = model.call("decode", &json!(null), 4096).unwrap_err();
    assert_eq!(error.code, ErrorCode::Internal);
    let mut auth = Session::new(
        bundle("export function exchange(input){return input}"),
        CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(auth.timeout, CALL_TIMEOUT);
    assert_eq!(
        auth.call("exchange", &json!({"access":"private"}), 4096)
            .unwrap(),
        json!({"access":"private"})
    );
    assert!(!auth.has_export("classify").unwrap());
    assert!(auth.has_export("exchange").unwrap());
}
