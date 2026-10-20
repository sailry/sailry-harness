use super::*;
use serde_json::{Value, json};

fn declaration() -> Value {
    json!({"id":"fixture","display":{"label":"Fixture"},"accounts":"single",
        "origins":["https://api.example.test"],"auth":[{"kind":"secret","fields":["token"]}],
        "fields":[{"name":"token","title":"Token","secret":true}],
        "inject":[{"origin":"https://api.example.test","header":"Authorization","prefix":"Bearer "}]})
}

fn accepted(value: Value) -> bool {
    serde_json::from_value::<Declaration>(value).is_ok_and(|declaration| declaration.valid())
}

#[test]
fn confines_endpoints_and_headers() {
    assert!(accepted(declaration()));
    for origin in [
        "http://api.example.test",
        "https://127.0.0.1",
        "https://[::1]",
        "https://localhost",
        "https://service.local",
        "https://api.example.test/path",
        "https://user@api.example.test",
    ] {
        let mut value = declaration();
        value["origins"] = json!([origin]);
        value["inject"][0]["origin"] = json!(origin);
        assert!(!accepted(value), "accepted {origin}");
    }
    for header in [
        "Host",
        "Proxy-Authorization",
        "Content-Length",
        "Cookie",
        "invalid header",
    ] {
        let mut value = declaration();
        value["inject"][0]["header"] = json!(header);
        assert!(!accepted(value), "accepted {header}");
    }
    let mut value = declaration();
    value["inject"][0]["origin"] = json!("https://other.example.test");
    assert!(!accepted(value));
}

#[test]
fn bounds_fields_and_accounts() {
    let mut value = declaration();
    let field = value["fields"][0].clone();
    value["fields"].as_array_mut().unwrap().push(field);
    assert!(!accepted(value));
    let mut value = declaration();
    value["accounts"] = json!("multiple");
    assert!(!accepted(value));
    let mut value = declaration();
    value["fields"][0]["default"] = json!("private-marker");
    assert!(!accepted(value));
    let mut value = declaration();
    value["fields"] = json!(
        (0..9)
            .map(|index| json!({"name":format!("field_{index}"),"title":"Field"}))
            .collect::<Vec<_>>()
    );
    assert!(!accepted(value));
}

#[test]
fn rejects_bundled_secrets() {
    let mut value = declaration();
    value["auth"] = json!([{"kind":"oauth2","flow":"pkce","authorize":"https://api.example.test/authorize","token":"https://api.example.test/token","client":{"bundled":{"client_id":"public"}},"scopes":["read"],"refresh":true}]);
    assert!(accepted(value.clone()));
    value["auth"][0]["client"]["bundled"]["client_secret"] = json!("private-marker");
    assert!(!accepted(value));
    let mut value = declaration();
    value["auth"] = json!([{"kind":"oauth2","flow":"pkce","authorize":"https://other.example.test/authorize","token":"https://api.example.test/token","client":{"bundled":{"client_id":"public"}},"scopes":[]}]);
    assert!(!accepted(value));
}

#[test]
fn confines_local_commands() {
    let mut value = declaration();
    value["auth"] = json!([{"kind":"local_cli","command":"fixture-cli","env":{"PROMPT_DISABLED":"1"},"status":{"argv":["status","--json"],"account":"/account"},"token":{"argv":["token","--account","{account}"],"parse":"line"},"timeout_ms":1000,"max_output":4096}]);
    assert!(accepted(value.clone()));
    for (pointer, invalid) in [
        ("/auth/0/command", json!("/bin/sh")),
        ("/auth/0/status/argv/0", json!("{account}")),
        ("/auth/0/token/argv/0", json!("{user_input}")),
        ("/auth/0/timeout_ms", json!(30001)),
        ("/auth/0/max_output", json!(65537)),
        ("/auth/0/env/PATH", json!("/untrusted")),
    ] {
        let mut changed = value.clone();
        if pointer == "/auth/0/env/PATH" {
            changed["auth"][0]["env"]["PATH"] = invalid;
        } else {
            *changed.pointer_mut(pointer).unwrap() = invalid;
        }
        assert!(!accepted(changed), "accepted {pointer}");
    }
}
