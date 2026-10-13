use super::*;
use gpui_kit::component::{
    WindowExt,
    input::{EditorState, GoToDefinition, HoverProvider},
};
use gpui_kit::test::TestWindowExt;
use sailry_protocol::{Permission, code};
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt, sync::Arc, time::Duration};

mod lifecycle;
mod observed;

fn fixture(remote: bool, permission: Permission) -> Fixture {
    configured(remote, permission, true)
}

fn configured(remote: bool, permission: Permission, editor_enabled: bool) -> Fixture {
    let fixture = Fixture::with_environment(remote, |directory| {
        let bin = directory.canonicalize().unwrap().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let python = std::process::Command::new("python3")
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            .unwrap();
        assert!(python.status.success());
        let python = String::from_utf8(python.stdout).unwrap();
        let server = bin.join("rust-analyzer");
        std::fs::write(
            &server,
            format!(
                "#!{}\n{}",
                python.trim(),
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../crates/node-runtime/src/code/tests/server.py"
                ))
            ),
        )
        .unwrap();
        std::fs::set_permissions(server, std::fs::Permissions::from_mode(0o700)).unwrap();
        sailry_node_runtime::EnvironmentFixture {
            home: directory.into(),
            shell: "/bin/false".into(),
            environment: BTreeMap::from([("PATH".into(), bin.into_os_string())]),
            timeout: Duration::from_secs(1),
        }
    });
    let root = fixture.directory.path().join("project");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(root.join("main.rs"), "🙂 broken symbol\n").unwrap();
    std::fs::write(root.join("target.rs"), "target symbol\n").unwrap();
    let mut config = fixture.session.config.clone();
    config.permission = permission;
    fixture.execute(Command::SetDefaults {
        expected_revision: 0,
        config,
    });
    let Output::CodeSettings(mut settings) = fixture.execute(Command::ReadCodeSettings) else {
        panic!("language settings expected")
    };
    assert!(!settings.editor_enabled);
    if editor_enabled {
        settings.editor_enabled = true;
        fixture.execute(Command::SaveCodeSettings {
            expected_revision: settings.revision,
            settings,
        });
    }
    fixture
}

fn editor_enabled(fixture: &Fixture, enabled: bool) {
    let Output::CodeSettings(mut settings) = fixture.execute(Command::ReadCodeSettings) else {
        panic!("language settings expected")
    };
    settings.editor_enabled = enabled;
    fixture.execute(Command::SaveCodeSettings {
        expected_revision: settings.revision,
        settings,
    });
}

fn open(
    panel: &Entity<Panel>,
    visual: &mut VisualTestContext,
    path: &'static str,
) -> (Entity<Controller>, RequestId, Entity<EditorState>) {
    click(visual, path);
    let controller = panel.read_with(visual, |panel, _| panel.documents.clone().unwrap());
    wait(visual, |cx| {
        controller
            .read(cx)
            .documents
            .values()
            .any(|document| document.path == "main.rs" && !document.dismissed)
    });
    let (id, input) = controller.read_with(visual, |controller, _| {
        controller
            .documents
            .iter()
            .find(|(_, document)| document.path == "main.rs")
            .map(|(id, document)| (*id, document.input.clone()))
            .unwrap()
    });
    (controller, id, input)
}

fn cold(controller: &Entity<Controller>, id: RequestId, visual: &mut VisualTestContext) {
    wait(visual, |cx| {
        controller.read(cx).documents[&id]
            .code
            .as_ref()
            .is_some_and(|provider| provider.startup.borrow().is_some())
    });
}

fn ready(input: &Entity<EditorState>, visual: &mut VisualTestContext) {
    wait(visual, |cx| {
        input
            .read(cx)
            .diagnostics()
            .is_some_and(|diagnostics| !diagnostics.is_empty())
    });
    visual.update(|window, cx| {
        assert_eq!(
            crate::feedback::tests::count(window, &crate::tr("editor_code_started"), cx),
            0
        );
    });
}

fn start(visual: &mut VisualTestContext) {
    click(visual, "editor-code-start");
    consent(visual);
    click(visual, "editor-code-start-confirm");
}

