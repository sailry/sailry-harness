use super::*;
use core::prelude::v1::test;
use sailry_protocol::plugin::{Summary, mcp::Availability};

fn service() -> catalog::Service {
    let summary = Summary {
        compatibility: Default::default(),
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
            member: catalog::Member::Mcp,
        },
        package: summary.reference(),
        kind: None,
        account: None,
        info: Some(
            serde_json::from_value(serde_json::json!({
                "summary":summary,"extension":null,"settings":null,"icon":null,
                "mcp_source":{"type":"stdio","command":"fixture","args":[],"env":[]}
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
fn exposes_one_instance_menu() {
    let service = service();
    assert_eq!(
        switch_target(&service),
        Some(Toggle::Server("server".into()))
    );
    assert_eq!(
        items(&service),
        [
            Item::Service(Operation::Configure, "connector_configure", true),
            Item::Service(Operation::Check, "connector_probe", true),
            Item::Service(Operation::Remove, "connector_remove", true),
        ]
    );
}

#[test]
fn account_management_uses_the_actual_id() {
    let mut service = service();
    service.info.as_mut().unwrap().mcp_source = None;
    let account: Connection = serde_json::from_value(serde_json::json!({
        "id":sailry_protocol::RequestId::new(), "package":service.package,
        "connector":"account", "method":"secret", "revision":1,
        "enabled":true,"state":"connected","can_disconnect":true,
        "identity":null,"expires_at_ms":null,"checked_at_ms":null,"fields":{},"configured":["token"]
    }))
    .unwrap();
    service.key.member = catalog::Member::Account(account.id);
    service.account = Some(account.clone());
    service.kind = Some(
        serde_json::from_value(serde_json::json!({
            "package":service.package,"enabled":true,
            "declaration":{"id":"account","display":{"label":"Account"},"accounts":"single",
                "origins":["https://example.com"],"auth":[{"kind":"secret","fields":["token"]}],
                "fields":[{"name":"token","title":"Token","secret":true}],"inject":[]}
        }))
        .unwrap(),
    );
    assert_eq!(switch_target(&service), Some(Toggle::Account(account.id)));
    use super::super::super::menu::Operation as Account;
    assert_eq!(
        items(&service),
        [
            Item::Account(Account::Edit, "connector_configure", true),
            Item::Account(Account::Probe, "connector_probe", true),
            Item::Account(Account::Remove, "connector_remove", true),
        ]
    );
    service.kind = None;
    assert_eq!(
        items(&service),
        [
            Item::Account(Account::Edit, "connector_configure", false),
            Item::Account(Account::Probe, "connector_probe", false),
            Item::Account(Account::Remove, "connector_remove", true),
        ]
    );
}

#[test]
fn disabled_packages_never_offer_a_restore_action() {
    let mut service = service();
    service.info.as_mut().unwrap().summary.enabled = false;
    assert_eq!(
        items(&service),
        [
            Item::Service(Operation::Configure, "connector_configure", true),
            Item::Service(Operation::Check, "connector_probe", false),
            Item::Service(Operation::Remove, "connector_remove", true),
        ]
    );
    assert_eq!(
        switch_target(&service),
        Some(Toggle::Server("server".into()))
    );
}

#[test]
fn missing_authorization_keeps_configuration_reachable() {
    let mut service = service();
    service.servers[0].availability = Availability::ConnectionRequired;
    assert_eq!(
        items(&service),
        [
            Item::Service(Operation::Configure, "connector_configure", true),
            Item::Service(Operation::Check, "connector_probe", false),
            Item::Service(Operation::Remove, "connector_remove", true),
        ]
    );
}
