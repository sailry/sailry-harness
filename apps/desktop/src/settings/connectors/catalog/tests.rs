use super::*;
use core::prelude::v1::test;
use sailry_protocol::{
    RequestId,
    plugin::{McpTransport, Summary, mcp::Availability},
};
use serde_json::json;

fn info(name: &str, servers: &[&str]) -> Info {
    Info {
        versions: Vec::new(),
        summary: Summary {
            compatibility: Default::default(),
            name: name.into(),
            revision: 1,
            digest: format!("{name}-digest"),
            settings_revision: 3,
            enabled: true,
            version: Some("0.1.0".into()),
            description: None,
        },
        release_notes: None,
        origin: None,
        icon: None,
        skill: None,
        mcp_source: None,
        extension: None,
        skills: Vec::new(),
        mcp: servers
            .iter()
            .map(|server| Mcp {
                host: None,
                name: (*server).into(),
                transport: McpTransport::StreamableHttp,
                allowed: true,
                availability: Availability::Available,
            })
            .collect(),
        settings: None,
        issues: Vec::new(),
    }
}

fn kind(info: &Info, id: &str, servers: &[&str]) -> Type {
    serde_json::from_value(json!({
        "package":info.summary.reference(),"enabled":info.summary.enabled,
        "declaration":{
            "id":id,"display":{"label":id},"accounts":"single",
            "origins":["https://example.com"],"auth":[{"kind":"secret","fields":["token"]}],
            "fields":[{"name":"token","title":"Token","secret":true}],
            "inject":servers.iter().map(|server| json!({"server":server,"header":"Authorization"})).collect::<Vec<_>>()
        }
    })).unwrap()
}

fn account(kind: &Type) -> Connection {
    serde_json::from_value(json!({
        "id":RequestId::new(),"package":kind.package,"connector":kind.declaration.id,
        "method":"secret","revision":7,"enabled":true,"state":"connected",
        "can_disconnect":true,"identity":{"name":"Account","id":"fixture"},
        "expires_at_ms":null,"checked_at_ms":null,"fields":{},"configured":["token"]
    }))
    .unwrap()
}

fn directory(types: &[Type], accounts: &[Connection], infos: &[Info]) -> Vec<Service> {
    collect(
        types,
        accounts,
        &infos
            .iter()
            .map(|info| (info.summary.name.clone(), info.clone()))
            .collect(),
    )
}

#[test]
fn tracks_account_changes_without_package_revisions() {
    let info = info("github", &["server"]);
    let kind = kind(&info, "account", &["server"]);
    let connected = account(&kind);
    let mut disconnected = connected.clone();
    disconnected.state = connector::State::NotConnected;
    disconnected.configured.clear();
    let package = BTreeSet::from(["github".into()]);
    assert_eq!(
        changed_accounts(&[disconnected.clone()], std::slice::from_ref(&connected)),
        package
    );
    assert_eq!(
        changed_accounts(std::slice::from_ref(&connected), &[disconnected]),
        package
    );
    for changed in [
        Connection {
            revision: connected.revision + 1,
            ..connected.clone()
        },
        Connection {
            enabled: false,
            ..connected.clone()
        },
        Connection {
            state: connector::State::Expired,
            ..connected.clone()
        },
    ] {
        assert_eq!(
            changed_accounts(std::slice::from_ref(&connected), &[changed]),
            package
        );
    }
}

#[test]
fn scopes_snapshots_to_changed_accounts() {
    let github = info("github", &["server"]);
    let figma = info("figma", &[]);
    let accounts = [
        account(&kind(&github, "account", &["server"])),
        account(&kind(&figma, "account", &[])),
    ];
    assert!(changed_accounts(&accounts, &[accounts[1].clone(), accounts[0].clone()]).is_empty());
    assert_eq!(
        changed_accounts(&[], &accounts),
        BTreeSet::from(["figma".into(), "github".into()])
    );
    assert_eq!(
        changed_accounts(&accounts, &[accounts[1].clone()]),
        BTreeSet::from(["github".into()])
    );
    assert_eq!(
        changed_accounts(&[accounts[1].clone()], &accounts),
        BTreeSet::from(["github".into()])
    );
}

