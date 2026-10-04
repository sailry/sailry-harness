use super::*;
use crate::shell::tests::setup;
use core::prelude::v1::test;

mod popovers;

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn pins_keep_the_active_page_and_menu(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    cx.update(|cx| cx.set_global(crate::preferences::Preferences::open(path.clone())));
    let (shell, mut cx) = setup(cx);
    click(&mut cx, "navigation-more");
    let row = "more-navigation-files";
    let bounds = cx.debug_bounds(row).unwrap();
    assert!(bounds.left() >= cx.debug_bounds("shell-feature-rail").unwrap().right());
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    click(&mut cx, "feature-pin-page:files");
    assert!(
        cx.debug_bounds(row).is_some(),
        "pinning keeps the menu open"
    );
    assert!(cx.debug_bounds("navigation-files").is_none());
    assert_eq!(
        shell.read_with(&cx, |shell, _| shell.page),
        Page::Conversation
    );
    assert!(
        !crate::preferences::Preferences::open(path.clone())
            .data
            .feature_pins
            .unwrap()["page:files"]
    );
    // The unpinned feature remains fully navigable through More.
    let row = cx.debug_bounds(row).unwrap();
    cx.simulate_click(
        row.origin + point(px(50.), row.size.height / 2.),
        Modifiers::default(),
    );
    cx.run_until_parked();
    assert_eq!(shell.read_with(&cx, |shell, _| shell.page), Page::Files);
    click(&mut cx, "navigation-more");
    click(&mut cx, "feature-pin-page:files");
    assert!(cx.debug_bounds("navigation-files").is_some());
    assert_eq!(shell.read_with(&cx, |shell, _| shell.page), Page::Files);
    assert!(
        crate::preferences::Preferences::open(path)
            .data
            .feature_pins
            .unwrap()["page:files"]
    );
    assert!(cx.debug_bounds("feature-pin-page:conversation").is_none());
    cx.simulate_keystrokes("escape");
    assert!(cx.debug_bounds("navigation-conversation").is_some());
}

#[gpui::test]
fn plugin_identity_survives_host_and_package_changes(cx: &mut TestAppContext) {
    use sailry_protocol::{
        NodeId,
        plugin::{Reference, Scope, desktop::Icon as Glyph},
    };
    let entry = crate::plugins::navigation::Entry {
        node: NodeId([1; 32]),
        worktree: None,
        scope: Scope::Desktop,
        navigation: Default::default(),
        label: "Example".into(),
        icon: Glyph::Name("reicon:devices/gamepad".into()),
        package: Reference {
            name: "example".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
    };
    let feature = Feature::from(entry.clone());
    cx.update(|cx| {
        assert!(!feature.pinned(cx));
        feature.toggle_pin(cx);
        let mut updated = entry;
        updated.node = NodeId([2; 32]);
        updated.package.digest = "b".repeat(64);
        let replacement = Feature::from(updated.clone());
        assert!(replacement.pinned(cx));
        updated.scope = Scope::Host;
        assert!(!Feature::from(updated).pinned(cx));
        replacement.toggle_pin(cx);
        assert!(!feature.pinned(cx));
        let mut republished = feature.clone();
        republished.default_pin = true;
        assert!(
            !republished.pinned(cx),
            "an explicit unpin overrides package defaults"
        );
    });
}

#[gpui::test]
fn package_placement_preserves_user_order(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|_, cx| {
        let mut features = shell.read(cx).features(cx);
        let entry = Feature::from(crate::plugins::navigation::Entry {
            node: sailry_protocol::NodeId([1; 32]),
            worktree: None,
            scope: sailry_protocol::plugin::Scope::Host,
            navigation: sailry_protocol::plugin::desktop::NavigationOptions {
                pinned: true,
                order: 300,
                ..Default::default()
            },
            label: "Example".into(),
            icon: sailry_protocol::plugin::desktop::Icon::Name("reicon:notifications/bell".into()),
            package: sailry_protocol::plugin::Reference {
                name: "example".into(),
                digest: "a".repeat(64),
                settings_revision: 0,
            },
        });
        assert!(entry.pinned(cx));
        features.push(entry);
        order::sort(&mut features, cx);
        let position = |key: &str, features: &[Feature]| {
            features
                .iter()
                .position(|feature| feature.key == key)
                .unwrap()
        };
        assert_eq!(
            position("plugin:host:example", &features),
            position("page:activity", &features) + 1
        );
        assert_eq!(
            position("page:files", &features),
            position("plugin:host:example", &features) + 1
        );
        assert!(order::move_feature(
            &features,
            "plugin:host:example",
            "page:git",
            true,
            cx
        ));
        order::sort(&mut features, cx);
        assert_eq!(
            position("plugin:host:example", &features),
            position("page:git", &features) + 1
        );
    });
}

