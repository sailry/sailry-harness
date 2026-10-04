use super::*;

#[gpui::test]
fn renders_partial_deltas(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let gates: Vec<_> = (0..3)
            .map(|_| Arc::new(tokio::sync::Notify::new()))
            .collect();
        let fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(support::Server::staged(gates.clone()))
        });
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        fixture.start();
        for (index, expected) in ["First", "First second", "First second third"]
            .iter()
            .enumerate()
        {
            wait(visual, |cx| {
                view.read(cx)
                    .history
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| {
                        snapshot.drafts.iter().any(|draft| {
                            draft
                                .parts
                                .iter()
                                .any(|part| matches!(part, Part::Text(text) if text == expected))
                        })
                    })
            });
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                visual.run_until_parked();
                let texts = view.read_with(visual, |view, _| {
                    view.texts.borrow().values().cloned().collect::<Vec<_>>()
                });
                let rendered = texts.iter().any(|state| {
                    state.update(visual, |state, cx| {
                        state.select_all(cx);
                        state.selected_text().contains(expected)
                    })
                });
                if rendered {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "streamed Markdown render deadline"
                );
                visual.update(|window, cx| {
                    window.draw(cx).clear(cx);
                });
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(view.read_with(visual, |view, _| {
                view.history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .runs
                    .iter()
                    .all(|run| run.status != Status::Completed)
            }));
            gates[index].notify_one();
        }
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
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn commentary_context(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(support::Server::phases())
        });
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!()
        };
        let mut provider = snapshot
            .providers
            .into_iter()
            .find(|provider| provider.id == fixture.session.config.provider)
            .unwrap();
        provider.api = ModelApi::Responses;
        fixture.execute(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        });
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        fixture.start();
        wait(visual, |cx| {
            let Some(snapshot) = &view.read(cx).history.snapshot else {
                return false;
            };
            for part in snapshot
                .page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .chain(snapshot.drafts.iter().flat_map(|draft| &draft.parts))
            {
                assert!(
                    !matches!(part, Part::Text(text) if text.contains("Creating JSON fixture") || text == "**")
                );
                assert!(
                    !matches!(part, Part::Resource(value) if value["server_tool_call"]["phase"] == "commentary")
                );
            }
            snapshot
                .page
                .runs
                .iter()
                .any(|run| run.status == Status::Completed)
        });
        view.read_with(visual, |view, _| {
            let snapshot = view.history.snapshot.as_ref().unwrap();
            assert!(
                snapshot
                    .page
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(
                        |part| matches!(part, Part::Text(text) if text == support::phases::ANSWER)
                    )
            );
            assert_eq!(view.history.calls.len(), 1);
        });
        let requests = fixture.server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let message = requests[1]["input"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["phase"] == "commentary")
            .expect("commentary must remain in model history");
        assert_eq!(message["content"][0]["text"], support::phases::COMMENTARY);
        drop(requests);
        visual.update(|window, _| window.remove_window());
        let (restored, visual) =
            fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            restored
                .read(cx)
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
        restored.read_with(visual, |view, _| {
            let snapshot = view.history.snapshot.as_ref().unwrap();
            assert!(snapshot.page.entries.iter().flat_map(|entry| &entry.parts).all(|part| !matches!(part, Part::Text(text) if text.contains("Creating JSON fixture"))));
        });
        wait(visual, |cx| restored.read(cx).connected());
        click(visual, "live-chat-input");
        visual.simulate_input("Continue with the same context");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            restored
                .read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .filter(|run| run.status == Status::Completed)
                        .count()
                        == 2
                })
        });
        let requests = fixture.server.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(
            requests[2]["input"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["phase"] == "commentary")
        );
        drop(requests);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
