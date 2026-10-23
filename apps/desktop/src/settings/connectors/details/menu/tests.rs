use super::*;
use core::prelude::v1::test;
use sailry_protocol::plugin::{Summary, mcp::Availability};

fn service() -> catalog::Service {
    let summary = Summary {
        name: "fixture".into(),
        revision: 1,
        digest: "digest".into(),
        settings_revision: 2,
        enabled: true,
        version: None,
        description: None,
    };
    catalog::Service {
        key: catalog::Key {
            package: summary.name.clone(),
            member: catalog::Member::Server("server".into()),
        },
        package: summary.reference(),
        kind: None,
        account: None,
        info: Some(
            serde_json::from_value(serde_json::json!({
                "summary":summary,"extension":null,"settings":null,"icon":null
            }))
            .unwrap(),
        ),
        servers: vec![Mcp {
            name: "server".into(),
            host: None,
            transport: sailry_protocol::plugin::McpTransport::Stdio,
            allowed: true,
            availability: Availability::Available,
        }],
    }
}

#[test]
fn preserves_declared_services_and_exposes_independent_actions() {
    let service = service();
    assert_eq!(
        switch_target(&service),
        Some(Toggle::Server("server".into()))
    );
    let actions = items(&service);
    assert_eq!(
        actions,
        [
            Item::Service(Operation::Configure, "connector_configure", true),
            Item::Separator,
            Item::Service(Operation::Check, "connector_probe", true),
        ]
    );
}

#[test]
fn account_switch_never_changes_server_enablement() {
    let mut service = service();
    service.key.member = catalog::Member::Account("account".into());
    service.account = Some(
        serde_json::from_value(serde_json::json!({
            "id":sailry_protocol::RequestId::new(), "package":service.package,
            "connector":"account", "method":"secret", "revision":1,
            "enabled":true,"state":"connected","can_disconnect":true,
            "identity":null,"expires_at_ms":null,"checked_at_ms":null,
            "fields":{},"configured":[]
        }))
        .unwrap(),
    );
    assert_eq!(
        switch_target(&service),
        Some(Toggle::Account(service.account.as_ref().unwrap().id))
    );
    assert_eq!(
        items(&service),
        [
            Item::Service(Operation::Configure, "connector_configure", true),
            Item::Separator,
            Item::Service(Operation::Check, "connector_probe", true),
            Item::Service(Operation::Authorize, "connector_authorize", true),
        ]
    );
}

#[test]
fn preserves_manual_server_target_after_login() {
    let mut service = service();
    service.info.as_mut().unwrap().mcp_source = Some(
        serde_json::from_value(serde_json::json!({
            "type":"streamable-http","url":"https://example.com/mcp","headers":[]
        }))
        .unwrap(),
    );
    service.key.member = catalog::Member::Account("oauth".into());
    service.kind = Some(
        serde_json::from_value(serde_json::json!({
            "package":service.package,"enabled":true,
            "declaration":{
                "id":"oauth","display":{"label":"OAuth"},"accounts":"single",
                "origins":["https://example.com"],"auth":[{"kind":"mcp_oauth","server":"server"}],
                "inject":[]
            }
        }))
        .unwrap(),
    );
    let expected = Some(Toggle::Server("server".into()));
    assert_eq!(switch_target(&service), expected);
    service.account = Some(
        serde_json::from_value(serde_json::json!({
            "id":sailry_protocol::RequestId::new(),"package":service.package,
            "connector":"oauth","method":"mcp_oauth","revision":1,
            "enabled":false,"state":"connected","can_disconnect":true,
            "identity":null,"expires_at_ms":null,"checked_at_ms":null,"fields":{},"configured":[]
        }))
        .unwrap(),
    );
    assert_eq!(switch_target(&service), expected);
    assert!(service.servers[0].allowed);
    assert!(!service.account.as_ref().unwrap().enabled);
    service.servers.push(Mcp {
        name: "unbound".into(),
        ..service.servers[0].clone()
    });
    assert_eq!(switch_target(&service), expected);
    service.kind.as_mut().unwrap().declaration.auth.clear();
    assert!(switch_target(&service).is_none());
}

#[test]
fn restores_only_disabled_manual_packages() {
    let mut service = service();
    service.info.as_mut().unwrap().summary.enabled = false;
    assert!(!restorable(&service));
    service.info.as_mut().unwrap().mcp_source = Some(
        serde_json::from_value(serde_json::json!({
            "type":"stdio","command":"fixture","args":[],"env":[]
        }))
        .unwrap(),
    );
    service.servers[0].allowed = false;
    assert!(restorable(&service));
    assert_eq!(
        items(&service),
        [
            Item::Service(Operation::Configure, "connector_configure", true),
            Item::Separator,
            Item::Service(Operation::Check, "connector_probe", false),
            Item::Separator,
            Item::Service(Operation::Remove, "settings_delete", true),
        ]
    );
    assert_eq!(
        switch_target(&service),
        Some(Toggle::Server("server".into()))
    );
    assert!(!service.servers[0].allowed);
    service.info.as_mut().unwrap().summary.enabled = true;
    assert!(!restorable(&service));
    service.info = None;
    assert!(switch_target(&service).is_none());
    assert!(!restorable(&service));
}

#[test]
fn groups_authentication_without_mcp_configuration() {
    let mut service = service();
    service.servers.clear();
    service.kind = Some(
        serde_json::from_value(serde_json::json!({
            "package":service.package,"enabled":true,
            "declaration":{
                "id":"account","display":{"label":"Account"},"accounts":"single",
                "origins":["https://example.com"],"auth":[{"kind":"secret","fields":["token"]}],
                "fields":[{"name":"token","title":"Token","secret":true}],"inject":[]
            }
        }))
        .unwrap(),
    );
    assert_eq!(
        items(&service),
        [Item::Service(
            Operation::Authorize,
            "connector_authorize",
            true,
        )]
    );
    service.account = Some(
        serde_json::from_value(serde_json::json!({
            "id":sailry_protocol::RequestId::new(), "package":service.package,
            "connector":"account", "method":"secret", "revision":1,
            "enabled":true,"state":"connected","can_disconnect":true,
            "identity":null,"expires_at_ms":null,"checked_at_ms":null,
            "fields":{},"configured":["token"]
        }))
        .unwrap(),
    );
    assert_eq!(
        items(&service),
        [
            Item::Account(
                super::super::super::menu::Operation::Edit,
                "connector_configure"
            ),
            Item::Separator,
            Item::Account(
                super::super::super::menu::Operation::Probe,
                "connector_probe"
            ),
            Item::Service(Operation::Authorize, "connector_authorize", true),
        ]
    );
    service.kind = None;
    assert_eq!(
        items(&service),
        [Item::Service(
            Operation::Authorize,
            "connector_authorize",
            true,
        )]
    );
    service.account = None;
    assert!(items(&service).is_empty());
}

#[test]
fn offers_one_check_for_multiple_servers() {
    let mut service = service();
    service.servers[0].availability = Availability::ConnectionRequired;
    service.servers.push(Mcp {
        name: "sibling".into(),
        availability: Availability::ServerDisabled,
        ..service.servers[0].clone()
    });
    let expected = [
        Item::Service(Operation::Configure, "connector_configure", true),
        Item::Separator,
        Item::Service(Operation::Check, "connector_probe", true),
    ];
    assert_eq!(items(&service), expected);
    service.servers[1].availability = Availability::ConnectionRequired;
    assert_eq!(
        items(&service).last(),
        Some(&Item::Service(Operation::Check, "connector_probe", false))
    );
}
