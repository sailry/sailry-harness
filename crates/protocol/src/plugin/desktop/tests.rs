use super::*;

fn manifest(paths: &[&str]) -> Manifest {
    Manifest {
        navigation: None,
        navigation_options: Default::default(),
        panel: None,
        conversations: vec![],
        renderers: vec![],
        previews: vec![],
        entry: Some("ui/main.js".into()),
        ui_entry: None,
        ui_overlay: false,
        ui_shared: false,
        resources: paths.iter().map(|path| (*path).into()).collect(),
    }
}

#[test]
fn accepts_localized_modules() {
    assert!(manifest(&["ui/main.js", "ui/locales/中文.json", "ui/components.mjs"]).valid());
    let command = crate::Command::ReadPluginView {
        surface: crate::plugin::desktop::Surface::Workspace,
        package: crate::plugin::Reference {
            name: "example".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
    };
    assert!(!command.durable());
    assert_eq!(
        serde_json::from_value::<crate::Command>(serde_json::to_value(&command).unwrap()).unwrap(),
        command
    );
}

#[test]
fn validates_resources_without_a_workspace_entry() {
    let mut native = manifest(&[]);
    native.entry = None;
    assert!(native.valid());
    native.resources.push("ui/main.js".into());
    assert!(native.valid());
    native.resources.push("../outside.js".into());
    assert!(!native.valid());
}

#[test]
fn contribution_only_modules_need_no_workspace_entry() {
    let mut value = manifest(&["ui/main.js"]);
    value.entry = None;
    value.ui_entry = Some("ui/main.js".into());
    assert!(value.valid());
    value.ui_entry = Some("ui/missing.js".into());
    assert!(!value.valid());
    value.ui_entry = Some("ui/main.js".into());
    value.resources.push("../outside.js".into());
    assert!(!value.valid());
    value.resources.pop();
    value.navigation = Some(Navigation {
        label: "Example".into(),
        locales: Default::default(),
        icon: Some(Icon::Name("reicon:folders/folder".into())),
    });
    assert!(!value.valid());
    value.navigation = None;
    value.panel = Some(Navigation {
        label: "Example".into(),
        locales: Default::default(),
        icon: None,
    });
    assert!(!value.valid());
}

#[test]
fn renderer_requires_creation_and_resource_access() {
    let mut extension: crate::plugin::Extension = serde_json::from_value(serde_json::json!({
        "api_version": "v1", "actions": ["terminals.read", "terminals.control"],
        "ui": [{"id": "create", "slot": "project", "kind": "button",
            "label": {"label": "Create"}, "handler": "create"}]
    }))
    .unwrap();
    let renderer = Renderer {
        resource: ResourceKind::Terminal,
        create: Some("create".into()),
        shortcut: None,
    };
    assert!(renderer.valid(&extension));
    extension.actions.pop();
    assert!(!renderer.valid(&extension));
    extension
        .actions
        .push(crate::plugin::Action::ControlTerminals);
    extension.ui[0].slot = crate::plugin::ui::Slot::Composer;
    assert!(!renderer.valid(&extension));
    let mut desktop = manifest(&["ui/main.js"]);
    desktop.renderers = vec![renderer.clone(), renderer];
    assert!(!desktop.valid());
}

#[test]
fn browser_renderer_uses_captured_session_resource() {
    let mut extension: crate::plugin::Extension = serde_json::from_value(serde_json::json!({
        "api_version":"v1", "actions":["browser.read", "browser.control"]
    }))
    .unwrap();
    let mut renderer = Renderer {
        resource: ResourceKind::Browser,
        create: None,
        shortcut: None,
    };
    assert!(renderer.valid(&extension));
    extension.actions.pop();
    assert!(!renderer.valid(&extension));
    extension
        .actions
        .push(crate::plugin::Action::ControlBrowser);
    renderer.create = Some("create".into());
    assert!(!renderer.valid(&extension));
    let mut desktop = manifest(&["ui/main.js"]);
    desktop.renderers = vec![renderer];
    assert!(!desktop.valid());
    desktop.renderers[0].create = None;
    assert!(desktop.valid());
    desktop.entry = None;
    desktop.resources.clear();
    assert!(!desktop.valid());
}

#[test]
fn renderer_shortcuts_are_optional_bounded_data() {
    let mut desktop = manifest(&["ui/main.js"]);
    desktop.renderers.push(Renderer {
        resource: ResourceKind::Documents,
        create: None,
        shortcut: None,
    });
    assert!(desktop.valid());
    for key in ["secondary-p", "ctrl-g ctrl-g", "ctrl-`"] {
        desktop.renderers[0].shortcut = Some(key.into());
        assert!(desktop.valid());
        assert_eq!(
            serde_json::from_value::<Manifest>(serde_json::to_value(&desktop).unwrap()).unwrap(),
            desktop
        );
    }
    for key in [
        "",
        " ",
        "ctrl-g ctrl-g ctrl-g",
        "ctrl-g\nctrl-g",
        "中文",
        &"x".repeat(129),
    ] {
        desktop.renderers[0].shortcut = Some(key.into());
        assert!(!desktop.valid());
    }
}

#[test]
fn navigation_placement_defaults_and_round_trips() {
    let mut value = serde_json::to_value(manifest(&["ui/main.js"])).unwrap();
    value.as_object_mut().unwrap().remove("navigation_options");
    let decoded: Manifest = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded.navigation_options, NavigationOptions::default());
    value["navigation_options"] = serde_json::json!({"pinned": true, "order": 300});
    let decoded: Manifest = serde_json::from_value(value).unwrap();
    assert_eq!(
        decoded.navigation_options,
        NavigationOptions {
            pinned: true,
            order: 300,
            ..Default::default()
        }
    );
    assert_eq!(
        serde_json::from_value::<Manifest>(serde_json::to_value(&decoded).unwrap()).unwrap(),
        decoded
    );
}

