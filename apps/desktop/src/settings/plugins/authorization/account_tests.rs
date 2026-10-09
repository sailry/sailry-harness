use super::super::test_support::{Fixture, init, tap, wait};
use super::*;
use core::prelude::v1::test;
use sailry_protocol::plugin::oauth::Provider;
#[path = "../../../../../../crates/node-runtime/tests/plugins/oauth/server.rs"]
mod tokens;

#[gpui::test]
fn connects_registered_accounts_on_each_node(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let tokens = fixture
            .runtime
            .block_on(tokens::Server::start(false, false));
        fixture
            .node
            .set_plugin_oauth_fixture(Provider::Google, &tokens.endpoint);
        fixture.package("1.0.0");
        let root = fixture.directory.path().join("project/package");
        let path = root.join("plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["api_version"] =
            serde_json::json!(sailry_protocol::plugin::API_VERSION);
        manifest["extensions"]["dev.sailry.platform"]["settings_schema"] =
            serde_json::json!("dev.sailry.platform/settings.json");
        manifest["extensions"]["dev.sailry.platform"]["oauth"] = serde_json::json!({"provider":"google","scopes":["https://www.googleapis.com/auth/drive.readonly"],"credentials":["token"]});
        std::fs::write(path, manifest.to_string()).unwrap();
        std::fs::write(root.join("dev.sailry.platform/settings.json"),serde_json::json!({
            "$schema":sailry_protocol::plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
            "properties":{"token":{"type":"string","x-sailry-secret":{"origin":"https://www.googleapis.com","header":"Authorization","prefix":"Bearer "}}},"required":[]
        }).to_string()).unwrap();
        let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
            worktree: fixture.worktree,
            path: "package".into(),
            name: "example".into(),
            expected_revision: 0,
        }) else {
            panic!("plugin expected");
        };
        assert!(info.issues.is_empty(), "{:?}", info.issues);
        let (owner, visual) = fixture.mount(cx);
        let binding = owner.read_with(visual, |owner, _| {
            owner.provider_link.as_ref().unwrap().binding.clone()
        });
        let login = visual.update(|window, cx| {
            open_plugin(
                binding,
                info.summary.reference(),
                Provider::Google,
                window,
                cx,
            )
        });
        wait(visual, |cx| login.read(cx).configured == Some(false));
        login.update_in(visual, |login, window, cx| {
            login.client_id.update(cx, |input, cx| {
                input.set_value("registered-client", window, cx)
            });
            login.client_secret.update(cx, |input, cx| {
                input.set_value("private-client-marker", window, cx)
            });
        });
        tap(visual, "mcp-oauth-connect");
        wait(visual, |cx| {
            matches!(login.read(cx).state(), Some(State::Pending { .. }))
        });
        let url = login.read_with(visual, |login, _| match login.state().unwrap() {
            State::Pending { url, .. } => url.clone(),
            _ => unreachable!(),
        });
        let authorization = url::Url::parse(&url).unwrap();
        let fields: std::collections::BTreeMap<_, _> = authorization.query_pairs().collect();
        let mut callback = url::Url::parse(fields["redirect_uri"].as_ref()).unwrap();
        callback
            .query_pairs_mut()
            .append_pair("state", &fields["state"])
            .append_pair("code", "single-code");
        assert!(
            fixture
                .runtime
                .block_on(issuer::visit(callback.as_str()))
                .starts_with("HTTP/1.1 200")
        );
        wait(visual, |cx| {
            matches!(login.read(cx).state(), Some(State::Connected(_)))
        });
        assert_eq!(tokens.bodies.lock().unwrap().len(), 1);
        assert!(login.read_with(visual, |login, cx| {
            login.client_secret.read(cx).value().is_empty()
        }));
        tap(visual, "mcp-oauth-revoke");
        crate::prompts::tests::answer(visual, "mcp_oauth_remove");
        wait(visual, |cx| login.read(cx).configured == Some(false));
        let reference = login.read_with(visual, |login, _| login.package.clone());
        let Output::PluginAuthorization(status) =
            fixture.execute(Command::ReadPluginAuthorization { package: reference })
        else {
            panic!("account status expected");
        };
        assert!(!status.configured);
        assert!(!serde_json::to_string(&status).unwrap().contains("private-"));
        tap(visual, "mcp-oauth-cancel");
    }
}
