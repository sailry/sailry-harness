use super::*;

#[gpui::test]
fn restores_collapsed_failures(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        for (failures, code, expected) in [
            (2, 503, Status::Completed),
            (1, 400, Status::Failed),
            (usize::MAX, 503, Status::Failed),
        ] {
            let fixture = fixture::Fixture::with_server(remote, |runtime| {
                runtime.block_on(support::Server::http(failures, code))
            });
            let (view, visual) =
                fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
            wait(visual, |cx| view.read(cx).connected());
            fixture.start();
            wait(visual, |cx| {
                view.read(cx)
                    .history
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| s.page.runs.first().is_some_and(|r| r.status == expected))
            });
            let (turn, count) = view.read_with(visual, |view, _| {
                let page = &view.history.snapshot.as_ref().unwrap().page;
                let count = page
                    .entries
                    .iter()
                    .flat_map(|e| &e.parts)
                    .filter(|p| matches!(p,Part::Resource(v) if v["type"] == "model_retry"))
                    .count();
                (page.runs[0].turn, count)
            });
            assert_eq!(count, if code == 503 { failures.min(5) } else { 0 });
            assert_eq!(
                fixture.task_requests(),
                if code == 503 { failures.min(5) + 1 } else { 1 }
            );
            let selector: &'static str = Box::leak(
                if code == 503 {
                    format!("live-model-retry-{turn}-{}", failures.min(5))
                } else {
                    format!("live-turn-error-{turn}")
                }
                .into_boxed_str(),
            );
            if code == 503 {
                fixture::tap(visual, &format!("live-turn-work-{turn}"));
                assert!(visual.debug_bounds(selector).is_some());
            } else {
                assert!(visual.debug_bounds(selector).is_some());
            }
            let reason = Box::leak(format!("live-turn-error-reason-{turn}").into_boxed_str());
            assert!(visual.debug_bounds(reason).is_none());
            if expected == Status::Failed {
                fixture::tap(visual, &format!("live-turn-error-{turn}"));
                assert!(visual.debug_bounds(reason).is_some());
            }
            if count > 1 {
                let first = Box::leak(format!("live-model-retry-{turn}-1").into_boxed_str());
                assert!(
                    visual.debug_bounds(first).is_none(),
                    "superseded retry counter must not remain visible"
                );
            }
            let running =
                Box::leak(format!("live-turn-status-{turn}-chat_running").into_boxed_str());
            assert!(visual.debug_bounds(running).is_none());
            visual.update(|window, _| window.remove_window());
            let (reopened, visual) =
                fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
            wait(visual, |cx| {
                reopened
                    .read(cx)
                    .history
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| !s.page.runs.is_empty())
            });
            if code == 503 {
                fixture::tap(visual, &format!("live-turn-work-{turn}"));
                assert!(visual.debug_bounds(selector).is_some());
            } else {
                assert!(visual.debug_bounds(selector).is_some());
            }
            assert!(visual.debug_bounds(reason).is_none());
            assert_eq!(
                fixture.task_requests(),
                if code == 503 { failures.min(5) + 1 } else { 1 }
            );
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}

#[gpui::test]
fn cancels_backoff(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_server(remote, |runtime| {
            runtime.block_on(support::Server::http(usize::MAX, 503))
        });
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        fixture.start();
        wait(visual, |cx| {
            view.read(cx).history.snapshot.as_ref().is_some_and(|s| {
                s.page.entries.iter().any(|e| {
                    e.parts
                        .iter()
                        .any(|p| matches!(p, Part::Resource(v) if v["type"] == "model_retry"))
                })
            })
        });
        let turn = view.read_with(visual, |view, _| {
            view.history.snapshot.as_ref().unwrap().page.runs[0].turn
        });
        fixture.execute(Command::StopTurn { turn });
        wait(visual, |cx| {
            view.read(cx).history.snapshot.as_ref().unwrap().page.runs[0].status
                == Status::Cancelled
        });
        let requests = fixture.task_requests();
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(fixture.task_requests(), requests);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
