use super::*;

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "roundtrips_packaged_settings",
            (|scenario: &'static str| {
                for entry in [None, Some("dev.sailry.platform/settings.js")] {
                    let value = serde_json::json!({
                        "navigation":{"label":"Settings"}, "entry":entry,
                        "enabled_control":true, "placement":{"group":"tools", "order":300}
                    });
                    let page: Settings = serde_json::from_value(value).unwrap();
                    assert_eq!(page.entry.as_deref(), entry, "{scenario}");
                    assert!(page.enabled_control, "{scenario}");
                    let encoded = serde_json::to_value(&page).unwrap();
                    assert!(encoded.get("native").is_none(), "{scenario}");
                    assert_eq!(
                        serde_json::from_value::<Settings>(encoded).unwrap(),
                        page,
                        "{scenario}"
                    );
                }
            }) as fn(&'static str),
        ),
        (
            "accepts_localized_modules",
            (|scenario: &'static str| {
                assert!(
                    manifest(&["ui/main.js", "ui/locales/中文.json", "ui/components.mjs"]).valid(),
                    "{scenario}"
                );
                let command = crate::Command::ReadPluginView {
                    surface: crate::plugin::desktop::Surface::Workspace,
                    package: crate::plugin::Reference {
                        name: "example".into(),
                        digest: "a".repeat(64),
                        settings_revision: 0,
                    },
                };
                assert!(!command.durable(), "{scenario}");
                assert_eq!(
                    serde_json::from_value::<crate::Command>(
                        serde_json::to_value(&command).unwrap()
                    )
                    .unwrap(),
                    command,
                    "{scenario}"
                );
            }),
        ),
        (
            "validates_surface_requirements",
            (|scenario: &'static str| {
                type Change = fn(&mut Manifest);
                let navigation = || Navigation {
                    label: "Example".into(),
                    icon: Some(Icon::Name("reicon:folders/folder".into())),
                    locales: BTreeMap::new(),
                };
                for (case, change, valid) in [
                    (
                        "no workspace",
                        (|value| {
                            value.entry = None;
                            value.resources.clear();
                        }) as Change,
                        true,
                    ),
                    (
                        "unused resource",
                        |value| {
                            value.entry = None;
                        },
                        true,
                    ),
                    (
                        "unsafe unused resource",
                        |value| {
                            value.entry = None;
                            value.resources.push("../outside.js".into());
                        },
                        false,
                    ),
                    (
                        "contributions only",
                        |value| {
                            value.entry = None;
                            value.ui_entry = Some("ui/main.js".into());
                        },
                        true,
                    ),
                    (
                        "missing contribution script",
                        |value| {
                            value.entry = None;
                            value.ui_entry = Some("ui/missing.js".into());
                        },
                        false,
                    ),
                    (
                        "unsafe contribution resource",
                        |value| {
                            value.entry = None;
                            value.ui_entry = Some("ui/main.js".into());
                            value.resources.push("../outside.js".into());
                        },
                        false,
                    ),
                    (
                        "contribution navigation needs workspace",
                        |value| {
                            value.entry = None;
                            value.ui_entry = Some("ui/main.js".into());
                            value.navigation = Some(Navigation {
                                label: "Example".into(),
                                icon: Some(Icon::Name("reicon:folders/folder".into())),
                                locales: BTreeMap::new(),
                            });
                        },
                        false,
                    ),
                    (
                        "contribution panel needs workspace",
                        |value| {
                            value.entry = None;
                            value.ui_entry = Some("ui/main.js".into());
                            value.panel = Some(Navigation {
                                label: "Example".into(),
                                icon: None,
                                locales: BTreeMap::new(),
                            });
                        },
                        false,
                    ),
                    (
                        "worktree needs navigation",
                        |value| {
                            value.navigation_options.target = NavigationTarget::Worktree;
                            value.navigation_options.details = true;
                        },
                        false,
                    ),
                    (
                        "glyph missing",
                        |value| {
                            value.navigation = Some(Navigation {
                                label: "Example".into(),
                                icon: None,
                                locales: BTreeMap::new(),
                            });
                        },
                        false,
                    ),
                    (
                        "glyph named",
                        |value| {
                            value.navigation = Some(Navigation {
                                label: "Example".into(),
                                icon: Some(Icon::Name("reicon:folders/folder".into())),
                                locales: BTreeMap::new(),
                            });
                        },
                        true,
                    ),
                    (
                        "glyph unknown",
                        |value| {
                            value.navigation = Some(Navigation {
                                label: "Example".into(),
                                icon: Some(Icon::Name("reicon:not-found".into())),
                                locales: BTreeMap::new(),
                            });
                        },
                        false,
                    ),
                    (
                        "glyph SVG",
                        |value| {
                            value.navigation = Some(Navigation {
                            label: "Example".into(),
                            icon: Some(Icon::Svg {
                                svg: r#"<svg viewBox="0 0 24 24"><path d="M4 4h16v16H4z"/></svg>"#.into(),
                            }),
                            locales: BTreeMap::new(),
                        });
                        },
                        true,
                    ),
                    (
                        "preview scripts",
                        |value| {
                            value.previews = vec!["text/html".into(), "application/pdf".into()];
                        },
                        true,
                    ),
                    (
                        "preview without script",
                        |value| {
                            value.previews = vec!["text/html".into(), "application/pdf".into()];
                            value.entry = None;
                        },
                        false,
                    ),
                    (
                        "overlay without controller",
                        |value| {
                            value.ui_overlay = true;
                        },
                        false,
                    ),
                    (
                        "overlay controller",
                        |value| {
                            value.ui_overlay = true;
                            value.ui_entry = Some("ui/main.js".into());
                        },
                        true,
                    ),
                    (
                        "shared without controller",
                        |value| {
                            value.ui_shared = true;
                        },
                        false,
                    ),
                    (
                        "shared controller",
                        |value| {
                            value.ui_shared = true;
                            value.ui_entry = Some("ui/main.js".into());
                        },
                        true,
                    ),
                ] {
                    let mut value = manifest(&["ui/main.js"]);
                    change(&mut value);
                    assert_eq!(value.valid(), valid, "{scenario}: {case}");
                    if valid {
                        assert_eq!(
                            serde_json::from_value::<Manifest>(
                                serde_json::to_value(&value).unwrap()
                            )
                            .unwrap(),
                            value,
                            "{scenario}: {case}: wire"
                        );
                    }
                }
                let mut value = manifest(&["ui/main.js"]);
                value.navigation_options.target = NavigationTarget::Worktree;
                value.navigation_options.details = true;
                value.navigation = Some(navigation());
                assert!(value.valid(), "{scenario}: worktree navigation");
                let encoded = serde_json::to_value(&value).unwrap();
                assert_eq!(
                    encoded["navigation_options"]["target"], "worktree",
                    "{scenario}: worktree target"
                );
                assert_eq!(
                    serde_json::from_value::<Manifest>(encoded).unwrap(),
                    value,
                    "{scenario}: worktree wire"
                );
                value.entry = None;
                assert!(!value.valid(), "{scenario}: worktree missing entry");
                for invalid in [
                    "text/html",
                    "text/*",
                    "text/html; charset=utf-8",
                    "html",
                    "text/",
                ] {
                    let mut value = manifest(&["ui/main.js"]);
                    value.previews =
                        vec!["text/html".into(), "application/pdf".into(), invalid.into()];
                    assert!(!value.valid(), "{scenario}: preview {invalid}");
                }
                let mut encoded = serde_json::to_value(manifest(&["ui/main.js"])).unwrap();
                encoded.as_object_mut().unwrap().remove("ui_shared");
                assert!(
                    !serde_json::from_value::<Manifest>(encoded)
                        .unwrap()
                        .ui_shared,
                    "{scenario}: shared default"
                );
            }),
        ),
        (
            "renderer_requires_creation_and_resource_access",
            (|scenario: &'static str| {
                let mut extension: crate::plugin::Extension =
                    serde_json::from_value(serde_json::json!({
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
                assert!(renderer.valid(&extension), "{scenario}");
                extension.actions.pop();
                assert!(!renderer.valid(&extension), "{scenario}");
                extension
                    .actions
                    .push(crate::plugin::Action::ControlTerminals);
                extension.ui[0].slot = crate::plugin::ui::Slot::Composer;
                assert!(!renderer.valid(&extension), "{scenario}");
                let mut desktop = manifest(&["ui/main.js"]);
                desktop.renderers = vec![renderer.clone(), renderer];
                assert!(!desktop.valid(), "{scenario}");
            }),
        ),
        (
            "browser_renderer_uses_captured_session_resource",
            (|scenario: &'static str| {
                let mut extension: crate::plugin::Extension =
                    serde_json::from_value(serde_json::json!({
                        "api_version":"v1", "actions":["browser.read", "browser.control"]
                    }))
                    .unwrap();
                let mut renderer = Renderer {
                    resource: ResourceKind::Browser,
                    create: None,
                    shortcut: None,
                };
                assert!(renderer.valid(&extension), "{scenario}");
                extension.actions.pop();
                assert!(!renderer.valid(&extension), "{scenario}");
                extension
                    .actions
                    .push(crate::plugin::Action::ControlBrowser);
                renderer.create = Some("create".into());
                assert!(!renderer.valid(&extension), "{scenario}");
                let mut desktop = manifest(&["ui/main.js"]);
                desktop.renderers = vec![renderer];
                assert!(!desktop.valid(), "{scenario}");
                desktop.renderers[0].create = None;
                assert!(desktop.valid(), "{scenario}");
                desktop.entry = None;
                desktop.resources.clear();
                assert!(!desktop.valid(), "{scenario}");
            }),
        ),
        (
            "renderer_shortcuts_are_optional_bounded_data",
            (|scenario: &'static str| {
                let mut desktop = manifest(&["ui/main.js"]);
                desktop.renderers.push(Renderer {
                    resource: ResourceKind::Documents,
                    create: None,
                    shortcut: None,
                });
                assert!(desktop.valid(), "{scenario}");
                for key in ["secondary-p", "ctrl-g ctrl-g", "ctrl-`"] {
                    desktop.renderers[0].shortcut = Some(key.into());
                    assert!(desktop.valid(), "{scenario}");
                    assert_eq!(
                        serde_json::from_value::<Manifest>(serde_json::to_value(&desktop).unwrap())
                            .unwrap(),
                        desktop,
                        "{scenario}"
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
                    assert!(!desktop.valid(), "{scenario}");
                }
            }),
        ),
        (
            "navigation_placement_defaults_and_round_trips",
            (|scenario: &'static str| {
                let mut value = serde_json::to_value(manifest(&["ui/main.js"])).unwrap();
                value.as_object_mut().unwrap().remove("navigation_options");
                let decoded: Manifest = serde_json::from_value(value.clone()).unwrap();
                assert_eq!(
                    decoded.navigation_options,
                    NavigationOptions::default(),
                    "{scenario}"
                );
                value["navigation_options"] = serde_json::json!({"pinned": true, "order": 300});
                let decoded: Manifest = serde_json::from_value(value).unwrap();
                assert_eq!(
                    decoded.navigation_options,
                    NavigationOptions {
                        pinned: true,
                        order: 300,
                        ..Default::default()
                    },
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::from_value::<Manifest>(serde_json::to_value(&decoded).unwrap())
                        .unwrap(),
                    decoded,
                    "{scenario}"
                );
            }),
        ),
        (
            "settings_placement_defaults_and_round_trips",
            (|scenario: &'static str| {
                let mut value =
                    serde_json::json!({"navigation":{"label":"Models"},"entry":"settings.js"});
                let decoded: Settings = serde_json::from_value(value.clone()).unwrap();
                assert_eq!(
                    decoded.placement,
                    SettingsPlacement::default(),
                    "{scenario}"
                );
                value["placement"] = serde_json::json!({"group":"ai","order":200});
                let decoded: Settings = serde_json::from_value(value.clone()).unwrap();
                assert_eq!(decoded.placement.group, SettingsGroup::Ai, "{scenario}");
                assert_eq!(decoded.placement.order, 200, "{scenario}");
                assert_eq!(
                    serde_json::from_value::<Settings>(serde_json::to_value(&decoded).unwrap())
                        .unwrap(),
                    decoded,
                    "{scenario}"
                );
                value["placement"]["group"] = serde_json::json!("unknown");
                assert!(
                    serde_json::from_value::<Settings>(value).is_err(),
                    "{scenario}"
                );
            }),
        ),
        (
            "rejects_invalid_modules",
            (|scenario: &'static str| {
                assert!(!manifest(&[]).valid(), "{scenario}");
                assert!(!manifest(&["ui/other.js"]).valid(), "{scenario}");
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
                    assert!(!value.valid(), "{scenario}: accepted {path:?}");
                    value.entry = None;
                    assert!(
                        !value.valid(),
                        "{scenario}: accepted settings resource {path:?}"
                    );
                }
                let mut value = manifest(&["ui/main.js"]);
                value
                    .resources
                    .extend((0..MAX_FILES).map(|index| format!("ui/{index}.js")));
                assert!(!value.valid(), "{scenario}");
            }),
        ),
        (
            "bounds_text_below_wire_limit",
            (|scenario: &'static str| {
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
                assert!(bundle.valid(), "{scenario}");
                assert!(
                    serde_json::to_vec(&crate::Output::PluginView(bundle.clone()))
                        .unwrap()
                        .len()
                        < crate::MAX_FRAME_BYTES,
                    "{scenario}"
                );
                bundle.files.get_mut("ui/main.js").unwrap().push('x');
                assert!(!bundle.valid(), "{scenario}");
            }),
        ),
        (
            "validates_and_localizes_navigation",
            (|scenario: &'static str| {
                let mut value = manifest(&["ui/main.js"]);
                value.navigation = Some(Navigation {
                    label: "Game".into(),
                    icon: Some(Icon::Name("reicon:devices/gamepad".into())),
                    locales: BTreeMap::from([("fr".into(), "Jeu".into())]),
                });
                assert!(value.valid(), "{scenario}");
                assert_eq!(
                    value.navigation.as_ref().unwrap().label("fr"),
                    "Jeu",
                    "{scenario}"
                );
                assert_eq!(
                    value.navigation.as_ref().unwrap().label("de"),
                    "Game",
                    "{scenario}"
                );
                for label in ["", "\n", &"x".repeat(129)] {
                    value.navigation.as_mut().unwrap().label = label.into();
                    assert!(!value.valid(), "{scenario}");
                }
                assert!(
                    crate::Command::GeneratePluginText {
                        effort: None,
                        model: "provider/model".into(),
                        prompt: "Choose".into()
                    }
                    .durable(),
                    "{scenario}"
                );
            }),
        ),
        (
            "accepts_optional_localized_panel",
            (|scenario: &'static str| {
                let mut value = manifest(&["ui/main.js"]);
                assert!(value.panel.is_none(), "{scenario}");
                value.panel = Some(Navigation {
                    label: "Summary".into(),
                    icon: None,
                    locales: BTreeMap::from([("fr".into(), "Résumé".into())]),
                });
                assert!(value.valid(), "{scenario}");
                assert_eq!(
                    value.panel.as_ref().unwrap().label("fr"),
                    "Résumé",
                    "{scenario}"
                );
                assert!(value.navigation.is_none(), "{scenario}");
                value.panel.as_mut().unwrap().label.clear();
                assert!(!value.valid(), "{scenario}");
            }),
        ),
        (
            "confines_image_paths_and_combined_resource_count",
            (|scenario: &'static str| {
                let mut bundle = Bundle {
                    package: super::super::Reference {
                        name: "art".into(),
                        digest: "a".repeat(64),
                        settings_revision: 0,
                    },
                    entry: "main.js".into(),
                    files: BTreeMap::from([(
                        "main.js".into(),
                        "export default class Art {}".into(),
                    )]),
                    images: BTreeMap::from([("art/token.png".into(), "AAAA".into())]),
                };
                assert!(bundle.valid(), "{scenario}");
                for path in ["../token.png", "/token.png", "art/token.js"] {
                    bundle.images = BTreeMap::from([(path.into(), "AAAA".into())]);
                    assert!(!bundle.valid(), "{scenario}");
                }
                bundle.images = (0..MAX_FILES)
                    .map(|i| (format!("art/{i}.png"), "AAAA".into()))
                    .collect();
                assert!(!bundle.valid(), "{scenario}");
                bundle.images.remove("art/0.png");
                assert!(bundle.valid(), "{scenario}");
                bundle.images.insert("art/1.png".into(), "\0\0\0\0".into());
                assert!(!bundle.valid(), "{scenario}");
            }),
        ),
        (
            "document_renderer_uses_host_file_access",
            (|scenario: &'static str| {
                let mut extension: super::super::Extension =
                    serde_json::from_value(serde_json::json!({
                        "api_version":"v1", "actions":["files.read"]
                    }))
                    .unwrap();
                let mut renderer = Renderer {
                    resource: ResourceKind::Documents,
                    create: None,
                    shortcut: None,
                };
                assert!(renderer.valid(&extension), "{scenario}");
                extension.actions.clear();
                assert!(!renderer.valid(&extension), "{scenario}");
                extension.actions.push(super::super::Action::ReadFiles);
                extension.scope = super::super::Scope::Desktop;
                assert!(!renderer.valid(&extension), "{scenario}");
                extension.scope = super::super::Scope::Host;
                renderer.create = Some("create".into());
                assert!(!renderer.valid(&extension), "{scenario}");
            }),
        ),
        (
            "navigation_ignores_removed_options",
            (|scenario: &'static str| {
                let options: NavigationOptions =
                    serde_json::from_value(serde_json::json!({})).unwrap();
                assert_eq!(options, NavigationOptions::default(), "{scenario}");
                let mut captured = serde_json::json!({
                    "pinned": true,
                    "order": 42,
                    "target": "worktree",
                    "surface": "workspace",
                    "details": true,
                    "retention": {"mode": "always"}
                });
                let options: NavigationOptions = serde_json::from_value(captured.clone()).unwrap();
                captured.as_object_mut().unwrap().remove("retention");
                assert_eq!(
                    serde_json::to_value(options).unwrap(),
                    captured,
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::from_value::<NavigationOptions>(captured).unwrap(),
                    options,
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

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
