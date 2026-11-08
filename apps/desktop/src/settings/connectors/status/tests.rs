use super::{Status, aggregate, server, service};
use crate::settings::connectors::catalog::{Key, Member, Service};
use sailry_protocol::plugin::{
    Mcp, McpTransport, Reference,
    mcp::{Availability, Runtime, RuntimeState},
};

fn fixture(state: Option<RuntimeState>, tools: u32) -> Mcp {
    Mcp {
        name: "fixture".into(),
        host: None,
        transport: McpTransport::Stdio,
        allowed: true,
        availability: Availability::Available,
        runtime: state.map(|state| Runtime {
            state,
            tools,
            fault: None,
            updated_at_ms: 1,
        }),
    }
}

#[test]
fn disabled_and_static_precedence() {
    let mut current = fixture(Some(RuntimeState::Failed), 0);
    assert_eq!(server(true, &current), Some(Status::Failed));
    for (availability, expected) in [
        (
            Availability::ConfigurationRequired,
            "connector_configuration_required",
        ),
        (Availability::ConnectionRequired, "connector_not_connected"),
        (Availability::AuthorizationExpired, "connector_expired"),
        (Availability::ConsentRequired, "connector_confirmation"),
        (Availability::Unavailable, "connector_unavailable"),
    ] {
        current.availability = availability;
        assert_eq!(server(true, &current), Some(Status::Problem(expected)));
        assert_eq!(server(false, &current), None);
        current.allowed = false;
        assert_eq!(server(true, &current), None);
        current.allowed = true;
    }
    for availability in [
        Availability::PackageDisabled,
        Availability::ConnectionDisabled,
        Availability::ServerDisabled,
    ] {
        current.availability = availability;
        assert_eq!(server(true, &current), None);
    }
}

#[test]
fn worst_state_and_tool_total() {
    let mut servers = vec![
        fixture(Some(RuntimeState::Connected), 7),
        fixture(Some(RuntimeState::Connected), 11),
        fixture(None, 0),
    ];
    assert_eq!(aggregate(&servers), Some(Status::Connected(18)));
    servers.push(fixture(Some(RuntimeState::Connecting), 0));
    assert_eq!(aggregate(&servers), Some(Status::Connecting));
    servers.push(fixture(Some(RuntimeState::Failed), 0));
    assert_eq!(aggregate(&servers), Some(Status::Failed));
    servers.reverse();
    assert_eq!(aggregate(&servers), Some(Status::Failed));
    servers[0].allowed = false;
    assert_eq!(aggregate(&servers), Some(Status::Connecting));
    assert_eq!(
        aggregate(&[fixture(Some(RuntimeState::Connected), 0)]),
        Some(Status::Connected(0))
    );
    assert_eq!(aggregate(&[fixture(None, 0)]), None);
    assert_eq!(aggregate(&[]), None);
}

#[test]
fn account_state_precedes_runtime() {
    let package = Reference {
        name: "fixture".into(),
        digest: "digest".into(),
        settings_revision: 1,
    };
    let account = serde_json::from_value(serde_json::json!({
        "id": sailry_protocol::RequestId::new(),
        "package": package,
        "connector": "account",
        "method": "secret",
        "revision": 1,
        "enabled": true,
        "state": "expired",
        "can_disconnect": true,
        "identity": null,
        "expires_at_ms": null,
        "checked_at_ms": null,
        "fields": {},
        "configured": ["token"]
    }))
    .unwrap();
    let mut current = Service {
        key: Key {
            package: package.name.clone(),
            member: Member::Connector("account".into()),
        },
        package,
        kind: None,
        account: Some(account),
        info: None,
        servers: vec![fixture(Some(RuntimeState::Connected), 5)],
    };
    assert_eq!(
        service(&current),
        Some(Status::Problem("connector_expired"))
    );
    current.account.as_mut().unwrap().enabled = false;
    assert_eq!(service(&current), None);
    current.account.as_mut().unwrap().enabled = true;
    current.account.as_mut().unwrap().state = sailry_protocol::plugin::connector::State::Connecting;
    assert_eq!(service(&current), Some(Status::Connecting));
    current.servers[0].allowed = false;
    assert_eq!(service(&current), None);
}
