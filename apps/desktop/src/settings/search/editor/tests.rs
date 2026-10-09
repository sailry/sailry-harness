use super::*;
use crate::settings::search::tests::fixture::{
    Fixture, draw, init, input, selected, tap, toast, wait,
};
use core::prelude::v1::test;
use std::sync::atomic::Ordering;

#[gpui::test]
fn blocks_stale_credentials_and_allows_close(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "search-edit-exa");
        input(visual, "search-api-key", "stale-target-key");
        fixture.bind(&owner, visual, true);
        tap(visual, "search-editor-save");
        toast(visual, "search_target_changed");
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        assert!(!fixture.settings().providers[&Driver::Exa].key_configured);
        let other = sailry_client::Client::new(fixture.other.local());
        let Output::SearchSettings(settings) = fixture
            .runtime
            .block_on(other.execute(other.prepare(Command::ReadSearchSettings)))
            .unwrap()
        else {
            panic!("settings expected")
        };
        assert!(!settings.providers[&Driver::Exa].key_configured);
        tap(visual, "search-editor-cancel");
        draw(visual);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        fixture.close(visual);
    }
}

#[gpui::test]
fn saves_private_credentials_and_tests_the_driver(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        let editor = visual.update(|window, cx| open(panel.clone(), Driver::Searxng, window, cx));
        input(
            visual,
            "search-endpoint",
            "https://search.example.com/search",
        );
        input(visual, "search-api-key", "private-search-key");
        input(visual, "search-username", "user");
        input(visual, "search-password", "password");
        editor.read_with(visual, |editor, cx| {
            for field in [&editor.key, &editor.username, &editor.password] {
                assert!(field.read(cx).presentation().is_masked());
            }
        });
        tap(visual, "search-editor-save");
        wait(visual, |cx| editor.read(cx).closed);
        toast(visual, "search_saved");
        let saved = fixture.settings();
        assert_eq!(
            saved.providers[&Driver::Searxng].endpoint.as_deref(),
            Some("https://search.example.com/search")
        );
        assert!(saved.providers[&Driver::Searxng].key_configured);
        assert!(saved.providers[&Driver::Searxng].basic_configured);
        assert!(
            !serde_json::to_string(&saved)
                .unwrap()
                .contains("private-search-key")
        );
        editor.read_with(visual, |editor, cx| {
            assert!(editor.key.read(cx).value().is_empty());
            assert!(editor.username.read(cx).value().is_empty());
            assert!(editor.password.read(cx).value().is_empty());
            assert!(editor.request.is_none());
        });
        visual.update(|window, cx| window.clear_notifications(cx));
        tap(visual, "search-test-searxng");
        wait(visual, |cx| !panel.read(cx).pending);
        toast(visual, "search_test_passed");
        let requests = fixture.server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].headers["x-api-key"], "private-search-key");
        assert_eq!(
            requests[0].headers["authorization"],
            "Basic dXNlcjpwYXNzd29yZA=="
        );
        drop(requests);
        assert_eq!(
            fixture.settings(),
            saved,
            "testing must not save another settings revision"
        );
        visual.update(|window, cx| window.clear_notifications(cx));
        tap(visual, "search-test-brave");
        wait(visual, |cx| {
            panel.read(cx).error == Some("search_not_configured")
        });
        toast(visual, "search_not_configured");
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        let editor = visual.update(|window, cx| open(panel.clone(), Driver::Searxng, window, cx));
        editor.read_with(visual, |editor, cx| {
            assert!(editor.key.read(cx).value().is_empty());
            assert!(editor.username.read(cx).value().is_empty());
            assert!(editor.password.read(cx).value().is_empty());
        });
        tap(visual, "search-remove-key");
        tap(visual, "search-remove-basic");
        tap(visual, "search-editor-save");
        wait(visual, |cx| editor.read(cx).closed);
        let saved = fixture.settings();
        assert!(!saved.providers[&Driver::Searxng].key_configured);
        assert!(!saved.providers[&Driver::Searxng].basic_configured);
        fixture.close(visual);
    }
}

#[gpui::test]
fn keeps_drafts_when_another_controller_saves(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        let editor = visual.update(|window, cx| open(panel.clone(), Driver::Exa, window, cx));
        input(visual, "search-api-key", "draft-key");
        let mut changed = fixture.settings();
        changed.fallback = false;
        fixture.execute(Command::SaveSearchSettings(Update {
            settings: changed,
            credentials: Default::default(),
        }));
        wait(visual, |cx| {
            panel.read(cx).saved.as_ref().unwrap().revision == 1
        });
        tap(visual, "search-editor-save");
        wait(visual, |cx| {
            editor.read(cx).error == Some("search_settings_conflict")
        });
        toast(visual, "search_settings_conflict");
        assert!(!fixture.settings().providers[&Driver::Exa].key_configured);
        editor.read_with(visual, |editor, cx| {
            assert_eq!(editor.key.read(cx).value(), "draft-key");
            assert!(editor.request.is_none());
            assert_eq!(editor.settings.revision, 0);
        });
        tap(visual, "search-editor-reload");
        wait(visual, |cx| {
            editor.read(cx).settings.revision == 1 && !editor.read(cx).pending
        });
        editor.read_with(visual, |editor, cx| {
            assert_eq!(editor.key.read(cx).value(), "draft-key")
        });
        tap(visual, "search-editor-save");
        wait(visual, |cx| editor.read(cx).closed);
        let saved = fixture.settings();
        assert_eq!(saved.revision, 2);
        assert!(!saved.fallback);
        assert!(saved.providers[&Driver::Exa].key_configured);
        fixture.close(visual);
    }
}

#[gpui::test]
fn retries_uncertain_save_with_the_same_request(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        let panel = visual.update(|_, cx| selected(&owner, cx).unwrap());
        let editor = visual.update(|window, cx| open(panel.clone(), Driver::Exa, window, cx));
        input(visual, "search-api-key", "retry-key");
        fixture.transport.lose_reply.store(true, Ordering::SeqCst);
        tap(visual, "search-editor-save");
        wait(visual, |cx| {
            editor.read(cx).error == Some("search_save_unknown") && !editor.read(cx).pending
        });
        toast(visual, "search_save_unknown");
        editor.read_with(visual, |editor, cx| {
            assert!(editor.request.is_some());
            assert_eq!(editor.key.read(cx).value(), "retry-key");
        });
        assert_eq!(fixture.settings().revision, 1);
        tap(visual, "search-editor-save");
        wait(visual, |cx| editor.read(cx).closed);
        assert_eq!(fixture.settings().revision, 1);
        let requests = fixture.transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
        drop(requests);
        fixture.close(visual);
    }
}
