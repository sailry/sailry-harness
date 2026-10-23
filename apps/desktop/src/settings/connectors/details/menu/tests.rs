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
    assert!(actions.contains(&Item::Service(
        Operation::Configure,
        "connector_configure",
        true
    )));
    assert!(actions.contains(&Item::Service(Operation::Check, "connector_probe", true)));
    assert!(actions.contains(&Item::Service(
        Operation::Plugin,
        "connector_view_plugin",
        true
    )));
    assert!(
        !actions
            .iter()
            .any(|item| matches!(item, Item::Service(Operation::Remove, _, _)))
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
    assert!(items(&service).iter().any(|item| matches!(
        item,
        Item::Account(crate::settings::connectors::menu::Operation::Disconnect, _)
    )));
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
    assert!(
        !items(&service)
            .iter()
            .any(|entry| matches!(entry, Item::Service(Operation::Enable, _, _)))
    );
    service.info.as_mut().unwrap().mcp_source = Some(
        serde_json::from_value(serde_json::json!({
            "type":"stdio","command":"fixture","args":[],"env":[]
        }))
        .unwrap(),
    );
    service.servers[0].allowed = false;
    assert!(items(&service).contains(&Item::Service(Operation::Enable, "connector_enable", true)));
    assert_eq!(
        switch_target(&service),
        Some(Toggle::Server("server".into()))
    );
    assert!(!service.servers[0].allowed);
    service.info.as_mut().unwrap().summary.enabled = true;
    assert!(
        !items(&service)
            .iter()
            .any(|entry| matches!(entry, Item::Service(Operation::Enable, _, _)))
    );
    service.info = None;
    assert!(switch_target(&service).is_none());
}
