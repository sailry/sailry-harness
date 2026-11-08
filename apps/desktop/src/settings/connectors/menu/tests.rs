use super::{Item, Operation, checkable, items};
use sailry_protocol::{
    RequestId,
    plugin::connector::{Connection, Kind, State, Type},
};
use serde_json::json;

fn fixture(method: Kind, fields: bool) -> (Connection, Type) {
    let auth = match method {
        Kind::Oauth2 => {
            json!({"kind":"oauth2","flow":"pkce","authorize":"https://example.com/auth","token":"https://example.com/token","client":{"bundled":{"client_id":"fixture"}}})
        }
        Kind::McpOauth => json!({"kind":"mcp_oauth","server":"fixture"}),
        Kind::LocalCli => {
            json!({"kind":"local_cli","command":"fixture","status":{"argv":["status"],"account":"/name"},"token":{"argv":["token"],"parse":"line"},"timeout_ms":1000,"max_output":1024})
        }
        Kind::Secret => json!({"kind":"secret","fields":["token"]}),
        Kind::Application => {
            json!({"kind":"application","fields":{"client_id":"token"},"token":{"url":"https://example.com/token","body":{},"access":"/access","expires_in":"/expires"}})
        }
    };
    let package = json!({"name":"fixture","digest":"a".repeat(64),"settings_revision":0});
    let row = serde_json::from_value(json!({
        "id":RequestId::new(),"package":package,"connector":"account","method":method,
        "revision":7,"enabled":true,"state":"connected","can_disconnect":true,
        "identity":null,"expires_at_ms":null,"checked_at_ms":null,"fields":{},"configured":[]
    }))
    .unwrap();
    let kind = serde_json::from_value(json!({"package":package,"declaration":{
        "id":"account","display":{"label":"Account"},"accounts":"single",
        "origins":["https://example.com"],"auth":[auth],"inject":[],
        "fields":if fields {json!([{"name":"token","title":"Token"}])} else {json!([])}
    }}))
    .unwrap();
    (row, kind)
}

#[test]
fn available_actions() {
    for (scenario, check) in [
        (
            "labels_actions_by_authentication_method",
            (|_scenario: &str| {
                for method in [
                    Kind::Secret,
                    Kind::Application,
                    Kind::Oauth2,
                    Kind::McpOauth,
                    Kind::LocalCli,
                ] {
                    let (row, kind) = fixture(method, false);
                    let edit = match method {
                        Kind::Oauth2 | Kind::McpOauth => "connector_reauthorize",
                        Kind::LocalCli => "connector_bind",
                        _ => "connector_configure",
                    };
                    assert_eq!(
                        items(&row, Some(&kind)),
                        [
                            Item::Action(Operation::Edit, edit),
                            Item::Action(Operation::Probe, "connector_probe"),
                            Item::Action(Operation::Remove, "connector_remove"),
                        ],
                        "labels actions by authentication method"
                    );
                }
            }) as fn(&str),
        ),
        (
            "checks_only_backend_supported_states",
            (|_scenario: &str| {
                let (mut row, kind) = fixture(Kind::Oauth2, true);
                for enabled in [false, true] {
                    row.enabled = enabled;
                    for state in [
                        State::Connected,
                        State::Failed,
                        State::Expired,
                        State::NotConnected,
                        State::Connecting,
                        State::NeedsConfirmation,
                    ] {
                        row.state = state;
                        let expected = enabled
                            && matches!(state, State::Connected | State::Failed | State::Expired);
                        assert_eq!(
                            checkable(&row),
                            expected,
                            "checks only backend supported states: {enabled}/{state:?}"
                        );
                        let actions = items(&row, Some(&kind));
                        assert_eq!(
                            actions.contains(&Item::Action(Operation::Probe, "connector_probe")),
                            expected,
                            "checks only backend supported states"
                        );
                        let edit = match state {
                            State::NeedsConfirmation => "connector_confirm",
                            State::NotConnected => "connector_connect",
                            State::Connected => "connector_configure",
                            _ => "connector_reauthorize",
                        };
                        assert!(actions.contains(&Item::Action(Operation::Edit, edit)));
                        assert!(
                            actions.contains(&Item::Action(Operation::Remove, "connector_remove")),
                            "checks only backend supported states"
                        );
                    }
                }
            }),
        ),
        (
            "keeps_disabled_configuration_accessible",
            (|_scenario: &str| {
                let (row, mut kind) = fixture(Kind::McpOauth, false);
                kind.enabled = false;
                assert_eq!(
                    items(&row, Some(&kind)),
                    [
                        Item::Action(Operation::Edit, "connector_reauthorize"),
                        Item::Action(Operation::Remove, "connector_remove"),
                    ],
                    "keeps disabled configuration accessible"
                );
            }),
        ),
        (
            "removes_orphans_without_disconnecting_other_accounts",
            (|_scenario: &str| {
                let (mut row, _) = fixture(Kind::Secret, false);
                row.state = State::NotConnected;
                row.can_disconnect = true;
                assert_eq!(
                    items(&row, None),
                    [Item::Action(Operation::Remove, "connector_remove")],
                    "removes orphans without disconnecting other accounts"
                );
                row.can_disconnect = false;
                assert_eq!(
                    items(&row, None),
                    [Item::Action(Operation::Remove, "connector_remove")],
                    "removes orphans without disconnecting other accounts"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
