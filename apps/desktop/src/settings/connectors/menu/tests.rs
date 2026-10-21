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
fn configures_credentials_without_extra_authorization() {
    for method in [Kind::Secret, Kind::Application] {
        let (row, kind) = fixture(method, true);
        assert_eq!(
            items(&row, Some(&kind)),
            [
                Item::Action(Operation::Edit, "connector_configure"),
                Item::Action(Operation::Probe, "connector_probe"),
                Item::Separator,
                Item::Action(Operation::Disconnect, "connector_disconnect"),
                Item::Action(Operation::Remove, "settings_delete"),
            ]
        );
    }
}

#[test]
fn separates_configuration_from_authorization() {
    for method in [Kind::Oauth2, Kind::McpOauth, Kind::LocalCli] {
        let (row, kind) = fixture(method, true);
        assert_eq!(
            items(&row, Some(&kind)),
            [
                Item::Action(Operation::Edit, "connector_configure"),
                Item::Action(
                    if method == Kind::LocalCli {
                        Operation::Bind
                    } else {
                        Operation::Reconnect
                    },
                    if method == Kind::LocalCli {
                        "connector_bind"
                    } else {
                        "connector_authorize"
                    }
                ),
                Item::Action(Operation::Probe, "connector_probe"),
                Item::Separator,
                Item::Action(Operation::Disconnect, "connector_disconnect"),
                Item::Action(Operation::Remove, "settings_delete"),
            ]
        );
    }
    let (row, kind) = fixture(Kind::LocalCli, false);
    assert_eq!(
        items(&row, Some(&kind))[0],
        Item::Action(Operation::Bind, "connector_bind")
    );
    let (row, mut kind) = fixture(Kind::LocalCli, true);
    kind.declaration.fields[0].secret = true;
    assert!(
        !items(&row, Some(&kind)).contains(&Item::Action(Operation::Edit, "connector_configure"))
    );
}

#[test]
fn checks_only_backend_supported_states() {
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
            row.can_disconnect = matches!(
                state,
                State::Connected | State::Failed | State::Expired | State::NeedsConfirmation
            );
            let expected =
                enabled && matches!(state, State::Connected | State::Failed | State::Expired);
            assert_eq!(checkable(&row), expected, "{enabled}/{state:?}");
            let actions = items(&row, Some(&kind));
            assert_eq!(
                actions.contains(&Item::Action(Operation::Probe, "connector_probe")),
                expected
            );
            if state == State::Connecting {
                assert!(actions.is_empty());
            } else {
                assert!(actions.contains(&Item::Action(Operation::Edit, "connector_configure")));
                assert_eq!(
                    actions.contains(&Item::Action(Operation::Disconnect, "connector_disconnect")),
                    row.can_disconnect
                );
            }
        }
    }
}

#[test]
fn prioritizes_required_consent() {
    let (mut row, kind) = fixture(Kind::Oauth2, true);
    row.state = State::NeedsConfirmation;
    assert_eq!(
        items(&row, Some(&kind)),
        [
            Item::Action(Operation::Confirm, "connector_confirm"),
            Item::Action(Operation::Edit, "connector_configure"),
            Item::Separator,
            Item::Action(Operation::Disconnect, "connector_disconnect"),
            Item::Action(Operation::Remove, "settings_delete"),
        ]
    );
    row.can_disconnect = false;
    assert!(
        !items(&row, Some(&kind))
            .contains(&Item::Action(Operation::Disconnect, "connector_disconnect"))
    );
}

#[test]
fn hides_unavailable_recovery_actions() {
    let (mut row, _) = fixture(Kind::Secret, false);
    row.state = State::NotConnected;
    row.can_disconnect = false;
    assert_eq!(
        items(&row, None),
        [Item::Action(Operation::Remove, "settings_delete")]
    );
}

#[test]
fn disconnects_retained_credentials_without_a_connection() {
    let (mut row, kind) = fixture(Kind::Oauth2, true);
    row.state = State::NotConnected;
    assert!(
        items(&row, Some(&kind))
            .contains(&Item::Action(Operation::Disconnect, "connector_disconnect"))
    );
    row.can_disconnect = false;
    assert!(
        !items(&row, Some(&kind))
            .contains(&Item::Action(Operation::Disconnect, "connector_disconnect"))
    );
}
