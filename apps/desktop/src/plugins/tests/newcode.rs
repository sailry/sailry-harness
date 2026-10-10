use super::*;
use sailry_protocol::{Output, code, plugin};
use std::{
    collections::BTreeMap,
    os::unix::fs::PermissionsExt,
    path::{Component, Path},
};

fn fixture(remote: bool) -> Fixture {
    Fixture::with_environment(remote, |directory| {
        let bin = directory.join("bin");
        std::fs::create_dir(&bin).unwrap();
        sailry_node_runtime::EnvironmentFixture {
            home: directory.into(),
            shell: "/bin/false".into(),
            environment: BTreeMap::from([("PATH".into(), bin.into_os_string())]),
            timeout: Duration::from_secs(1),
        }
    })
}

fn catalog(fixture: &Fixture) -> BTreeMap<String, code::Candidate> {
    let Output::CodeCatalog(languages) = fixture.execute(Command::ReadCodeCatalog) else {
        panic!("language catalog expected");
    };
    let mut candidates = BTreeMap::new();
    for candidate in languages.into_iter().flat_map(|language| language.servers) {
        candidates
            .entry(candidate.server.clone())
            .or_insert(candidate);
    }
    candidates
}

fn install(fixture: &Fixture) -> plugin::Info {
    let root = fixture.directory.path().join("project/code");
    fixture::copy_package(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/code"),
        &root,
    );
    let view = root.join("dev.sailry.platform/desktop/view.js");
    let source = std::fs::read_to_string(&view).unwrap();
    let button = "new Button(id).label(label)";
    assert!(source.contains(button), "native button label probe target");
    // A transparent probe records the label passed to the unchanged Kit button.
    let source = source.replace(
        button,
        "new Button(id).label(label).relative().child(Bounds.new(`${id}-label-${label}`))",
    );
    std::fs::write(
        view,
        format!("import {{Bounds}} from 'sailry/test';\n{source}"),
    )
    .unwrap();
    let Output::Plugin(current) = fixture.execute(Command::ReadPlugin {
        name: "code".into(),
    }) else {
        panic!("language package expected");
    };
    let Output::Plugin(package) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "code".into(),
        name: "code".into(),
        expected_revision: current.summary.revision,
    }) else {
        panic!("language package expected");
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    package
}

fn mount<'a>(
    fixture: &Fixture,
    package: &plugin::Info,
    cx: &'a mut TestAppContext,
) -> (Entity<Panel>, &'a mut VisualTestContext) {
    let mut binding = fixture.binding.clone();
    binding.project = None;
    binding.worktree = None;
    let mut panel = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Panel::settings(binding, false, cx));
        panel = Some(view.clone());
        Root::new(view, window, cx)
    });
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1040.), px(820.)));
    let panel = panel.unwrap();
    wait(visual, |cx| {
        panel.read(cx).connected && panel.read(cx).metadata.read(cx).settled()
    });
    visual.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open(package.summary.reference(), window, cx)
        });
    });
    (panel, visual)
}

#[gpui::test]
fn filters_and_localizes_captured_settings(cx: &mut TestAppContext) {
    init(cx);
    rust_i18n::set_locale("zh-CN");
    for remote in [false, true] {
        let fixture = fixture(remote);
        let missing = catalog(&fixture);
        let unavailable = missing
            .values()
            .find(|candidate| {
                candidate.status == code::Status::NotInstalled
                    && candidate
                        .package
                        .as_ref()
                        .is_some_and(|package| package.unavailable.is_some())
                    && matches!(
                        Path::new(&candidate.command)
                            .components()
                            .collect::<Vec<_>>()
                            .as_slice(),
                        [Component::Normal(_)]
                    )
            })
            .expect("blocked package in the pinned catalog");
        let executable = fixture
            .directory
            .path()
            .join("bin")
            .join(&unavailable.command);
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let candidates = catalog(&fixture);
        assert_eq!(
            candidates[&unavailable.server].status,
            code::Status::Stopped
        );
        let eligible = candidates
            .values()
            .find(|candidate| {
                candidate.status == code::Status::NotInstalled
                    && candidate
                        .package
                        .as_ref()
                        .is_some_and(|package| package.install.is_some())
            })
            .expect("installable package in the pinned catalog");
        let package = install(&fixture);
        let (panel, visual) = mount(&fixture, &package, cx);
        wait(visual, |cx| {
            snapshot(&panel, cx).contains(&format!("\"code-server-{}\"", eligible.server))
        });
        let tree = panel.read_with(visual, |_, cx| snapshot(&panel, cx));
        for candidate in candidates.values() {
            let expected = candidate.status == code::Status::NotInstalled
                && candidate
                    .package
                    .as_ref()
                    .is_some_and(|package| package.install.is_some());
            assert_eq!(
                tree.contains(&format!("\"code-server-{}\"", candidate.server)),
                expected,
                "available server {}: remote={remote}",
                candidate.server,
            );
        }
        click(visual, "code-tabs-item-installed");
        wait(visual, |cx| {
            snapshot(&panel, cx).contains(&format!("\"code-server-{}\"", unavailable.server))
        });
        click(
            visual,
            Box::leak(format!("code-details-{}", unavailable.server).into_boxed_str()),
        );
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("code-server-details")
        });
        std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
        wait(visual, |_| true);
        let cancel = visual
            .debug_bounds("code-details-close-label-取消")
            .unwrap();
        let dialog = visual.debug_bounds("code-dialog-1").unwrap();
        assert!(cancel.size.width > px(0.) && cancel.size.width < dialog.size.width / 2.);
        assert!(
            visual
                .debug_bounds("code-detail-availability-info")
                .is_none()
        );
        assert!(!panel.read_with(visual, |_, cx| {
            snapshot(&panel, cx).contains("code-detail-availability-info")
        }));
        click(visual, "code-details-close-label-取消");
        wait(visual, |cx| {
            !snapshot(&panel, cx).contains("code-server-details")
        });
        assert_eq!(fixture.binding.client.target(), fixture.node.id());
        assert_ne!(fixture.binding.client.target(), fixture.controller.id());
        let requests = fixture.transport.requests.lock().unwrap();
        for command in [
            Command::ReadCodeSettings,
            Command::ReadCodeCatalog,
            Command::ListCodeInstallations,
            Command::ReadCodeRules,
        ] {
            assert!(
                requests.iter().any(|request| {
                    request.command == command
                        && request.plugin.as_ref().is_some_and(|context| {
                            context.package == package.summary.reference()
                                && context.surface == plugin::desktop::Surface::Settings
                                && context.worktree.is_none()
                                && context.session.is_none()
                        })
                }),
                "captured settings command {command:?}: remote={remote}"
            );
        }
        assert!(!requests.iter().any(|request| matches!(
            request.command,
            Command::SaveCodeSettings { .. }
                | Command::InstallCodeServer { .. }
                | Command::UninstallCodeServer { .. }
                | Command::CancelCodeInstall { .. }
                | Command::UpdateCodeRules
        )));
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
    rust_i18n::set_locale("en");
}
