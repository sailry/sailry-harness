use super::*;
use gpui_kit::component::{Root, WindowExt, dialog};
use sailry_protocol::{
    Command, NodeId, Output,
    search::{Driver, Settings, Update},
};
mod fixture;
use fixture::{init, *};

#[gpui::test]
fn protects_credentials_and_captured_nodes(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "search-prefer-native-toggle");
        wait(visual, |cx| {
            !fixture.settings().prefer_native && ready(&owner, cx)
        });
        tap(visual, "search-edit-exa");
        input(visual, "search-editor-key", "private-fixture-key");
        let panel = owner.read_with(visual, |owner, _| owner.plugin_settings_panel().unwrap());
        assert!(
            !panel
                .read_with(visual, |_, cx| crate::plugins::diagnostics(&panel, cx))
                .contains("private-fixture-key")
        );
        tap(visual, "search-editor-save");
        wait(visual, |cx| {
            fixture.settings().providers[&Driver::Exa].key_configured && ready(&owner, cx)
        });
        tap(visual, "search-test-exa");
        toast(visual, "search_test_passed");
        assert!(!fixture.server.requests.lock().unwrap().is_empty());
        wait(visual, |cx| ready(&owner, cx));
        tap(visual, "search-edit-exa");
        input(visual, "search-editor-key", "stale-draft");
        let mut settings = fixture.settings();
        settings.fallback = false;
        let revision = settings.revision + 2;
        fixture.execute(Command::SaveSearchSettings(Update {
            settings,
            credentials: Default::default(),
        }));
        tap(visual, "search-editor-save");
        toast(visual, "search_settings_conflict");
        assert!(visual.debug_bounds("search-editor-2").is_some());
        wait(visual, |cx| ready(&owner, cx));
        tap(visual, "search-editor-reload");
        tap(visual, "search-editor-save");
        wait(visual, |cx| {
            fixture.settings().revision == revision && ready(&owner, cx)
        });
        fixture.bind(&owner, visual, true);
        assert!(!panel.read_with(visual, |panel, _| panel.resource_active()));
        let other = sailry_client::Client::new(fixture.other.local());
        let Output::SearchSettings(settings) = fixture
            .runtime
            .block_on(other.execute(other.prepare(Command::ReadSearchSettings)))
            .unwrap()
        else {
            panic!("settings expected")
        };
        assert_eq!(settings, Settings::default());
        fixture.close(visual);
    }
}
