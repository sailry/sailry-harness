use super::super::test_support::{Fixture, init, tap, wait};
use super::*;
use core::prelude::v1::test;
use std::sync::atomic::Ordering;

fn setup(fixture: &Fixture) -> (issuer::Server, Reference) {
    let issuer = fixture
        .runtime
        .block_on(issuer::Server::start("http://127.0.0.1:1/mcp"));
    fixture.package("1.0.0");
    std::fs::write(
        fixture.directory.path().join("project/package/mcp.json"),
        serde_json::json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
            "mcpServers":{"input":{"type":"streamable-http", "url":issuer.endpoint}}
        })
        .to_string(),
    )
    .unwrap();
    let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.worktree,
        path: "package".into(),
        name: "example".into(),
        expected_revision: 0,
    }) else {
        panic!("plugin expected")
    };
    (issuer, info.summary.reference())
}

fn mount<'a>(
    fixture: &Fixture,
    package: Reference,
    cx: &'a mut TestAppContext,
) -> (Entity<Login>, &'a mut VisualTestContext) {
    let (owner, visual) = fixture.mount(cx);
    let binding = owner.read_with(visual, |owner, _| {
        owner.provider_link.as_ref().unwrap().binding.clone()
    });
    let login = visual.update(|window, cx| open(binding, package, "input".into(), window, cx));
    wait(visual, |cx| login.read(cx).configured.is_some());
    (login, visual)
}

#[gpui::test]
fn callbacks_and_revocation(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (issuer, package) = setup(&fixture);
        let (login, visual) = mount(&fixture, package, cx);
        fixture.transport.mode.store(1, Ordering::SeqCst);
        tap(visual, "mcp-oauth-connect");
        wait(visual, |cx| login.read(cx).error.is_some());
        let admitted = login.read_with(visual, |login, _| login.request.as_ref().unwrap().id);
        tap(visual, "mcp-oauth-retry");
        wait(visual, |cx| {
            matches!(login.read(cx).state(), Some(State::Pending { .. }))
        });
        assert_eq!(
            login.read_with(visual, |login, _| login
                .view
                .update
                .as_ref()
                .unwrap()
                .attempt
                .id),
            admitted
        );
        let url = login.read_with(visual, |login, _| match login.state().unwrap() {
            State::Pending { url, .. } => url.clone(),
            _ => unreachable!(),
        });
        let callback = fixture.runtime.block_on(issuer::Server::callback(&url));
        let mut forged = url::Url::parse(&callback).unwrap();
        forged.query_pairs_mut().append_pair("state", "forged");
        assert!(
            fixture
                .runtime
                .block_on(issuer::visit(forged.as_str()))
                .starts_with("HTTP/1.1 400")
        );
        assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 0);
        fixture.transport.mode.store(1, Ordering::SeqCst);
        assert!(
            fixture
                .runtime
                .block_on(issuer::visit(&callback))
                .starts_with("HTTP/1.1 200")
        );
        wait(visual, |cx| {
            matches!(login.read(cx).state(), Some(State::Connected(_)))
        });
        wait(visual, |cx| !login.read(cx).pending);
        tap(visual, "mcp-oauth-retry");
        wait(visual, |cx| login.read(cx).request.is_none());
        assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 1);
        tap(visual, "mcp-oauth-revoke");
        crate::prompts::tests::answer(visual, "mcp_oauth_remove");
        wait(visual, |cx| login.read(cx).configured == Some(false));
        let package = login.read_with(visual, |login, _| login.package.clone());
        let Output::McpAuthorization(status) = fixture.execute(Command::ReadMcpAuthorization {
            package,
            server: "input".into(),
        }) else {
            panic!("status expected")
        };
        assert!(!status.configured);
        tap(visual, "mcp-oauth-cancel");
    }
}

#[gpui::test]
fn escape_cancels_bound_attempt(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (issuer, package) = setup(&fixture);
        let (login, visual) = mount(&fixture, package, cx);
        tap(visual, "mcp-oauth-connect");
        wait(visual, |cx| {
            matches!(login.read(cx).state(), Some(State::Pending { .. }))
        });
        let attempt = login.read_with(visual, |login, _| {
            login.view.update.as_ref().unwrap().attempt.clone()
        });
        visual.simulate_keystrokes("escape");
        wait(visual, |cx| login.read(cx).closed);
        let state = fixture.runtime.block_on(async {
            let mut stream = fixture
                .client
                .subscribe_mcp_login(attempt.id)
                .await
                .unwrap();
            stream.next().await.unwrap()
        });
        assert!(
            matches!(state, sailry_protocol::Update::McpLogin(update) if update.state == State::Cancelled)
        );
        assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 0);
    }
}