#[gpui::test]
fn plugin_management_is_a_feature(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    click(&mut cx, "navigation-settings_plugins");
    assert_eq!(shell.read_with(&cx, |shell, _| shell.page), Page::Plugins);
    assert!(cx.debug_bounds("shell-navigation").is_none());
    assert!(cx.debug_bounds("plugins-install").is_some());
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        cx.update(|window, cx| Theme::change(mode, Some(window), cx));
        for (section, tab, control) in [
            (
                crate::settings::Section::Market,
                "plugins-market-tab",
                "plugins-install",
            ),
            (
                crate::settings::Section::Plugins,
                "plugins-manage-tab",
                "plugins-install",
            ),
            (
                crate::settings::Section::Skills,
                "plugins-skills-tab",
                "skills-install",
            ),
            (
                crate::settings::Section::Mcp,
                "plugins-mcp-tab",
                "mcp-open-plugins",
            ),
        ] {
            click(&mut cx, tab);
            assert_eq!(shell.read_with(&cx, |shell, _| shell.page), Page::Plugins);
            assert_eq!(
                shell.read_with(&cx, |shell, cx| shell.settings.read(cx).section),
                section
            );
            assert!(cx.debug_bounds("shell-navigation").is_none());
            assert!(cx.debug_bounds(control).is_some());
            let header = cx.debug_bounds("shell-module-header").unwrap();
            assert!(header.contains(&cx.debug_bounds(tab).unwrap().center()));
        }
    }
    click(&mut cx, "sidebar-settings");
    assert_eq!(shell.read_with(&cx, |shell, _| shell.page), Page::Settings);
    assert!(cx.debug_bounds("settings_plugins").is_none());
    assert!(cx.debug_bounds("settings_skills").is_none());
    assert!(cx.debug_bounds("settings_mcp").is_none());
    assert!(
        crate::settings::Section::GROUPS
            .iter()
            .all(|(_, sections)| { sections.iter().all(|section| !section.is_extension()) })
    );
    shell.read_with(&cx, |shell, cx| {
        assert_eq!(
            shell.settings.read(cx).section,
            crate::settings::Section::General
        );
    });
    cx.update(|_, cx| {
        shell.read(cx).settings.clone().update(cx, |settings, cx| {
            settings.select(crate::settings::Section::Mcp, cx);
        });
    });
    click(&mut cx, "mcp-open-plugins");
    assert_eq!(shell.read_with(&cx, |shell, _| shell.page), Page::Plugins);
    assert!(cx.debug_bounds("shell-navigation").is_none());
    assert!(cx.debug_bounds("plugins-install").is_some());
}

