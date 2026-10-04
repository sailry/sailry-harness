use super::*;
use crate::settings::{
    Section,
    plugins::test_support::{Fixture, draw, init, input, shown, tap, wait},
};
use core::prelude::v1::test;
use std::sync::atomic::Ordering;

#[gpui::test]
fn cards_open_owned_configuration(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        fixture.execute(Command::InstallPlugin {
            worktree: fixture.worktree,
            path: "package".into(),
            name: "example".into(),
            expected_revision: 0,
        });
        for name in ["mcp-alpha", "mcp-beta", "mcp-gamma"] {
            fixture.execute(Command::InstallMcp {
                name: name.into(),
                expected_revision: 0,
                definition: Definition::StreamableHttp {
                    url: "https://example.invalid/mcp".into(),
                    headers: Vec::new(),
                },
                secrets: Default::default(),
            });
        }
        let (owner, visual) = fixture.mount(cx);
        owner.update(visual, |owner, cx| owner.select(Section::Mcp, cx));
        shown(visual, "mcp-card-mcp-gamma", true);
        shown(visual, "plugin-component-example:native", false);
        // Plugin-provided servers precede the standalone cards in the inventory.
        let first = visual.debug_bounds("mcp-card-mcp-alpha").unwrap();
        let second = visual.debug_bounds("mcp-card-mcp-beta").unwrap();
        let last = visual.debug_bounds("mcp-card-mcp-gamma").unwrap();
        assert_eq!(second.top(), last.top());
        assert!(last.top() > first.top());
        assert_eq!(first.left(), last.left());
        assert!(second.left() < first.left());
        assert!((first.size.width - last.size.width).abs() < px(1.));
        tap(visual, "mcp-edit-mcp-alpha");
        shown(visual, "mcp-editor", true);
        tap(visual, "mcp-authorize");
        shown(visual, "mcp-oauth-connect", true);
        tap(visual, "mcp-oauth-cancel");
        shown(visual, "mcp-editor", true);
        tap(visual, "mcp-cancel");
        shown(visual, "mcp-editor", false);
        crate::settings::plugins::test_support::menu(visual, "mcp-menu-mcp-alpha", 1);
        crate::prompts::tests::answer(visual, "plugins_uninstall");
        shown(visual, "mcp-card-mcp-alpha", false);
        assert!(
            fixture
                .plugins()
                .iter()
                .any(|plugin| plugin.name == "example")
        );
        fixture.close(visual);
    }
}

fn mount(
    owner: &Entity<Workspace>,
    original: Option<Info>,
    cx: &mut VisualTestContext,
) -> Entity<Editor> {
    let binding = owner.read_with(cx, |owner, _| {
        owner.provider_link.as_ref().unwrap().binding.clone()
    });
    let editor = cx.update(|window, cx| open(binding, original, window, cx));
    wait(cx, |cx| !editor.read(cx).loading);
    draw(cx);
    editor
}

fn info(fixture: &Fixture) -> Info {
    let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
        name: fixture.plugins()[0].name.clone(),
    }) else {
        panic!("MCP information expected")
    };
    info
}

fn state(fixture: &Fixture) -> State {
    let Output::PluginSettings(state) = fixture.execute(Command::ReadPluginSettings {
        package: fixture.plugins()[0].reference(),
    }) else {
        panic!("MCP settings expected")
    };
    state
}

fn scroll(cx: &mut VisualTestContext) {
    // Match the click fixture: let the dialog finish opening before hit testing.
    std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    draw(cx);
    let editor = cx.debug_bounds("mcp-editor").unwrap();
    let footer = cx.debug_bounds("mcp-save").unwrap();
    // The editor selector belongs to the scrolling content. Its center may be
    // clipped or land on an input, so send the wheel to the visible body edge.
    let position = point(editor.right() - px(12.), footer.top() - px(48.));
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Pixels(point(px(0.), px(-700.))),
        touch_phase: TouchPhase::Moved,
        modifiers: Modifiers::default(),
    });
    draw(cx);
}

