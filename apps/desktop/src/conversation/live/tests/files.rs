use super::*;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::resources::SideResource;
use crate::{backend::Services, shell::Shell};
use sailry_protocol::conversation::assets;

#[gpui::test]
fn markdown_cards_open_locally(cx: &mut TestAppContext) {
    cards(cx, false);
}

#[gpui::test]
fn markdown_cards_open_remotely(cx: &mut TestAppContext) {
    cards(cx, true);
}

fn cards(cx: &mut TestAppContext, remote: bool) {
    fixture::init(cx);
    cx.update(crate::shell::init);
    for (path, label) in [
        ("报告.docx", "报告"),
        ("budget.XLSX", "Budget"),
        ("slides.pptx", "Slides"),
        ("report.pdf", "Report"),
        ("preview.html", "Preview"),
        ("report.md", "Markdown report"),
        ("notes.txt", "Notes"),
        ("data.json", "Data"),
        ("stats.py", "Script"),
        ("execution.log", "Log"),
        ("README", "Readme"),
    ] {
        let fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(support::Server::markdown(format!(
                "Before\n\n[{label}]({path})\n\nAfter"
            )))
        });
        std::fs::write(
            fixture.directory.path().join("project").join(path),
            "file content fixture",
        )
        .unwrap();
        cx.update(|cx| {
            cx.set_global(Services {
                profile: fixture.node.profile().to_path_buf(),
                runtime: fixture.runtime.clone(),
                local: fixture.node.local(),
                link: fixture.node.link(),
                relay_enabled: false,
            });
            cx.set_global(crate::preferences::Preferences::default());
        });
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| Shell::new(window, cx));
            shell.update(cx, |shell, cx| {
                let live = shell.live.as_mut().unwrap();
                live.override_transport(fixture.transport.clone());
                live.select(fixture.node.id(), cx);
                shell.reveal_session(fixture.session.clone(), window, cx);
            });
            owner = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = owner.unwrap();
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1440.), px(900.)));
        wait(visual, |cx| {
            shell
                .read(cx)
                .current_chat()
                .is_some_and(|chat| chat.read(cx).connected())
        });
        let chat = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
        fixture.start();
        wait(visual, |cx| {
            chat.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .any(|run| run.status == Status::Completed)
                })
        });
        let requests = fixture.server.requests.lock().unwrap();
        let request = requests
            .iter()
            .find(|request| {
                request["tools"]
                    .as_array()
                    .is_some_and(|tools| !tools.is_empty())
            })
            .expect("task request");
        let prompt = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            // Pinned ADK prepends instructions as synthetic user content.
            .take_while(|message| message["role"] != "assistant")
            .map(|message| message["content"].to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            prompt.contains("Files and previews"),
            "task instruction: {prompt}"
        );
        assert!(prompt.contains("![Description](images/result.png)"));
        assert!(prompt.contains("[Preview](output/preview.html)"));
        assert!(prompt.contains("opens its interactive preview in the side panel"));
        assert!(
            !request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["function"]["name"] == "publish_file")
        );
        drop(requests);
        let received = std::rc::Rc::new(std::cell::RefCell::new(None));
        let observed = received.clone();
        let subscription = visual.update(|_, cx| {
            cx.subscribe(&chat, move |_, event, _| {
                if let Event::Artifact(worktree, file) = event {
                    *observed.borrow_mut() = Some((*worktree, file.clone()));
                }
            })
        });
        let title = visual.debug_bounds("artifact-file-title").unwrap();
        let open = visual.debug_bounds("artifact-file-open").unwrap();
        let more = visual.debug_bounds("artifact-file-more").unwrap();
        assert!(title.right() <= open.left());
        assert!(open.right() <= more.left());
        fixture::tap(visual, "artifact-file-title");
        assert!(received.borrow().is_none(), "the title is not an action");
        fixture::tap(visual, "artifact-file-open");
        let (worktree, file) = received.borrow().clone().unwrap();
        assert_eq!(worktree, fixture.session.worktree);
        assert_eq!(file.path, path);
        assert_eq!(file.label(), label);
        assert!(visual.debug_bounds("artifact-inline").is_none());
        *received.borrow_mut() = None;
        fixture::tap(visual, "artifact-file-more");
        visual.simulate_keystrokes("down enter");
        assert!(
            received.borrow().is_some(),
            "the menu can also open a preview"
        );
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if path == "报告.docx" {
            select(&shell, visual, 2, "Selected paragraph");
        }
        fixture::tap(visual, "live-chat-input");
        visual.simulate_input("Keep this draft");
        fixture::tap(visual, "artifact-file-more");
        visual.simulate_keystrokes("down down down down enter");
        chat.read_with(visual, |chat, cx| {
                assert!(chat.input.read(cx).value().starts_with("Keep this draft"));
                assert!(chat.input.read(cx).value().contains(path));
                assert!(chat.active_references(cx).iter().any(|reference| matches!(&reference.target, sailry_protocol::conversation::reference::Target::File(found) if found == path)));
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                if path == "报告.docx" {
                    let draft = chat.input.read(cx).value();
                    assert!(draft.contains("Selected paragraph"));
                    assert!(draft.contains(rust_i18n::t!("artifact_selection_context", page = 2).as_ref()));
                }
            });
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if path == "报告.docx" {
            let mut foreign = file.clone();
            foreign.path = "foreign.docx".into();
            foreign.name = None;
            visual.update(|window, cx| {
                chat.update(cx, |chat, cx| {
                    chat.input.update(cx, |input, cx| {
                        input.set_value("Keep this draft", window, cx)
                    });
                    cx.emit(Event::Artifact(worktree, foreign));
                });
            });
            select(&shell, visual, 7, "Foreign paragraph");
            fixture::tap(visual, "artifact-file-more");
            visual.simulate_keystrokes("down down down down enter");
            chat.read_with(visual, |chat, cx| {
                    let draft = chat.input.read(cx).value();
                    assert!(draft.starts_with("Keep this draft"));
                    assert!(draft.contains(path));
                    assert!(!draft.contains("Selected paragraph"));
                    assert!(!draft.contains("Foreign paragraph"));
                    assert!(!draft.contains("foreign.docx"));
                    assert!(!draft.contains(rust_i18n::t!("artifact_selection_context", page = 7).as_ref()));
                    assert!(chat.active_references(cx).iter().any(|reference| matches!(&reference.target, sailry_protocol::conversation::reference::Target::File(found) if found == path)));
                });
        }
        let Output::ConversationAssets(page) = fixture.execute(Command::ListConversationAssets {
            session: fixture.session.id,
            query: assets::Query {
                kind: Some(assets::Kind::Artifact),
                before: None,
                limit: 20,
            },
        }) else {
            panic!("assets expected");
        };
        assert!(
            page.groups.iter().flat_map(|group| &group.items).any(
                |target| matches!(target, assets::Target::File { path: found } if found == path)
            )
        );
        drop(subscription);
        visual.update(|window, _| window.remove_window());
        drop(chat);
        drop(shell);
        cx.update(|cx| cx.remove_global::<Services>());
        fixture.close();
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn select(shell: &Entity<Shell>, visual: &mut VisualTestContext, page: u32, text: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        wait(visual, |_| true);
        if visual.update(|_, cx| {
            let preview = match &shell.read(cx).side_resource {
                Some(SideResource::Artifact(panel)) => panel.read(cx).file_preview(cx),
                _ => None,
            };
            preview.is_some_and(|preview| {
                crate::content::files::html::set_edit_context(&preview, page, text, cx)
            })
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "file preview selection deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}
