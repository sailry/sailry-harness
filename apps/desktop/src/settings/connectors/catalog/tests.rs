use super::*;
use core::prelude::v1::test;
use sailry_protocol::{
    RequestId,
    plugin::{McpTransport, Summary, mcp::Availability},
};
use serde_json::json;

fn info(name: &str, servers: &[&str]) -> Info {
    Info {
        summary: Summary {
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
        changed_accounts(&[disconnected.clone()], &[connected.clone()]),
        package
    );
    assert_eq!(
        changed_accounts(&[connected.clone()], &[disconnected]),
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
        assert_eq!(changed_accounts(&[connected.clone()], &[changed]), package);
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

#[test]
fn includes_api_services_before_login() {
    let infos = [info("google", &[]), info("figma", &[])];
    let types = infos
        .iter()
        .map(|info| kind(info, &info.summary.name, &[]))
        .collect::<Vec<_>>();
    let services = directory(&types, &[], &infos);
    assert_eq!(services.len(), 2);
    assert!(
        services
            .iter()
            .all(|service| service.account.is_none() && service.servers.is_empty())
    );
    assert!(
        services
            .iter()
            .all(|service| service.matches(filter::Method::Secret, ""))
    );
}

#[test]
fn combines_explicit_token_and_oauth_bindings() {
    let info = info("github", &["github", "oauth"]);
    let mut kind = kind(&info, "account", &["github"]);
    kind.declaration.auth.push(connector::Auth::McpOauth {
        server: "oauth".into(),
        client: None,
    });
    let account = account(&kind);
    let services = directory(&[kind], &[account.clone()], &[info]);
    assert_eq!(services.len(), 1);
    assert_eq!(services[0].account.as_ref().unwrap().id, account.id);
    assert_eq!(
        services[0].account.as_ref().unwrap().configured,
        account.configured
    );
    assert_eq!(
        services[0]
            .servers
            .iter()
            .map(|server| server.name.as_str())
            .collect::<Vec<_>>(),
        ["github", "oauth"]
    );
}

#[test]
fn preserves_independent_server_statuses() {
    let mut info = info("service", &["read", "write"]);
    info.mcp[1].allowed = false;
    info.mcp[1].availability = Availability::ServerDisabled;
    let kind = kind(&info, "account", &["read", "write"]);
    let services = directory(&[kind], &[], &[info]);
    assert_eq!(services.len(), 1);
    assert!(services[0].servers[0].allowed);
    assert!(!services[0].servers[1].allowed);
    assert_eq!(services[0].servers[0].availability, Availability::Available);
    assert_eq!(
        services[0].servers[1].availability,
        Availability::ServerDisabled
    );
}

#[test]
fn keeps_no_auth_and_manual_servers_accountless() {
    let standard = info("no-auth", &["server"]);
    let mut manual = info("manual", &["server"]);
    manual.mcp_source = Some(
        serde_json::from_value(json!({
            "type":"stdio","command":"fixture","args":[],"env":[]
        }))
        .unwrap(),
    );
    let services = directory(&[], &[], &[standard, manual]);
    assert_eq!(services.len(), 2);
    assert!(
        services
            .iter()
            .all(|service| service.kind.is_none() && service.account.is_none())
    );
    assert!(
        services
            .iter()
            .all(|service| super::super::status(service).is_none())
    );
}

#[test]
fn leaves_optional_manual_auth_without_an_account() {
    let mut info = info("mcp-manual", &["server"]);
    info.mcp_source = Some(
        serde_json::from_value(json!({
            "type":"streamable-http","url":"https://example.com/mcp","headers":[]
        }))
        .unwrap(),
    );
    let mut kind = kind(&info, "server", &["server"]);
    kind.declaration.auth = vec![connector::Auth::McpOauth {
        server: "server".into(),
        client: None,
    }];
    let services = directory(&[kind], &[], &[info]);
    assert_eq!(services.len(), 1);
    assert!(services[0].account.is_none());
    assert_eq!(services[0].label(), "manual");
    assert!(super::super::status(&services[0]).is_none());
}

#[test]
fn never_merges_matching_names_across_packages() {
    let first = info("first", &["server"]);
    let second = info("second", &["server"]);
    let kind = kind(&first, "account", &["server"]);
    let account = account(&kind);
    let services = directory(&[kind], &[account], &[first, second]);
    assert_eq!(services.len(), 2);
    let second = services
        .iter()
        .find(|service| service.package.name == "second")
        .unwrap();
    assert!(second.kind.is_none() && second.account.is_none());
    assert_eq!(second.servers.len(), 1);
}

#[test]
fn retains_disabled_configuration_and_management() {
    let mut info = info("service", &["server"]);
    info.summary.enabled = false;
    info.mcp[0].availability = Availability::PackageDisabled;
    let kind = kind(&info, "account", &["server"]);
    let account = account(&kind);
    let services = directory(&[kind], &[account.clone()], &[info]);
    assert_eq!(services.len(), 1);
    let service = &services[0];
    assert!(!service.enabled());
    assert!(service.info.is_some());
    assert_eq!(service.account.as_ref().unwrap(), &account);
    assert_eq!(
        super::super::status(service),
        Some("connector_package_disabled")
    );
}

#[test]
fn does_not_join_mixed_package_revisions() {
    let mut info = info("service", &["server"]);
    let kind = kind(&info, "account", &["server"]);
    info.summary.settings_revision += 1;
    let services = directory(&[kind], &[], &[info]);
    assert_eq!(services.len(), 1);
    let account = services
        .iter()
        .find(|service| service.kind.is_some())
        .unwrap();
    assert!(account.info.is_none() && account.servers.is_empty());
}

#[test]
fn retains_accounts_after_declaration_removal() {
    let info = info("service", &[]);
    let account = account(&kind(&info, "removed", &[]));
    let services = directory(&[], &[account.clone()], &[info]);
    assert_eq!(services.len(), 1);
    assert!(services[0].kind.is_none());
    assert_eq!(services[0].account.as_ref().unwrap().id, account.id);
}

#[test]
fn exposes_declared_api_tools_without_probing() {
    let mut info = info("api", &[]);
    info.extension = Some(
        serde_json::from_value(json!({
            "api_version":"v1","tools":[
                {"name":"read","description":"Read account data"},
                {"name":"remote","server":"server","description":"Declared MCP output"}
            ]
        }))
        .unwrap(),
    );
    let kind = kind(&info, "account", &[]);
    let services = directory(&[kind], &[], &[info]);
    assert_eq!(services.len(), 1);
    assert!(services[0].servers.is_empty());
    assert!(services[0].account.is_none());
    let tools = services[0].tools().collect::<Vec<_>>();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "read");
    assert_eq!(tools[0].description.as_deref(), Some("Read account data"));
}

#[test]
fn keeps_selection_stable_across_configuration_changes() {
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
    assert_ne!(before[0].package, after[0].package);
}
