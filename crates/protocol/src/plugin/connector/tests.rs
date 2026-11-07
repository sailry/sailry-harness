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
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "defaults_package_enablement",
            (|scenario: &'static str| {
                let mut value = json!({
                    "package": {"name":"example", "digest":"a".repeat(64), "settings_revision":0},
                    "declaration": declaration(),
                });
                let kind: Type = serde_json::from_value(value.clone()).unwrap();
                assert!(kind.enabled, "{scenario}");
                value["enabled"] = false.into();
                let disabled: Type = serde_json::from_value(value.clone()).unwrap();
                assert!(!disabled.enabled, "{scenario}");
                assert_eq!(
                    serde_json::from_value::<Type>(serde_json::to_value(&disabled).unwrap())
                        .unwrap(),
                    disabled,
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "confines_endpoints_and_headers",
            (|scenario: &'static str| {
                assert!(accepted(declaration()), "{scenario}");
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
                    assert!(!accepted(value), "{scenario}: accepted {origin}");
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
                    assert!(!accepted(value), "{scenario}: accepted {header}");
                }
                let mut value = declaration();
                value["inject"][0]["origin"] = json!("https://other.example.test");
                assert!(!accepted(value), "{scenario}");
            }),
        ),
        (
            "bounds_fields_and_accounts",
            (|scenario: &'static str| {
                let mut value = declaration();
                let field = value["fields"][0].clone();
                value["fields"].as_array_mut().unwrap().push(field);
                assert!(!accepted(value), "{scenario}");
                let mut value = declaration();
                value["accounts"] = json!("multiple");
                assert!(!accepted(value), "{scenario}");
                let mut value = declaration();
                value["fields"][0]["default"] = json!("private-marker");
                assert!(!accepted(value), "{scenario}");
                let mut value = declaration();
                value["fields"] = json!(
                    (0..9)
                        .map(|index| json!({"name":format!("field_{index}"),"title":"Field"}))
                        .collect::<Vec<_>>()
                );
                assert!(!accepted(value), "{scenario}");
            }),
        ),
        (
            "rejects_bundled_secrets",
            (|scenario: &'static str| {
                let mut value = declaration();
                value["auth"] = json!([{"kind":"oauth2","flow":"pkce","authorize":"https://api.example.test/authorize","token":"https://api.example.test/token","client":{"bundled":{"client_id":"public"}},"scopes":["read"],"refresh":true}]);
                assert!(accepted(value.clone()), "{scenario}");
                value["auth"][0]["client"]["bundled"]["client_secret"] = json!("private-marker");
                assert!(!accepted(value), "{scenario}");
                let mut value = declaration();
                value["auth"] = json!([{"kind":"oauth2","flow":"pkce","authorize":"https://other.example.test/authorize","token":"https://api.example.test/token","client":{"bundled":{"client_id":"public"}},"scopes":[]}]);
                assert!(!accepted(value), "{scenario}");
            }),
        ),
        (
            "confines_local_commands",
            (|scenario: &'static str| {
                let mut value = declaration();
                value["auth"] = json!([{"kind":"local_cli","command":"fixture-cli","env":{"PROMPT_DISABLED":"1"},"status":{"argv":["status","--json"],"account":"/account"},"token":{"argv":["token","--account","{account}"],"parse":"line"},"timeout_ms":1000,"max_output":4096}]);
                assert!(accepted(value.clone()), "{scenario}");
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
                    assert!(!accepted(changed), "{scenario}: accepted {pointer}");
                }
            }),
        ),
        (
            "defaults_missing_disconnect_capability",
            (|scenario: &'static str| {
                let connection: Connection = serde_json::from_value(json!({
                "id":RequestId::new(),"package":{"name":"fixture","digest":"a".repeat(64),"settings_revision":0},
                "connector":"account","method":"oauth2","revision":1,"enabled":true,"state":"failed",
                "identity":null,"expires_at_ms":null,"checked_at_ms":null,"fields":{},"configured":[]
            })).unwrap();
                assert!(!connection.can_disconnect, "{scenario}");
                assert_eq!(
                    serde_json::to_value(connection).unwrap()["can_disconnect"],
                    false,
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

mod fields {
    use super::*;

    type Select = fn(&Declaration, &Auth, &Field) -> bool;

    fn methods(auth: Value, fields: Value, inject: Value) -> Declaration {
        let mut value = declaration();
        value["auth"] = auth;
        value["fields"] = fields;
        value["inject"] = inject;
        let declaration: Declaration = serde_json::from_value(value).unwrap();
        assert!(declaration.valid());
        declaration
    }

    fn names(declaration: &Declaration, kind: Kind, select: Select) -> Vec<String> {
        let method = declaration
            .auth
            .iter()
            .find(|auth| auth.kind() == kind)
            .unwrap();
        declaration
            .fields
            .iter()
            .filter(|field| select(declaration, method, field))
            .map(|field| field.name.clone())
            .collect()
    }

    fn bundled() -> Value {
        json!({"kind":"oauth2","flow":"device_code","authorize":"https://api.example.test/authorize",
            "token":"https://api.example.test/token","device":"https://api.example.test/device",
            "client":{"bundled":{"client_id":"public"}},"scopes":["read"],"refresh":true})
    }

    fn local() -> Value {
        json!({"kind":"local_cli","command":"fixture-cli","status":{"argv":["status"],"account":"/account"},
            "token":{"argv":["token"],"parse":"line"},"timeout_ms":1000,"max_output":4096})
    }

    fn bearer() -> Value {
        json!({"origin":"https://api.example.test","header":"Authorization","prefix":"Bearer "})
    }

    #[test]
    fn skips_other_method_secrets() {
        for client in [json!(null), json!({"bundled":{"client_id":"public"}})] {
            let declaration = methods(
                json!([{"kind":"secret","fields":["token"]}, bundled(),
                    {"kind":"mcp_oauth","server":"fixture","client":client}, local()]),
                json!([{"name":"token","title":"Token","secret":true},
                    {"name":"tenant","title":"Tenant","optional":true,"default":"common"}]),
                json!([bearer()]),
            );
            assert_eq!(
                names(&declaration, Kind::Secret, Declaration::uses),
                ["token", "tenant"]
            );
            assert_eq!(
                names(&declaration, Kind::Secret, Declaration::requires),
                ["token"]
            );
            for kind in [Kind::Oauth2, Kind::McpOauth, Kind::LocalCli] {
                assert_eq!(
                    names(&declaration, kind, Declaration::uses),
                    ["tenant"],
                    "{kind:?}"
                );
                assert!(
                    names(&declaration, kind, Declaration::requires).is_empty(),
                    "{kind:?}"
                );
            }
        }
    }

    #[test]
    fn keeps_client_fields_with_their_method() {
        let declaration = methods(
            json!([{"kind":"secret","fields":["token"]},
                {"kind":"oauth2","flow":"pkce","authorize":"https://api.example.test/authorize",
                    "token":"https://api.example.test/token",
                    "client":{"fields":{"client_id":"client_id","client_secret":"client_secret"}}},
                {"kind":"mcp_oauth","server":"fixture","client":{"fields":{"client_id":"client_id"}}}]),
            json!([{"name":"token","title":"Token","secret":true},
                {"name":"client_id","title":"Client ID"},
                {"name":"client_secret","title":"Client secret","secret":true,"optional":true}]),
            json!([bearer()]),
        );
        assert_eq!(
            names(&declaration, Kind::Secret, Declaration::uses),
            ["token"]
        );
        assert_eq!(
            names(&declaration, Kind::Oauth2, Declaration::uses),
            ["client_id", "client_secret"]
        );
        assert_eq!(
            names(&declaration, Kind::Oauth2, Declaration::requires),
            ["client_id"]
        );
        assert_eq!(
            names(&declaration, Kind::McpOauth, Declaration::uses),
            ["client_id"]
        );
    }

    #[test]
    fn shares_injected_secrets_except_local_cli() {
        let declaration = methods(
            json!([{"kind":"secret","fields":["token"]}, bundled(), local()]),
            json!([{"name":"token","title":"Token","secret":true},
                {"name":"key","title":"Key","secret":true}]),
            json!([bearer(), {"origin":"https://api.example.test","header":"X-Key","field":"key"}]),
        );
        assert!(declaration.injects("key"));
        assert!(!declaration.injects("token"));
        assert_eq!(
            names(&declaration, Kind::Secret, Declaration::requires),
            ["token", "key"]
        );
        assert_eq!(
            names(&declaration, Kind::Oauth2, Declaration::requires),
            ["key"]
        );
        assert!(names(&declaration, Kind::LocalCli, Declaration::uses).is_empty());
    }
}