fn manual(name: &str) -> Info {
    let mut info = info(name, &["server"]);
    info.mcp_source = Some(
        serde_json::from_value(json!({
            "type":"streamable-http","url":"https://example.com/mcp","headers":[]
        }))
        .unwrap(),
    );
    info
}

#[test]
fn excludes_unadded_and_plugin_internal_services() {
    let infos = [info("api", &[]), info("ordinary-mcp", &["server"])];
    let types = infos
        .iter()
        .map(|info| kind(info, "account", &[]))
        .collect::<Vec<_>>();
    assert!(directory(&types, &[], &infos).is_empty());
    assert!(directory(&[], &[], &[info("computer-capability", &["server"])]).is_empty());
}

#[test]
fn retains_real_ids_with_shared_labels() {
    let info = info("service", &[]);
    let kind = kind(&info, "account", &[]);
    let first = account(&kind);
    let mut second = first.clone();
    second.id = RequestId::new();
    let services = directory(&[kind], &[first.clone(), second.clone()], &[info]);
    assert_eq!(services.len(), 2);
    assert_ne!(services[0].key, services[1].key);
    assert!(
        services
            .iter()
            .any(|service| service.key.member == Member::Account(first.id))
    );
    assert!(
        services
            .iter()
            .any(|service| service.key.member == Member::Account(second.id))
    );
}

#[test]
fn keeps_bound_capabilities_inside_the_account() {
    let info = info("service", &["read", "write"]);
    let kind = kind(&info, "account", &["read", "write"]);
    let account = account(&kind);
    let services = directory(&[kind], std::slice::from_ref(&account), &[info]);
    assert_eq!(services.len(), 1);
    assert_eq!(services[0].account.as_ref().unwrap(), &account);
    assert_eq!(services[0].servers.len(), 2);
    assert!(!services[0].manual());
}

#[test]
fn keeps_anonymous_instances_accountless() {
    let services = directory(&[], &[], &[manual("mcp-anonymous")]);
    assert_eq!(services.len(), 1);
    assert_eq!(services[0].key.member, Member::Mcp);
    assert!(services[0].account.is_none());
    assert!(services[0].kind.is_none());
    assert!(services[0].allowed(&services[0].servers[0]));
    assert!(super::super::status(&services[0]).is_none());
}

#[test]
fn merges_authorization_into_the_original_instance() {
    let info = manual("mcp-custom");
    let mut kind = kind(&info, "server", &["server"]);
    kind.declaration.auth = vec![connector::Auth::McpOauth {
        server: "server".into(),
        client: None,
    }];
    let before = directory(&[kind.clone()], &[], std::slice::from_ref(&info));
    let mut account = account(&kind);
    account.method = connector::Kind::McpOauth;
    let after = directory(&[kind], &[account.clone()], &[info]);
    assert_eq!(before.len(), 1);
    assert_eq!(after.len(), 1);
    assert_eq!(before[0].key, after[0].key);
    assert_eq!(after[0].account.as_ref().unwrap().id, account.id);
    assert_eq!(after[0].label(), "custom");
}

#[test]
fn never_merges_instances_by_url_or_display() {
    let first = manual("mcp-first");
    let second = manual("mcp-second");
    assert_eq!(first.mcp_source, second.mcp_source);
    let services = directory(&[], &[], &[first, second]);
    assert_eq!(services.len(), 2);
    assert_ne!(services[0].key, services[1].key);
    assert_ne!(services[0].package.name, services[1].package.name);
}