fn consent(visual: &mut VisualTestContext) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        wait(visual, |_| true);
        if visual.debug_bounds("editor-code-start-confirm").is_some() {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "language startup dialog deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn messages(fixture: &Fixture) -> Vec<Value> {
    std::fs::read_to_string(fixture.directory.path().join("project/messages"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn calls(fixture: &Fixture, method: &str) -> usize {
    messages(fixture)
        .iter()
        .filter(|message| message["method"] == method)
        .count()
}

#[gpui_kit::test]
fn asks_before_starting_and_uses_native_markers(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote, Permission::Ask);
        let (panel, visual) = publication::mount(&fixture, cx);
        let (controller, id, input) = open(&panel, visual, "resource-file-main.rs");
        cold(&controller, id, visual);
        assert!(!fixture.directory.path().join("project/pid").exists());
        click(visual, "editor-code-start");
        consent(visual);
        visual.update(|window, cx| window.close_dialog(cx));
        assert!(!fixture.directory.path().join("project/pid").exists());
        start(visual);
        ready(&input, visual);
        input.read_with(visual, |input, _| {
            let diagnostic = input.diagnostics().unwrap().iter().next().unwrap();
            assert_eq!(diagnostic.message.as_str(), "fixture syntax error");
            assert_eq!(diagnostic.range, "🙂 ".len().."🙂 broken".len());
            assert!(input.lsp().hover_provider.is_some());
            assert!(input.lsp().definition_provider.is_some());
            assert!(input.lsp().completion_provider.is_some());
        });
        assert_eq!(calls(&fixture, "initialize"), 1);
        let range = "🙂 broken ".len().."🙂 broken symbol".len();
        let bounds = input
            .read_with(visual, |input, _| input.range_to_bounds(&range))
            .unwrap();
        visual.simulate_mouse_move(bounds.center(), None, Modifiers::default());
        wait(visual, |cx| input.read(cx).hover_popover().is_some());
        input.read_with(visual, |input, _| {
            let contents = &input.hover_popover().unwrap().hover.contents;
            assert_eq!(
                contents,
                &lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                    kind: lsp_types::MarkupKind::PlainText,
                    value: "fixture signature ".into(),
                }),
                "hover contents: remote={remote}"
            );
        });
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(input);
        fixture.close();
    }
}

#[gpui_kit::test]
fn editor_switch_detaches_without_stopping_shared_servers(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = configured(remote, Permission::Full, false);
        let observed = Arc::new(observed::Observed::new(fixture.transport.clone()));
        fixture.binding.client = Arc::new(sailry_client::Client::new(observed.clone()));
        let (panel, visual) = publication::mount(&fixture, cx);
        let (controller, id, input) = open(&panel, visual, "resource-file-main.rs");
        wait(visual, |_| {
            observed
                .subscriptions
                .load(std::sync::atomic::Ordering::SeqCst)
                > 0
        });
        publication::replace(&panel, visual, "🙂 broken symbol changed\n");
        visual.run_until_parked();
        input.read_with(visual, |input, _| {
            assert!(input.lsp().hover_provider.is_none());
            assert!(input.lsp().definition_provider.is_none());
            assert!(input.lsp().completion_provider.is_none());
            assert!(input.lsp().show_document.is_none());
        });
        assert!(observed.requests.lock().unwrap().is_empty());
        assert!(!fixture.directory.path().join("project/pid").exists());
        editor_enabled(&fixture, true);
        ready(&input, visual);
        let provider = controller.read_with(visual, |controller, _| {
            controller.documents[&id].code.clone().unwrap()
        });
        let pid = std::fs::read_to_string(fixture.directory.path().join("project/pid")).unwrap();
        std::fs::write(
            fixture.directory.path().join("project/control"),
            "{\"hang\":true}",
        )
        .unwrap();
        let text = input.read_with(visual, |input, _| input.text().clone());
        let querying =
            visual.update(|window, cx| provider.hover(&text, "🙂 broken ".len(), window, cx));
        wait(visual, |_| calls(&fixture, "textDocument/hover") > 0);
        editor_enabled(&fixture, false);
        wait(visual, |cx| {
            controller.read(cx).documents[&id].code.is_none()
        });
        assert!(provider.stopped());
        assert!(visual.foreground_executor().block_test(querying).is_err());
        wait(visual, |_| {
            observed
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| matches!(request.command, Command::CancelCodeQuery { .. }))
        });
        std::fs::write(fixture.directory.path().join("project/control"), "{}").unwrap();
        input.read_with(visual, |input, _| {
            assert!(input.lsp().hover_provider.is_none());
            assert!(input.lsp().definition_provider.is_none());
            assert!(input.lsp().completion_provider.is_none());
            assert!(input.lsp().show_document.is_none());
            assert!(input.diagnostics().is_none_or(|items| items.is_empty()));
            assert_eq!(input.value().as_ref(), "🙂 broken symbol changed\n");
        });
        let requests = observed.requests.lock().unwrap().len();
        publication::replace(&panel, visual, "🙂 broken symbol retained\n");
        visual.run_until_parked();
        assert_eq!(observed.requests.lock().unwrap().len(), requests);
        let Output::Code(result) = fixture.execute(Command::ReadCode {
            worktree: fixture.session.worktree,
            query: code::Query::Hover {
                path: "main.rs".into(),
                line: 1,
                symbol: "symbol".into(),
            },
            overlay: None,
        }) else {
            panic!("shared language result expected")
        };
        assert!(matches!(result.content, code::Content::Hover(_)));
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/pid")).unwrap(),
            pid
        );
        assert_eq!(calls(&fixture, "initialize"), 1);
        editor_enabled(&fixture, true);
        ready(&input, visual);
        assert_eq!(calls(&fixture, "initialize"), 1);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(input);
        drop(provider);
        fixture.close();
    }
}

