use super::*;
use sailry_protocol::conversation::assets;

#[gpui::test]
fn markdown_cards_open_locally_and_remotely(cx: &mut TestAppContext) {
    fixture::init(cx);
    for (path, label) in [
        ("报告.docx", "报告"),
        ("budget.XLSX", "Budget"),
        ("slides.pptx", "Slides"),
        ("report.pdf", "Report"),
        ("preview.html", "Preview"),
    ] {
        for remote in [false, true] {
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
            let (chat, visual) =
                fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
            wait(visual, |cx| chat.read(cx).connected());
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
            let Output::ConversationAssets(page) =
                fixture.execute(Command::ListConversationAssets {
                    session: fixture.session.id,
                    query: assets::Query {
                        kind: Some(assets::Kind::Artifact),
                        before: None,
                        limit: 20,
                    },
                })
            else {
                panic!("assets expected");
            };
            assert!(page.groups.iter().flat_map(|group| &group.items).any(
                |target| matches!(target, assets::Target::File { path: found } if found == path)
            ));
            drop(subscription);
            fixture.close();
        }
    }
}
