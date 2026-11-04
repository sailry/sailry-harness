use super::*;
use core::prelude::v1::test;
use sailry_protocol::plugin::{Summary, mcp::Availability};

fn service() -> catalog::Service {
    let package = Summary {
        compatibility: Default::default(),
        name: "fixture".into(),
        revision: 1,
        digest: "digest".into(),
        settings_revision: 3,
        enabled: true,
        version: None,
        description: None,
    }
    .reference();
    catalog::Service {
        key: catalog::Key {
            package: package.name.clone(),
            member: catalog::Member::Mcp,
        },
        package,
        kind: None,
        account: None,
        info: None,
        servers: Vec::new(),
    }
}

#[test]
fn scoped_results_and_drafts() {
    for (scenario, check) in [
        (
            "rejects_results_after_node_or_revision_changes",
            (|_scenario: &str| {
                let node = NodeId([1; 32]);
                let other = NodeId([2; 32]);
                let service = service();
                assert!(
                    current(node, Some(node), &service.package, Some(&service)),
                    "rejects results after node or revision changes"
                );
                assert!(
                    !current(node, Some(other), &service.package, Some(&service)),
                    "rejects results after node or revision changes"
                );
                assert!(
                    !current(node, None, &service.package, Some(&service)),
                    "rejects results after node or revision changes"
                );
                let mut newer = service.clone();
                newer.package.settings_revision += 1;
                assert!(
                    !current(node, Some(node), &service.package, Some(&newer)),
                    "rejects results after node or revision changes"
                );
                assert!(
                    !current(node, Some(node), &service.package, None),
                    "rejects results after node or revision changes"
                );
            }) as fn(&str),
        ),
        (
            "discards_checks_after_account_changes",
            (|_scenario: &str| {
                let mut service = service();
                let account: Connection = serde_json::from_value(serde_json::json!({
                "id": sailry_protocol::RequestId::new(), "package": service.package,
                "connector": "account", "method": "secret", "revision": 1,
                "enabled": true, "state": "connected", "can_disconnect": true,
                "identity": null, "expires_at_ms": null, "checked_at_ms": null,
                "fields": {}, "configured": ["token"]
                    }))
                .unwrap();
                service.account = Some(account.clone());
                let checked = Checked {
                    result: Check {
                        package: service.package.clone(),
                        server: "server".into(),
                        tools: Vec::new(),
                    },
                    account: Some(account.clone()),
                };
                assert!(
                    checked.current(&service),
                    "discards checks after account changes"
                );
                for changed in [
                    Connection {
                        revision: 2,
                        ..account.clone()
                    },
                    Connection {
                        enabled: false,
                        ..account.clone()
                    },
                    Connection {
                        state: connector::State::NotConnected,
                        ..account.clone()
                    },
                ] {
                    service.account = Some(changed);
                    assert!(
                        !checked.current(&service),
                        "discards checks after account changes"
                    );
                }
                service.account = None;
                assert!(
                    !checked.current(&service),
                    "discards checks after account changes"
                );
                service.account = Some(account);
                service.package.settings_revision += 1;
                assert!(
                    !checked.current(&service),
                    "discards checks after account changes"
                );
            }),
        ),
        (
            "permits_explicit_checks_before_agent_enablement",
            (|_scenario: &str| {
                let service = service();
                let mut server = Mcp {
                    name: "server".into(),
                    host: None,
                    transport: sailry_protocol::plugin::McpTransport::StreamableHttp,
                    allowed: false,
                    availability: Availability::ServerDisabled,
                };
                assert!(
                    checkable(&service, &server),
                    "permits explicit checks before agent enablement"
                );
                for unavailable in [
                    Availability::PackageDisabled,
                    Availability::ConfigurationRequired,
                    Availability::ConnectionRequired,
                    Availability::ConnectionDisabled,
                    Availability::AuthorizationExpired,
                    Availability::ConsentRequired,
                    Availability::Unavailable,
                ] {
                    server.availability = unavailable;
                    assert!(
                        !checkable(&service, &server),
                        "permits explicit checks before agent enablement"
                    );
                }
            }),
        ),
        (
            "retries_only_the_same_uncertain_toggle",
            (|_scenario: &str| {
                let service = service();
                let request = Request::new(
                    NodeId([1; 32]),
                    Command::SetPluginMcpAllowed {
                        package: service.package.clone(),
                        server: "server".into(),
                        allowed: false,
                    },
                );
                assert!(
                    same_toggle(&request, &service.package, "server", false),
                    "retries only the same uncertain toggle"
                );
                assert!(
                    !same_toggle(&request, &service.package, "server", true),
                    "retries only the same uncertain toggle"
                );
                assert!(
                    !same_toggle(&request, &service.package, "other", false),
                    "retries only the same uncertain toggle"
                );
                let mut newer = service.package.clone();
                newer.settings_revision += 1;
                assert!(
                    !same_toggle(&request, &newer, "server", false),
                    "retries only the same uncertain toggle"
                );
            }),
        ),
        (
            "retains_choices_after_sibling_revision_conflicts",
            (|_scenario: &str| {
                let mut service = service();
                service.package.settings_revision += 1;
                service.info = Some(
                serde_json::from_value(serde_json::json!({
            "summary":{"name":"fixture","revision":1,"digest":"digest","settings_revision":4,
                "enabled":true,"version":null,"description":null},
            "icon":null,"extension":null,"settings":null
                }))
                .unwrap(),
            );
                service.servers = vec![Mcp {
                    name: "server".into(),
                    host: None,
                    transport: sailry_protocol::plugin::McpTransport::Stdio,
                    allowed: true,
                    availability: Availability::Available,
                }];
                let mut drafts = BTreeMap::from([("server".into(), false)]);
                retain_drafts(&mut drafts, &BTreeSet::new(), &BTreeMap::new(), &service);
                assert!(
                    !drafts["server"],
                    "retains choices after sibling revision conflicts"
                );
                service.servers[0].allowed = false;
                retain_drafts(&mut drafts, &BTreeSet::new(), &BTreeMap::new(), &service);
                assert!(
                    drafts.is_empty(),
                    "retains choices after sibling revision conflicts"
                );
            }),
        ),
        (
            "keeps_drafts_during_metadata_refresh",
            (|_scenario: &str| {
                let service = service();
                let mut drafts = BTreeMap::from([("server".into(), false)]);
                retain_drafts(&mut drafts, &BTreeSet::new(), &BTreeMap::new(), &service);
                assert!(!drafts["server"], "keeps drafts during metadata refresh");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