#[test]
fn preserves_disabled_and_unavailable_accounts() {
    let info = info("service", &[]);
    let kind = kind(&info, "account", &[]);
    for state in [
        connector::State::Connected,
        connector::State::NotConnected,
        connector::State::Expired,
        connector::State::Connecting,
        connector::State::NeedsConfirmation,
    ] {
        let mut account = account(&kind);
        account.enabled = false;
        account.state = state;
        let services = directory(
            std::slice::from_ref(&kind),
            &[account.clone()],
            std::slice::from_ref(&info),
        );
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].account.as_ref().unwrap(), &account);
    }
}

#[test]
fn retains_removed_declarations_by_account_id() {
    let info = info("service", &[]);
    let account = account(&kind(&info, "removed", &[]));
    let services = directory(&[], std::slice::from_ref(&account), &[info]);
    assert_eq!(services.len(), 1);
    assert!(services[0].kind.is_none());
    assert_eq!(services[0].key.member, Member::Account(account.id));
    assert_eq!(services[0].account.as_ref().unwrap().id, account.id);
}

#[test]
fn never_joins_mixed_configuration_revisions() {
    let mut info = info("service", &["server"]);
    let kind = kind(&info, "account", &["server"]);
    let account = account(&kind);
    info.summary.settings_revision += 1;
    let services = directory(&[kind], std::slice::from_ref(&account), &[info]);
    assert_eq!(services.len(), 1);
    assert_eq!(services[0].account.as_ref().unwrap().id, account.id);
    assert!(services[0].info.is_none());
    assert!(services[0].servers.is_empty());
}

#[test]
fn keeps_selection_through_configuration_changes() {
    let mut info = info("service", &["server"]);
    let mut kind = kind(&info, "account", &["server"]);
    let mut account = account(&kind);
    let before = directory(&[kind.clone()], &[account.clone()], &[info.clone()]);
    info.summary.settings_revision += 1;
    info.summary.enabled = false;
    kind.package = info.summary.reference();
    kind.enabled = false;
    account.package = kind.package.clone();
    let after = directory(&[kind], &[account.clone()], &[info]);
    assert_eq!(before[0].key, after[0].key);
    assert_eq!(after[0].account.as_ref().unwrap().id, account.id);
    assert_eq!(
        super::super::status(&after[0]),
        Some("connector_package_disabled")
    );
}

#[test]
fn exposes_only_valid_legacy_switch_disablement() {
    let info = manual("mcp-custom");
    let kind = kind(&info, "server", &["server"]);
    let mut account = account(&kind);
    account.enabled = false;
    account.can_disconnect = false;
    let service = directory(&[kind], &[account.clone()], &[info]).remove(0);
    assert!(!service.allowed(&service.servers[0]));
    for (state, availability, reason) in [
        (
            connector::State::NotConnected,
            Availability::ConnectionRequired,
            "connector_not_connected",
        ),
        (
            connector::State::Connecting,
            Availability::ConnectionRequired,
            "connector_not_connected",
        ),
        (
            connector::State::Expired,
            Availability::AuthorizationExpired,
            "connector_expired",
        ),
        (
            connector::State::NeedsConfirmation,
            Availability::ConsentRequired,
            "connector_confirmation",
        ),
    ] {
        let mut changed = service.clone();
        changed.account.as_mut().unwrap().state = state;
        changed.servers[0].availability = availability;
        assert!(changed.allowed(&changed.servers[0]));
        assert_eq!(super::super::status(&changed), Some(reason));
    }
    let mut disabled = service.clone();
    disabled.account.as_mut().unwrap().enabled = true;
    disabled.info.as_mut().unwrap().summary.enabled = false;
    disabled.servers[0].availability = Availability::PackageDisabled;
    assert!(disabled.allowed(&disabled.servers[0]));
    assert_eq!(
        super::super::status(&disabled),
        Some("connector_package_disabled")
    );
    disabled.servers[0].allowed = false;
    assert!(!disabled.allowed(&disabled.servers[0]));
}