#[test]
fn settings_placement_defaults_and_round_trips() {
    let mut value = serde_json::json!({"navigation":{"label":"Models"},"entry":"settings.js"});
    let decoded: Settings = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded.placement, SettingsPlacement::default());
    value["placement"] = serde_json::json!({"group":"ai","order":200});
    let decoded: Settings = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded.placement.group, SettingsGroup::Ai);
    assert_eq!(decoded.placement.order, 200);
    assert_eq!(
        serde_json::from_value::<Settings>(serde_json::to_value(&decoded).unwrap()).unwrap(),
        decoded
    );
    value["placement"]["group"] = serde_json::json!("unknown");
    assert!(serde_json::from_value::<Settings>(value).is_err());
}

#[test]
fn worktree_workspace_requires_a_navigation_entry() {
    let mut value = manifest(&["ui/main.js"]);
    value.navigation_options.target = NavigationTarget::Worktree;
    value.navigation_options.details = true;
    assert!(!value.valid());
    value.navigation = Some(Navigation {
        label: "Workspace".into(),
        icon: Some(Icon::Name("reicon:folders/folder".into())),
        locales: BTreeMap::new(),
    });
    assert!(value.valid());
    let encoded = serde_json::to_value(&value).unwrap();
    assert_eq!(encoded["navigation_options"]["target"], "worktree");
    assert_eq!(serde_json::from_value::<Manifest>(encoded).unwrap(), value);
    value.entry = None;
    assert!(!value.valid());
}

#[test]
fn rejects_invalid_modules() {
    assert!(!manifest(&[]).valid());
    assert!(!manifest(&["ui/other.js"]).valid());
    for path in [
        "ui/../secret",
        "/absolute",
        "C:/file",
        "ui\\file",
        "ui/./file",
        "ui//file",
        "ui/file\0",
        "ui/file.",
        "ui/file ",
        "ui/AUX.js",
        "ui/COM1",
        "ui/LPT¹.json",
        "ui/MAIN.js",
        "ui/main.js",
        "ui/file?.js",
    ] {
        let mut value = manifest(&["ui/main.js", path]);
        assert!(!value.valid(), "accepted {path:?}");
        value.entry = None;
        assert!(!value.valid(), "accepted settings resource {path:?}");
    }
    let mut value = manifest(&["ui/main.js"]);
    value
        .resources
        .extend((0..MAX_FILES).map(|index| format!("ui/{index}.js")));
    assert!(!value.valid());
}