#[gpui_kit::test]
fn navigates_through_the_native_definition_action(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote, Permission::Project);
        let (panel, visual) = publication::mount(&fixture, cx);
        let (controller, _, input) = open(&panel, visual, "resource-file-main.rs");
        ready(&input, visual);
        click(visual, "document-focus");
        visual.update(|window, cx| {
            input.update(cx, |input, cx| {
                input.set_cursor_position(lsp_types::Position::new(0, 9), window, cx)
            });
            window.dispatch_action(Box::new(GoToDefinition), cx);
        });
        wait(visual, |cx| {
            controller
                .read(cx)
                .documents
                .values()
                .any(|document| document.path == "target.rs" && !document.dismissed)
        });
        assert!(calls(&fixture, "textDocument/definition") > 0);
        controller.read_with(visual, |controller, cx| {
            let (id, _) = controller.reveal.unwrap();
            let target = &controller.documents[&id];
            assert_eq!(target.path, "target.rs");
            assert_eq!(
                target.input.read(cx).cursor_position(),
                lsp_types::Position::new(0, 0)
            );
        });
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/main.rs")).unwrap(),
            "🙂 broken symbol\n"
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(input);
        fixture.close();
    }
}

fn editor_wait(
    fixture: &Fixture,
    input: &Entity<EditorState>,
    visual: &mut VisualTestContext,
    remote: bool,
    phase: &str,
    predicate: impl Fn(&EditorState) -> bool,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        wait(visual, |_| true);
        let (ready, details) = visual.update(|window, cx| {
            let input = input.read(cx);
            (
                predicate(input),
                format!(
                    "text={:?}, selection={:?}, cursor={:?}, focused={}, handler={}, menu={:?}",
                    input.value(),
                    input.selected_range(),
                    input.cursor_position(),
                    input.focus_handle(cx).is_focused(window),
                    input.has_overlay_action_handler(),
                    input.completion_menu_state()
                ),
            )
        });
        if ready {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{phase} deadline: remote={remote}, {details}, recent={:?}",
            messages(fixture)
                .into_iter()
                .rev()
                .take(5)
                .map(|message| (message["method"].clone(), message["id"].clone()))
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui_kit::test]
fn applies_completion_to_the_draft(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote, Permission::Project);
        std::fs::write(fixture.directory.path().join("project/control"),
            json!({"completions":[{"label":"completed","textEdit":{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":3}},"newText":"completed"}}]}).to_string()).unwrap();
        let (panel, visual) = publication::mount(&fixture, cx);
        let (controller, id, input) = open(&panel, visual, "resource-file-main.rs");
        ready(&input, visual);
        click(visual, "document-focus");
        wait(visual, |_| true);
        assert!(
            visual.update(|window, cx| input.focus_handle(cx).is_focused(window)),
            "editor focus: remote={remote}"
        );
        visual.simulate_keystrokes("secondary-a");
        assert_eq!(
            input.read_with(visual, |input, _| input.selected_range()),
            0.."🙂 broken symbol\n".len(),
            "editor selection: remote={remote}"
        );
        visual.simulate_input("com");
        input.read_with(visual, |input, _| {
            assert_eq!(
                input.value().as_ref(),
                "com",
                "completion draft: remote={remote}"
            );
            assert_eq!(input.selected_range(), 3..3);
            assert_eq!(input.cursor_position(), lsp_types::Position::new(0, 3));
        });
        wait(visual, |_| calls(&fixture, "textDocument/completion") > 0);
        editor_wait(
            &fixture,
            &input,
            visual,
            remote,
            "completion response",
            |input| input.completion_menu_state().open,
        );
        input.read_with(visual, |input, _| {
            let menu = input.completion_menu_state();
            assert!(input.has_overlay_action_handler());
            assert_eq!(menu.items.len(), 1, "completion items: remote={remote}");
            let item = &menu.items[0];
            assert_eq!(item.label, "completed");
            assert_eq!(item.insert_text.as_deref(), Some("completed"));
            assert_eq!(
                item.insert_text_format,
                Some(lsp_types::InsertTextFormat::PLAIN_TEXT)
            );
            assert_eq!(
                item.text_edit,
                Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
                    range: lsp_types::Range::new(
                        lsp_types::Position::new(0, 0),
                        lsp_types::Position::new(0, 3)
                    ),
                    new_text: "completed".into(),
                }))
            );
        });
        visual.update(|window, cx| window.press("enter", cx));
        editor_wait(
            &fixture,
            &input,
            visual,
            remote,
            "completion acceptance",
            |input| input.value().as_ref() == "completed",
        );
        assert!(controller.read_with(visual, |controller, cx| controller.documents[&id].dirty(cx)));
        assert!(!input.read_with(visual, |input, _| input.completion_menu_state().open));
        let settled = std::time::Instant::now() + Duration::from_millis(500);
        wait(visual, |_| std::time::Instant::now() >= settled);
        assert_eq!(
            calls(&fixture, "textDocument/didChange"),
            2,
            "native diagnostic watcher resynchronized an unchanged draft: remote={remote}"
        );
        visual.update(|window, cx| window.press("enter", cx));
        editor_wait(
            &fixture,
            &input,
            visual,
            remote,
            "ordinary Enter",
            |input| input.value().as_ref() == "completed\n",
        );
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/main.rs")).unwrap(),
            "🙂 broken symbol\n"
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(input);
        fixture.close();
    }
}

#[gpui_kit::test]
fn cancels_superseded_hover_and_rejects_stale_text(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = fixture(remote, Permission::Project);
        let observed = Arc::new(observed::Observed::new(fixture.transport.clone()));
        fixture.binding.client = Arc::new(sailry_client::Client::new(observed.clone()));
        let (panel, visual) = publication::mount(&fixture, cx);
        let (controller, id, input) = open(&panel, visual, "resource-file-main.rs");
        ready(&input, visual);
        let provider = controller.read_with(visual, |controller, _| {
            controller.documents[&id].code.clone().unwrap()
        });
        std::fs::write(
            fixture.directory.path().join("project/control"),
            "{\"hang\":true}",
        )
        .unwrap();
        let text = input.read_with(visual, |input, _| input.text().clone());
        let first =
            visual.update(|window, cx| provider.hover(&text, "🙂 broken ".len(), window, cx));
        wait(visual, |_| calls(&fixture, "textDocument/hover") > 0);
        std::fs::write(fixture.directory.path().join("project/control"), "{}").unwrap();
        let second =
            visual.update(|window, cx| provider.hover(&text, "🙂 broken s".len(), window, cx));
        let hover = visual
            .foreground_executor()
            .block_test(second)
            .unwrap()
            .unwrap();
        assert_eq!(
            hover.contents,
            lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                kind: lsp_types::MarkupKind::PlainText,
                value: "fixture signature ".into(),
            }),
            "hover contents: remote={remote}"
        );
        assert!(visual.foreground_executor().block_test(first).is_err());
        wait(visual, |_| {
            observed
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| matches!(request.command, Command::CancelCodeQuery { .. }))
        });
        let requests = observed.requests.lock().unwrap();
        let original = requests
            .iter()
            .find(|request| {
                matches!(
                    request.command,
                    Command::ReadCode {
                        query: code::Query::HoverAt { .. },
                        ..
                    }
                )
            })
            .unwrap();
        assert!(requests.iter().any(|request| matches!(request.command,Command::CancelCodeQuery {request:id} if id == original.id)));
        drop(requests);
        let stale =
            visual.update(|window, cx| provider.hover(&text, "🙂 broken ".len(), window, cx));
        visual.update(|window, cx| {
            input.update(cx, |input, cx| input.set_value("A newer draft", window, cx))
        });
        assert!(visual.foreground_executor().block_test(stale).is_err());
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/main.rs")).unwrap(),
            "🙂 broken symbol\n"
        );
        visual.update(|window, _| window.remove_window());
        drop(provider);
        drop(panel);
        drop(controller);
        drop(input);
        fixture.close();
    }
}

