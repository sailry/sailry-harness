use super::*;
use crate::agent_fixture::plugin_tool;
use sailry_client::conversation::tools::State;
use serde_json::json;

struct Harness(Entity<View>);

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

#[gpui::test]
fn compact_file_queries(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("source #1.txt"),
            "中文 🙂\nreadable result\nreadable again",
        )
        .unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let node = runtime
            .block_on(Node::start(directory.path().join("node")))
            .unwrap();
        let controller = runtime
            .block_on(Link::controller(
                directory.path().join("controller"),
                NetworkScope::default(),
            ))
            .unwrap();
        let address = runtime
            .block_on(
                controller
                    .handle()
                    .pair(node.link().invite().unwrap().ticket()),
            )
            .unwrap();
        let transport: Arc<dyn Transport> = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Arc::new(Client::new(transport));
        let server = runtime.block_on(support::Server::tools(vec![
            (
                plugin_tool("files", "read_file"),
                json!({"path": "source #1.txt"}),
            ),
            (
                plugin_tool("files", "read_file"),
                json!({"path": "missing.txt"}),
            ),
            (
                plugin_tool("files", "search_files"),
                json!({"query": "readable", "globs": ["source #1.txt"]}),
            ),
            (plugin_tool("files", "list_directory"), json!({"path": ""})),
            (
                plugin_tool("files", "read_file"),
                json!({"path": "source #1.txt"}),
            ),
            (
                plugin_tool("files", "search_files"),
                json!({"query": "no-matching-content", "globs": ["source #1.txt"]}),
            ),
        ]));
        let mut model = provider(&server.endpoint, "tool-model");
        model.models[0].tools = true;
        runtime
            .block_on(client.execute(client.prepare(Command::SaveProvider {
                provider: model,
                expected_revision: 0,
                secret: None,
            })))
            .unwrap();
        let Output::Project(project) = runtime
            .block_on(client.execute(client.prepare(Command::RegisterProject {
                name: "Tool fixture".into(),
                path: directory.path().to_str().unwrap().into(),
            })))
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.project == Some(project.id) && tree.main)
            .unwrap()
            .id;
        let binding = Binding {
            defaults: client.clone(),
            client,
            runtime: runtime.clone(),
            project: Some(project.id),
            worktree: Some(worktree),
            host: "Fixture Node".into(),
            project_name: "Tool fixture".into(),
            branch: "main".into(),
        };
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| View::new(binding, None, window, cx));
            entity = Some(view.clone());
            let harness = cx.new(|_| Harness(view));
            Root::new(harness, window, cx)
        });
        let view = entity.unwrap();
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).configured()
        });
        click(visual, "live-chat-input");
        visual.simulate_input("Read tool fixture");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            view.read(cx)
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
        let (turn, key, source) = view.read_with(visual, |view, _| {
            assert_eq!(view.history.calls.len(), 6);
            assert!(
                view.history
                    .calls
                    .iter()
                    .all(|call| call.state == State::Returned)
            );
            let first = &view.history.calls[0];
            assert_eq!(
                first
                    .result(&view.history.snapshot.as_ref().unwrap().page)
                    .unwrap()["data"]["text"],
                "中文 🙂\nreadable result\nreadable again"
            );
            (
                first.turn,
                format!("tools-{}", first.source.key()),
                first.source.key(),
            )
        });
        let selectors = view.read_with(visual, |view, _| {
            let calls = &view.history.calls;
            vec![
                format!("live-{turn}-tool-group-{source}"),
                format!("live-{turn}-tool-group-{}", calls[2].source.key()),
                format!("live-{turn}-tools-{}", calls[3].source.key()),
                format!("live-{turn}-tool-group-{}", calls[4].source.key()),
                format!("live-{turn}-tool-group-{}", calls[5].source.key()),
            ]
        });
        let work = format!("live-turn-work-{turn}");
        let sequence = format!("live-{turn}-tool-sequence-{source}");
        let answer: &'static str = Box::leak(format!("live-turn-text-{turn}").into_boxed_str());
        fixture::tap(visual, &work);
        assert!(
            visual
                .debug_bounds(Box::leak(sequence.clone().into_boxed_str()))
                .is_some()
        );
        for selector in &selectors {
            assert!(
                visual
                    .debug_bounds(Box::leak(selector.clone().into_boxed_str()))
                    .is_none()
            );
        }
        assert!(visual.debug_bounds(answer).is_some());
        fixture::tap(visual, &sequence);
        // Tool groups expand in the conversation without a nested height cap.
        assert!(
            visual
                .debug_bounds(Box::leak(format!("{sequence}-content").into_boxed_str()))
                .is_some()
        );
        assert!(
            visual
                .debug_bounds(Box::leak(format!("{sequence}-scroll").into_boxed_str()))
                .is_none()
        );
        fixture::tap(visual, &work);
        assert!(
            visual
                .debug_bounds(Box::leak(sequence.clone().into_boxed_str()))
                .is_none()
        );
        assert!(visual.debug_bounds(answer).is_some());
        fixture::tap(visual, &work);
        for selector in &selectors {
            click(visual, Box::leak(selector.clone().into_boxed_str()));
        }
        assert_eq!(view.read_with(visual, |view, _| view.expanded.len()), 5);
        let (failed_key, diagnostic) = view.read_with(visual, |view, _| {
            let call = &view.history.calls[1];
            let result = call
                .result(&view.history.snapshot.as_ref().unwrap().page)
                .unwrap();
            let fault: sailry_protocol::Fault =
                serde_json::from_value(result["error"].clone()).unwrap();
            (
                format!("{}-{}", call.turn, call.source.key()),
                fault.message,
            )
        });
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-tool-error-{failed_key}").into_boxed_str()
                ))
                .is_some()
        );
        fixture::tap(visual, &format!("live-tool-copy-{failed_key}"));
        visual.update(|_, cx| {
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                diagnostic
            )
        });
        let opened = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let captured = opened.clone();
        let _subscription = visual.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| {
                if let Event::FileAt(_, path) = event {
                    captured.borrow_mut().push(path.clone());
                }
            })
        });
        let files = view.read_with(visual, |view, _| {
            [(0, 0), (0, 1), (2, 0), (4, 0)].map(|(call, index)| {
                format!(
                    "live-tool-file-{turn}-{}-{index}",
                    view.history.calls[call].source.key()
                )
            })
        });
        let absent = view.read_with(visual, |view, _| {
            [
                format!(
                    "live-tool-file-{turn}-{}-1",
                    view.history.calls[2].source.key()
                ),
                format!(
                    "live-tool-file-{turn}-{}-0",
                    view.history.calls[5].source.key()
                ),
            ]
        });
        for selector in absent {
            assert!(
                visual
                    .debug_bounds(Box::leak(selector.into_boxed_str()))
                    .is_none()
            );
        }
        for file in &files {
            assert!(
                visual
                    .debug_bounds(Box::leak(file.clone().into_boxed_str()))
                    .is_some()
            );
            click(visual, Box::leak(file.clone().into_boxed_str()));
        }
        assert_eq!(view.read_with(visual, |view, _| view.expanded.len()), 5);
        assert_eq!(
            *opened.borrow(),
            [
                "source #1.txt",
                "missing.txt",
                "source #1.txt",
                "source #1.txt"
            ]
        );
        click(visual, Box::leak(selectors[0].clone().into_boxed_str()));
        for file in &files[..2] {
            assert!(
                visual
                    .debug_bounds(Box::leak(file.clone().into_boxed_str()))
                    .is_none()
            );
        }
        // Old expansion preferences cannot reopen file contents, including during execution.
        visual.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.expanded.insert((turn, key.clone()), true);
                view.expanded
                    .insert((turn, format!("tool-group-{source}")), true);
                Arc::make_mut(&mut view.history.calls)[0].state = State::Running;
                cx.notify();
            })
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let sources = view.read_with(visual, |view, _| {
            view.history
                .calls
                .iter()
                .filter(|call| {
                    call.result(&view.history.snapshot.as_ref().unwrap().page)
                        .is_none_or(|result| result.get("error").is_none())
                })
                .map(|call| call.source.key())
                .collect::<Vec<_>>()
        });
        for source in sources {
            for prefix in ["live-tool-result", "live-tool-copy"] {
                assert!(
                    visual
                        .debug_bounds(Box::leak(
                            format!("{prefix}-{turn}-{source}").into_boxed_str()
                        ))
                        .is_none()
                );
            }
        }
        for selector in selectors {
            assert_eq!(
                visual
                    .debug_bounds(Box::leak(selector.into_boxed_str()))
                    .unwrap()
                    .size
                    .height,
                px(24.)
            );
        }
        assert_eq!(server.requests.lock().unwrap().len(), 7);
        visual.update(|window, _| window.remove_window());
        runtime.block_on(node.shutdown()).unwrap();
        runtime.block_on(controller.close()).unwrap();
    }
}