#[test]
fn bounds_text_below_wire_limit() {
    let mut bundle = Bundle {
        package: super::super::Reference {
            name: "example".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
        entry: "ui/main.js".into(),
        images: BTreeMap::from([("ui/image.png".into(), "A".repeat(MAX_IMAGE_BYTES))]),
        files: BTreeMap::from([("ui/main.js".into(), "\0".repeat(MAX_BYTES))]),
    };
    assert!(bundle.valid());
    assert!(
        serde_json::to_vec(&crate::Output::PluginView(bundle.clone()))
            .unwrap()
            .len()
            < crate::MAX_FRAME_BYTES
    );
    bundle.files.get_mut("ui/main.js").unwrap().push('x');
    assert!(!bundle.valid());
}

#[test]
fn validates_and_localizes_navigation() {
    let mut value = manifest(&["ui/main.js"]);
    value.navigation = Some(Navigation {
        label: "Game".into(),
        icon: Some(Icon::Name("reicon:devices/gamepad".into())),
        locales: BTreeMap::from([("fr".into(), "Jeu".into())]),
    });
    assert!(value.valid());
    assert_eq!(value.navigation.as_ref().unwrap().label("fr"), "Jeu");
    assert_eq!(value.navigation.as_ref().unwrap().label("de"), "Game");
    for label in ["", "\n", &"x".repeat(129)] {
        value.navigation.as_mut().unwrap().label = label.into();
        assert!(!value.valid());
    }
    assert!(
        crate::Command::GeneratePluginText {
            effort: None,
            model: "provider/model".into(),
            prompt: "Choose".into()
        }
        .durable()
    );
}

#[test]
fn accepts_optional_localized_panel() {
    let mut value = manifest(&["ui/main.js"]);
    assert!(value.panel.is_none());
    value.panel = Some(Navigation {
        label: "Summary".into(),
        icon: None,
        locales: BTreeMap::from([("fr".into(), "Résumé".into())]),
    });
    assert!(value.valid());
    assert_eq!(value.panel.as_ref().unwrap().label("fr"), "Résumé");
    assert!(value.navigation.is_none());
    value.panel.as_mut().unwrap().label.clear();
    assert!(!value.valid());
}

#[test]
fn confines_image_paths_and_combined_resource_count() {
    let mut bundle = Bundle {
        package: super::super::Reference {
            name: "art".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
        entry: "main.js".into(),
        files: BTreeMap::from([("main.js".into(), "export default class Art {}".into())]),
        images: BTreeMap::from([("art/token.png".into(), "AAAA".into())]),
    };
    assert!(bundle.valid());
    for path in ["../token.png", "/token.png", "art/token.js"] {
        bundle.images = BTreeMap::from([(path.into(), "AAAA".into())]);
        assert!(!bundle.valid());
    }
    bundle.images = (0..MAX_FILES)
        .map(|i| (format!("art/{i}.png"), "AAAA".into()))
        .collect();
    assert!(!bundle.valid());
    bundle.images.remove("art/0.png");
    assert!(bundle.valid());
    bundle.images.insert("art/1.png".into(), "\0\0\0\0".into());
    assert!(!bundle.valid());
}

#[test]
fn navigation_requires_a_glyph() {
    let mut value = manifest(&["ui/main.js"]);
    value.navigation = Some(Navigation {
        label: "Example".into(),
        icon: None,
        locales: BTreeMap::new(),
    });
    assert!(!value.valid());
    value.navigation.as_mut().unwrap().icon = Some(Icon::Name("reicon:folders/folder".into()));
    assert!(value.valid());
    value.navigation.as_mut().unwrap().icon = Some(Icon::Name("reicon:not-found".into()));
    assert!(!value.valid());
    value.navigation.as_mut().unwrap().icon = Some(Icon::Svg {
        svg: r#"<svg viewBox="0 0 24 24"><path d="M4 4h16v16H4z"/></svg>"#.into(),
    });
    assert!(value.valid());
}

#[test]
fn previews_require_a_script_and_distinct_mime_types() {
    let mut value = manifest(&["ui/main.js"]);
    value.previews = vec!["text/html".into(), "application/pdf".into()];
    assert!(value.valid());
    value.entry = None;
    assert!(!value.valid());
    value.entry = Some("ui/main.js".into());
    for invalid in [
        "text/html",
        "text/*",
        "text/html; charset=utf-8",
        "html",
        "text/",
    ] {
        value.previews.push(invalid.into());
        assert!(!value.valid(), "{invalid}");
        value.previews.pop();
    }
}

#[test]
fn document_renderer_uses_host_file_access() {
    let mut extension: super::super::Extension = serde_json::from_value(serde_json::json!({
        "api_version":"v1", "actions":["files.read"]
    }))
    .unwrap();
    let mut renderer = Renderer {
        resource: ResourceKind::Documents,
        create: None,
        shortcut: None,
    };
    assert!(renderer.valid(&extension));
    extension.actions.clear();
    assert!(!renderer.valid(&extension));
    extension.actions.push(super::super::Action::ReadFiles);
    extension.scope = super::super::Scope::Desktop;
    assert!(!renderer.valid(&extension));
    extension.scope = super::super::Scope::Host;
    renderer.create = Some("create".into());
    assert!(!renderer.valid(&extension));
}

#[test]
fn overlay_requires_a_controller() {
    let mut view = manifest(&["ui/main.js"]);
    view.ui_overlay = true;
    assert!(!view.valid());
    view.ui_entry = Some("ui/main.js".into());
    assert!(view.valid());
}

#[test]
fn shared_controls_require_a_controller() {
    let mut view = manifest(&["ui/main.js"]);
    let mut value = serde_json::to_value(&view).unwrap();
    value.as_object_mut().unwrap().remove("ui_shared");
    assert!(!serde_json::from_value::<Manifest>(value).unwrap().ui_shared);
    view.ui_shared = true;
    assert!(!view.valid());
    view.ui_entry = Some("ui/main.js".into());
    assert!(view.valid());
    assert_eq!(
        serde_json::from_value::<Manifest>(serde_json::to_value(&view).unwrap()).unwrap(),
        view,
    );
}
