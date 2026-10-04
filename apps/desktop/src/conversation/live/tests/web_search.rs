use super::*;
use sailry_client::conversation::tools::State;

#[gpui::test]
fn preserves_grounding(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        let server = fixture
            .runtime
            .block_on(crate::native_provider_fixture::Server::start(
                ModelApi::Gemini,
                crate::native_provider_fixture::Reply::Grounding,
            ));
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let mut provider = snapshot.providers[0].clone();
        provider.api = ModelApi::Gemini;
        provider.endpoint = server.endpoint.clone();
        provider.models[0].web_search = true;
        provider.models[0].tools = false;
        fixture.execute(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        });
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        click(visual, "live-chat-input");
        visual.simulate_input("Find a source");
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
        assert!(visual.debug_bounds("live-search-result").is_some());
        let existing = visual.read(|cx| cx.windows());
        click(visual, "live-search-result");
        let result = visual.read(|cx| {
            cx.windows()
                .into_iter()
                .find(|window| !existing.contains(window))
                .expect("search result window expected")
        });
        result
            .update(visual, |_, window, _| window.remove_window())
            .unwrap();
        view.read_with(visual, |view, _| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            let entry = page
                .entries
                .iter()
                .find(|entry| entry.search_suggestions.is_some())
                .unwrap();
            assert_eq!(
                entry.parts,
                vec![Part::Text(crate::native_provider_fixture::ANSWER.into())]
            );
            assert_eq!(entry.citations.len(), 1);
        });
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
                        .entries
                        .iter()
                        .any(|entry| entry.search_suggestions.is_some())
                })
        });
        assert!(visual.debug_bounds("live-search-result").is_some());
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn restores_sources(cx: &mut TestAppContext) {
    fixture::init(cx);
    for api in [ModelApi::Responses, ModelApi::Anthropic] {
        for remote in [false, true] {
            let fixture = fixture::Fixture::with_tools(remote, vec![]);
            let server = fixture.runtime.block_on(async {
                if api == ModelApi::Anthropic {
                    support::Server::anthropic(support::anthropic::Reply::Search).await
                } else {
                    support::Server::web_search().await
                }
            });
            let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
                panic!("snapshot expected")
            };
            let mut provider = snapshot
                .providers
                .iter()
                .find(|provider| provider.id == fixture.session.config.provider)
                .unwrap()
                .clone();
            provider.api = api;
            provider.endpoint = server.endpoint.clone();
            provider.models[0].web_search = true;
            provider.models[0].tools = false;
            fixture.execute(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            });
            let (view, visual) =
                fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
            wait(visual, |cx| view.read(cx).connected());
            click(visual, "live-chat-input");
            visual.simulate_input("Find a source");
            visual.simulate_keystrokes("enter");
            wait(visual, |cx| {
                view.read(cx)
                    .history
                    .calls
                    .first()
                    .is_some_and(|call| call.state == State::Returned)
            });
            let turn = view.read_with(visual, |view, _| {
                let call = &view.history.calls[0];
                let page = &view.history.snapshot.as_ref().unwrap().page;
                assert_eq!(call.name, "web_search");
                let display = call.display(page).expect("package display expected");
                assert_eq!(display.label("en"), "Search web");
                assert_eq!(display.label("zh-CN"), "网页搜索");
                if api == ModelApi::Responses {
                    assert_eq!(call.result(page).unwrap()["status"], "completed");
                } else {
                    assert_eq!(
                        call.result(page).unwrap()["sources"][0]["url"],
                        support::web_search::URI
                    );
                }
                let entry = page
                    .entries
                    .iter()
                    .find(|entry| !entry.citations.is_empty())
                    .unwrap();
                assert_eq!(entry.citations.len(), 2);
                assert!(
                    entry
                        .citations
                        .iter()
                        .all(|citation| citation.uri == support::web_search::URI)
                );
                call.turn
            });
            let key = view.read_with(visual, |view, _| view.history.calls[0].source.key());
            fixture::tap(visual, &format!("live-turn-work-{turn}"));
            fixture::tap(visual, &format!("live-{turn}-tools-{key}"));
            view.read_with(visual, |view, _| {
                assert!(!view.expanded.contains_key(&(turn, format!("tools-{key}"))));
            });
            view.update(visual, |view, cx| {
                view.expanded.insert((turn, format!("tools-{key}")), true);
                cx.notify();
            });
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("live-tool-result-{turn}-{key}").into_boxed_str()
                    ))
                    .is_none()
            );
            fixture.execute(Command::RemovePlugin {
                name: "web-search".into(),
                expected_revision: 1,
            });
            visual.update(|window, _| window.remove_window());
            let (restored, visual) =
                fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
            wait(visual, |cx| {
                restored
                    .read(cx)
                    .history
                    .calls
                    .first()
                    .is_some_and(|call| call.state == State::Returned)
            });
            restored.read_with(visual, |view, _| {
                let page = &view.history.snapshot.as_ref().unwrap().page;
                assert_eq!(
                    view.history.calls[0].display(page).unwrap().label("en"),
                    "Search web"
                );
                assert_eq!(
                    page.entries
                        .iter()
                        .flat_map(|entry| &entry.citations)
                        .count(),
                    2
                );
            });
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}
