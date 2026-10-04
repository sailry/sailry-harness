use super::*;

#[gpui::test]
fn failed_read_recovers_on_entry(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let package = fixture.install(0);
        let (panel, visual) = mount(&fixture, cx);
        *fixture.transport.view_target.lock().unwrap() = Some(package.summary.reference());
        fixture.transport.mode.store(18, Ordering::SeqCst);
        open_package(visual, &panel, "project-summary");
        wait(visual, |cx| {
            panel.read(cx).error.is_some() && !panel.read(cx).loading
        });
        assert!(panel.read_with(visual, |panel, _| panel.mounted.is_none()));
        assert!(visual.debug_bounds("plugin-view-retry").is_none());
        assert_eq!(fixture.transport.views.lock().unwrap().len(), 1);
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.enter(package.summary.reference(), window, cx)
            })
        });
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        assert!(panel.read_with(visual, |panel, _| panel.error.is_none()));
        assert_eq!(fixture.transport.views.lock().unwrap().len(), 2);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn waits_for_read_capacity(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let package = fixture.install(0);
        let (panel, visual) = mount(&fixture, cx);
        *fixture.transport.view_target.lock().unwrap() = Some(package.summary.reference());
        fixture.transport.mode.store(15, Ordering::SeqCst);
        open_package(visual, &panel, "project-summary");
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        let requests = fixture.transport.views.lock().unwrap();
        assert!(requests.len() >= 2);
        assert!(requests.iter().all(|request| *request == requests[0]));
        assert!(!requests[0].command.durable());
        assert!(fixture.transport.entered.is_cancelled());
        assert_eq!(fixture.transport.mode.load(Ordering::SeqCst), 0);
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn closing_cancels_capacity_wait(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let package = fixture.install(0);
        let (panel, visual) = mount(&fixture, cx);
        *fixture.transport.view_target.lock().unwrap() = Some(package.summary.reference());
        fixture.transport.mode.store(16, Ordering::SeqCst);
        open_package(visual, &panel, "project-summary");
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        panel.update(visual, |panel, cx| panel.back(cx));
        let attempts = fixture.transport.views.lock().unwrap().len();
        assert!(attempts > 0);
        fixture.runtime.block_on(async {
            tokio::time::sleep(Duration::from_millis(150)).await;
        });
        visual.run_until_parked();
        assert_eq!(fixture.transport.views.lock().unwrap().len(), attempts);
        panel.read_with(visual, |panel, _| {
            assert!(!panel.loading);
            assert!(panel.selected.is_none());
            assert!(panel.mounted.is_none());
            assert!(panel.error.is_none());
        });
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