fn drag(cx: &mut VisualTestContext, source: &'static str, target: &'static str, after: bool) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let start = cx.debug_bounds(source).unwrap().center();
    let target = cx.debug_bounds(target).unwrap();
    let end = point(
        target.center().x,
        if after {
            target.bottom() - px(2.)
        } else {
            target.top() + px(2.)
        },
    );
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(
        start + point(px(10.), px(10.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn dragging_updates_menu_rail_and_saved_order(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    cx.update(|cx| cx.set_global(crate::preferences::Preferences::open(path.clone())));
    let (shell, mut cx) = setup(cx);
    click(&mut cx, "navigation-more");
    drag(
        &mut cx,
        "feature-drag-page:files",
        "more-navigation-conversation",
        false,
    );
    for (first, second) in [
        ("more-navigation-files", "more-navigation-conversation"),
        ("navigation-files", "navigation-conversation"),
    ] {
        assert!(cx.debug_bounds(first).unwrap().top() < cx.debug_bounds(second).unwrap().top());
    }
    assert_eq!(
        shell.read_with(&cx, |shell, _| shell.page),
        Page::Conversation
    );
    let saved = crate::preferences::Preferences::open(path.clone()).data;
    assert_eq!(
        saved.feature_order.as_ref().unwrap().first().unwrap(),
        "page:files"
    );
    assert_eq!(
        saved.feature_order.as_ref().unwrap().last().unwrap(),
        "page:settings_plugins"
    );
    assert!(
        saved.feature_pins.is_none(),
        "dragging does not change pins"
    );
    // A lower-half drop goes after the target, while management stays last.
    drag(
        &mut cx,
        "feature-drag-page:files",
        "more-navigation-settings_plugins",
        true,
    );
    assert!(
        cx.debug_bounds("more-navigation-files").unwrap().top()
            < cx.debug_bounds("more-navigation-settings_plugins")
                .unwrap()
                .top()
    );
    assert!(
        cx.debug_bounds("more-navigation-files").unwrap().top()
            > cx.debug_bounds("more-navigation-git").unwrap().top()
    );
    let saved = crate::preferences::Preferences::open(path.clone()).data;
    cx.simulate_keystrokes("escape");
    cx.update(|_, cx| cx.set_global(crate::preferences::Preferences::open(path)));
    click(&mut cx, "navigation-more");
    assert!(
        cx.debug_bounds("more-navigation-files").unwrap().top()
            > cx.debug_bounds("more-navigation-git").unwrap().top()
    );
    // The management row is not draggable and cannot move away from the tail.
    drag(
        &mut cx,
        "feature-drag-page:settings_plugins",
        "more-navigation-conversation",
        false,
    );
    assert_eq!(
        cx.update(|_, cx| crate::preferences::data(cx).feature_order),
        saved.feature_order
    );
    assert_eq!(
        shell.read_with(&cx, |shell, _| shell.page),
        Page::Conversation
    );
    cx.simulate_keystrokes("escape");
}

#[gpui::test]
fn ordering_keeps_unavailable_contributions(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|_, cx| {
        let mut features = shell.read(cx).features(cx);
        features.push(Feature::from(crate::plugins::navigation::Entry {
            node: sailry_protocol::NodeId([1; 32]),
            worktree: None,
            scope: sailry_protocol::plugin::Scope::Desktop,
            navigation: Default::default(),
            label: "Example".into(),
            icon: sailry_protocol::plugin::desktop::Icon::Name("reicon:devices/gamepad".into()),
            package: sailry_protocol::plugin::Reference {
                name: "example".into(),
                digest: "a".repeat(64),
                settings_revision: 0,
            },
        }));
        crate::preferences::update(cx, |data| {
            data.feature_order = Some(vec![
                "page:settings_plugins".into(),
                "plugin:desktop:unavailable".into(),
                "page:files".into(),
            ]);
        });
        order::sort(&mut features, cx);
        assert_eq!(features.last().unwrap().key, "page:settings_plugins");
        assert!(order::move_feature(
            &features,
            "plugin:desktop:example",
            "page:files",
            false,
            cx
        ));
        order::sort(&mut features, cx);
        assert_eq!(features[0].key, "plugin:desktop:example");
        assert_eq!(features[1].key, "page:files");
        assert_eq!(features.last().unwrap().key, "page:settings_plugins");
        let saved = crate::preferences::data(cx).feature_order.unwrap();
        assert!(saved.contains(&"plugin:desktop:unavailable".into()));
        assert!(!order::move_feature(
            &features,
            "page:settings_plugins",
            "page:files",
            false,
            cx
        ));
        assert!(!order::move_feature(
            &features,
            "unknown",
            "page:files",
            false,
            cx
        ));
        assert!(!order::move_feature(
            &features,
            "page:files",
            "unknown",
            false,
            cx
        ));
        assert_eq!(crate::preferences::data(cx).feature_order.unwrap(), saved);
    });
}