#[gpui::test]
fn edits_private_slots(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        owner.update(visual, |owner, cx| owner.select(Section::Mcp, cx));
        shown(visual, "mcp-add", true);
        let editor = mount(&owner, None, visual);
        let bar = visual.debug_bounds("mcp-name").unwrap();
        let tabs = ["mcp-transport-0", "mcp-transport-1", "mcp-transport-2"]
            .map(|id| visual.debug_bounds(id).unwrap());
        assert!((tabs[0].size.width - tabs[1].size.width).abs() <= px(1.));
        assert!((tabs[1].size.width - tabs[2].size.width).abs() <= px(1.));
        assert!(bar.right() - tabs[2].right() <= px(8.));
        input(visual, "mcp-name", "search");
        input(visual, "mcp-command", "fixture_mcp_server");
        tap(visual, "mcp-add-argument");
        input(visual, "mcp-argument-0", "an argument with spaces");
        scroll(visual);
        tap(visual, "mcp-add-slot");
        scroll(visual);
        input(visual, "mcp-slot-name-0", "TOKEN");
        input(visual, "mcp-slot-value-0", "fixture-private-token");
        let command = editor.read_with(visual, |editor, cx| editor.prepare(cx).unwrap());
        assert!(!format!("{command:?}").contains("fixture-private-token"));
        tap(visual, "mcp-save");
        shown(visual, "mcp-editor", false);
        let initial = info(&fixture);
        assert_eq!(
            initial.mcp_source,
            Some(Definition::Stdio {
                command: "fixture_mcp_server".into(),
                args: vec!["an argument with spaces".into()],
                env: vec!["TOKEN".into()],
            })
        );
        assert_eq!(state(&fixture).configured.len(), 1);
        assert!(editor.read_with(visual, |editor, cx| {
            editor
                .slots
                .iter()
                .all(|slot| slot.value.read(cx).value().is_empty())
        }));
        shown(visual, "mcp-card-mcp-search", true);
        shown(visual, "plugin-component-mcp-search:server", false);
        let editor = mount(&owner, Some(initial), visual);
        assert!(
            editor.read_with(visual, |editor, cx| editor.slots[0].configured
                && editor.slots[0].value.read(cx).value().is_empty())
        );
        tap(visual, "mcp-transport-1");
        input(visual, "mcp-url", "https://example.invalid/mcp");
        assert!(
            matches!(editor.read_with(visual, |editor, cx| editor.prepare(cx).unwrap()), Command::InstallMcp { secrets, .. } if secrets["TOKEN"] == SecretUpdate::Clear)
        );
        shown(visual, "mcp-clear-slot-0", false);
        tap(visual, "mcp-transport-0");
        input(visual, "mcp-command", "updated_mcp_server");
        assert!(
            matches!(editor.read_with(visual, |editor, cx| editor.prepare(cx).unwrap()), Command::InstallMcp { secrets, .. } if secrets["TOKEN"] == SecretUpdate::Keep)
        );
        tap(visual, "mcp-save");
        shown(visual, "mcp-editor", false);
        assert_eq!(state(&fixture).configured.len(), 1);
        let editor = mount(&owner, Some(info(&fixture)), visual);
        scroll(visual);
        let clear = visual.debug_bounds("mcp-clear-slot-0").unwrap();
        let footer = visual.debug_bounds("mcp-save").unwrap();
        assert!(
            clear.top() >= px(0.) && clear.bottom() <= footer.top(),
            "credential clear control is outside the dialog body: {clear:?}, footer: {footer:?}"
        );
        tap(visual, "mcp-clear-slot-0");
        assert!(editor.read_with(visual, |editor, _| editor.slots[0].clear));
        assert!(
            matches!(editor.read_with(visual, |editor, cx| editor.prepare(cx).unwrap()), Command::InstallMcp { secrets, .. } if secrets["TOKEN"] == SecretUpdate::Clear)
        );
        tap(visual, "mcp-save");
        shown(visual, "mcp-editor", false);
        assert!(state(&fixture).configured.is_empty());
        let revision = fixture.plugins()[0].revision;
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .metadata
                .as_ref()
                .unwrap()
                .read(cx)
                .entries
                .get("mcp-search")
                .is_some_and(|info| info.summary.revision == revision)
        });
        tap(visual, "mcp-toggle-mcp-search");
        wait(visual, |_| !fixture.plugins()[0].enabled);
        fixture.close(visual);
    }
}