#[gpui::test]
fn process_result_disclosure(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![
                (
                    crate::agent_fixture::plugin_tool("browser", "browser_tabs"),
                    json!({}),
                ),
                (
                    crate::agent_fixture::plugin_tool("browser", "browser_tabs"),
                    json!({}),
                ),
                (
                    crate::agent_fixture::plugin_tool("browser", "browser_read"),
                    json!({}),
                ),
            ],
        );
        let mut subscription = fixture
            .runtime
            .block_on(fixture.binding.client.subscribe_browser())
            .unwrap();
        let client = fixture.binding.client.clone();
        let responder = fixture.runtime.spawn(async move {
            for index in 0..3 {
                let sailry_protocol::Update::BrowserCall(call) = subscription.next().await.unwrap()
                else {
                    panic!("browser call expected");
                };
                let result = if index < 2 {
                    Ok(json!({"tabs":[{"id":0,"url":"https://example.com"}]}))
                } else {
                    Err(sailry_protocol::Fault::new(
                        sailry_protocol::ErrorCode::NotFound,
                        "browser tab is empty; navigate to a URL before reading or interacting",
                    ))
                };
                client
                    .execute(client.prepare(Command::CompleteBrowser {
                        id: call.id,
                        result,
                    }))
                    .await
                    .unwrap();
            }
            subscription
        });
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        click(visual, "live-chat-input");
        visual.simulate_input("Inspect browser tabs and read the page");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .last()
                        .is_some_and(|run| run.status == Status::Completed)
                })
        });
        let turn = view.read_with(visual, |view, _| view.history.calls[0].turn);
        fixture::tap(visual, &format!("live-turn-work-{turn}"));
        let source = view.read_with(visual, |view, _| view.history.calls[0].source.key());
        fixture::tap(visual, &format!("live-{turn}-tool-sequence-{source}"));
        let selectors = view.update(visual, |view, cx| {
            assert_eq!(view.history.calls.len(), 3);
            assert!(
                view.history
                    .calls
                    .iter()
                    .all(|call| call.presentation == sailry_protocol::tool::Presentation::Summary)
            );
            let result = view.history.calls[2]
                .result(&view.history.snapshot.as_ref().unwrap().page)
                .unwrap();
            assert_eq!(result["sailry_content"]["blocks"][0]["kind"], "notice");
            assert_eq!(result["error"]["code"], "not_found");
            let mut selectors = Vec::new();
            for (index, call) in view.history.calls.iter().enumerate() {
                assert!(
                    call.result(&view.history.snapshot.as_ref().unwrap().page)
                        .is_some()
                );

                let key = format!(
                    "{}-{}",
                    if index == 0 { "tool-group" } else { "tools" },
                    call.source.key()
                );
                view.expanded.insert((call.turn, key.clone()), true);
                if index != 1 {
                    selectors.push(format!("live-{}-{key}", call.turn));
                }
            }
            cx.notify();
            selectors
        });
        for names in [
            ["browser_tabs", "browser_tabs", "browser_read"],
            ["click", "click", "plugin_advice"],
            ["bring_to_front", "bring_to_front", "get_window_state"],
            ["plugin_probe", "plugin_probe", "plugin_failure"],
            ["plugin_probe", "plugin_probe", "plugin_report"],
        ] {
            // Reuse the delivered rows to exercise presentation without executing computer input.
            view.update(visual, |view, cx| {
                for (call, name) in Arc::make_mut(&mut view.history.calls).iter_mut().zip(names) {
                    call.name = name.into();
                    call.presentation = if name == "plugin_report" { sailry_protocol::tool::Presentation::Details } else { sailry_protocol::tool::Presentation::Summary };
                }
                let call = &view.history.calls[2];
                let response = call.response.as_ref().unwrap();
                let snapshot = Arc::make_mut(view.history.snapshot.as_mut().unwrap());
                let page = Arc::make_mut(&mut snapshot.page);
                let entry = page.entries.iter_mut().find(|entry| entry.id == response.entry).unwrap();
                let sailry_protocol::conversation::Part::ToolResult { result, .. } = &mut entry.parts[response.index] else { panic!("tool result expected") };
                *result = if call.name == "plugin_advice" {
                    json!({"available":false,"error":"Provider returned HTTP 402","sailry_result":{"version":1,"diagnostics":[{"text":"Provider returned HTTP 402","error":true}]}})
                } else if call.name == "plugin_report" {
                    json!({"report":"Successful plugin output remains expandable"})
                } else {
                    json!({"error":sailry_protocol::Fault::new(sailry_protocol::ErrorCode::Unavailable,"native operation unavailable")})
                };

                view.expanded.insert((call.turn, format!("tools-{}", call.source.key())), true);
                cx.notify();
            });
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let failed_result = view.read_with(visual, |view, _| {
                let call = &view.history.calls[2];
                format!("live-tool-result-{}-{}", call.turn, call.source.key())
            });
            assert!(
                visual
                    .debug_bounds(Box::leak(failed_result.clone().into_boxed_str()))
                    .is_some()
            );
            for selector in &selectors {
                fixture::tap(visual, selector);
            }
            let results = view.read_with(visual, |view, _| {
                view.history
                    .calls
                    .iter()
                    .map(|call| format!("live-tool-result-{}-{}", call.turn, call.source.key()))
                    .collect::<Vec<_>>()
            });
            for result in results {
                assert!(
                    visual
                        .debug_bounds(Box::leak(result.into_boxed_str()))
                        .is_none()
                );
            }
            fixture::tap(visual, selectors.last().unwrap());
            assert!(
                visual
                    .debug_bounds(Box::leak(failed_result.into_boxed_str()))
                    .is_some()
            );
        }
        let subscription = fixture.runtime.block_on(responder).unwrap();
        drop(subscription);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn fault_details(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![(plugin_tool("git", "git_log"), json!({"cursor":"invalid"}))],
        );
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        fixture.start();
        wait(visual, |cx| {
            view.read(cx)
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
        let (turn, source, message) = view.read_with(visual, |view, _| {
            let call = &view.history.calls[0];
            let result = call
                .result(&view.history.snapshot.as_ref().unwrap().page)
                .unwrap();
            let fault: sailry_protocol::Fault =
                serde_json::from_value(result["error"].clone()).unwrap();
            assert!(!fault.message.is_empty());
            (call.turn, call.source.key(), fault.message)
        });
        fixture::tap(visual, &format!("live-turn-work-{turn}"));
        fixture::tap(visual, &format!("live-{turn}-tools-{source}"));
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-tool-error-{turn}-{source}").into_boxed_str()
                ))
                .is_some()
        );
        fixture::tap(visual, &format!("live-tool-copy-{turn}-{source}"));
        visual
            .update(|_, cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), message));
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