#[gpui_kit::test]
fn retains_an_uncertain_start_across_closing(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for canonical in [false, true] {
            let mut fixture = fixture(remote, Permission::Ask);
            let observed = Arc::new(observed::Observed::new(fixture.transport.clone()));
            fixture.binding.client = Arc::new(sailry_client::Client::new(observed.clone()));
            let (panel, visual) = publication::mount(&fixture, cx);
            let (controller, id, input) = open(&panel, visual, "resource-file-main.rs");
            cold(&controller, id, visual);
            if canonical {
                observed
                    .unknown
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            } else {
                observed
                    .lose
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
            start(visual);
            wait(visual, |cx| {
                controller.read(cx).documents[&id]
                    .code
                    .as_ref()
                    .is_some_and(|provider| provider.pending() && !*provider.starting.borrow())
            });
            let original = observed.starts().last().unwrap().clone();
            crate::feedback::tests::shown(visual);
            visual.update(|window, cx| {
                assert!(!crate::feedback::tests::summary(window, cx).is_empty());
            });
            assert!(fixture.directory.path().join("project/pid").exists());
            editor_enabled(&fixture, false);
            wait(visual, |cx| {
                controller.read(cx).documents[&id].code.is_none()
            });
            assert!(controller.read_with(visual, |controller, _| controller.pending_code()));
            assert_eq!(observed.starts(), vec![original.clone()]);
            click(visual, "document-discard");
            wait(visual, |cx| controller.read(cx).documents[&id].dismissed);
            let (_, reopened, retained) = open(&panel, visual, "resource-file-main.rs");
            assert_eq!(reopened, id);
            assert_eq!(retained.entity_id(), input.entity_id());
            assert!(controller.read_with(visual, |controller, _| {
                controller.documents[&id].code.is_none()
            }));
            editor_enabled(&fixture, true);
            wait(visual, |cx| {
                controller.read(cx).documents[&id].code.is_some()
            });
            assert_eq!(observed.starts(), vec![original.clone()]);
            observed
                .lose
                .store(false, std::sync::atomic::Ordering::SeqCst);
            observed
                .unknown
                .store(false, std::sync::atomic::Ordering::SeqCst);
            click(visual, "editor-code-start");
            wait(visual, |cx| {
                !controller.read(cx).documents[&id]
                    .code
                    .as_ref()
                    .unwrap()
                    .pending()
            });
            assert_eq!(observed.starts(), vec![original.clone(), original]);
            assert_eq!(calls(&fixture, "initialize"), 1);
            visual.update(|window, _| window.remove_window());
            drop(panel);
            drop(controller);
            drop(input);
            drop(retained);
            fixture.close();
        }
    }
}