#[gpui::test]
fn retries_on_captured_node(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        let editor = mount(&owner, None, visual);
        input(visual, "mcp-name", "remote");
        tap(visual, "mcp-transport-1");
        input(visual, "mcp-url", "https://example.invalid/mcp");
        tap(visual, "mcp-add-slot");
        scroll(visual);
        input(visual, "mcp-slot-name-0", "Authorization");
        input(visual, "mcp-slot-value-0", "Bearer fixture-private");
        fixture.transport.mode.store(1, Ordering::SeqCst);
        tap(visual, "mcp-save");
        wait(visual, |cx| {
            !editor.read(cx).pending && editor.read(cx).request.is_some()
        });
        let id = editor.read_with(visual, |editor, _| editor.request.as_ref().unwrap().id);
        assert!(editor.read_with(visual, |editor, _| editor.locked()));
        fixture.bind(&owner, visual, true);
        tap(visual, "mcp-save");
        shown(visual, "mcp-editor", false);
        assert_eq!(*fixture.transport.requests.lock().unwrap(), [id, id]);
        assert_eq!(state(&fixture).configured.len(), 1);
        assert!(matches!(
            info(&fixture).mcp_source,
            Some(Definition::StreamableHttp { .. })
        ));
        let client = sailry_client::Client::new(fixture.other.local());
        let Output::Plugins(plugins) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::ListPlugins)))
            .unwrap()
        else {
            panic!("plugins expected")
        };
        assert!(
            plugins
                .iter()
                .all(|plugin| !plugin.name.starts_with("mcp-"))
        );
        fixture.close(visual);
    }
}

#[gpui::test]
fn conflicts_and_stale_reads(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.execute(Command::InstallMcp {
            name: "mcp-existing".into(),
            expected_revision: 0,
            definition: Definition::Sse {
                url: "https://example.invalid/sse".into(),
                headers: vec!["Authorization".into()],
            },
            secrets: BTreeMap::from([(
                "Authorization".into(),
                SecretUpdate::Replace(Secret::new("original-private".into())),
            )]),
        });
        let original = info(&fixture);
        let (owner, visual) = fixture.mount(cx);
        let editor = mount(&owner, Some(original.clone()), visual);
        input(visual, "mcp-url", "https://example.invalid/my-draft");
        fixture.execute(Command::SetPluginEnabled {
            name: original.summary.name.clone(),
            expected_revision: original.summary.revision,
            enabled: false,
        });
        tap(visual, "mcp-save");
        wait(visual, |cx| {
            !editor.read(cx).pending && editor.read(cx).error == Some("plugins_conflict")
        });
        assert_eq!(
            editor.read_with(visual, |editor, cx| editor.url.read(cx).value()),
            "https://example.invalid/my-draft"
        );
        assert_eq!(state(&fixture).configured.len(), 1);
        tap(visual, "mcp-cancel");
        let current = info(&fixture);
        let field = current
            .settings
            .as_ref()
            .unwrap()
            .properties
            .keys()
            .next()
            .unwrap()
            .clone();
        fixture.execute(Command::SavePluginSettings {
            package: current.summary.reference(),
            values: BTreeMap::new(),
            secrets: BTreeMap::from([(field, SecretUpdate::Keep)]),
        });
        let editor = mount(&owner, Some(current), visual);
        assert!(!editor.read_with(visual, |editor, _| editor.loaded));
        let revision = fixture.plugins()[0].revision;
        tap(visual, "mcp-save");
        assert_eq!(fixture.plugins()[0].revision, revision);
        assert_eq!(state(&fixture).configured.len(), 1);
        tap(visual, "mcp-cancel");
        fixture.close(visual);
    }
}
