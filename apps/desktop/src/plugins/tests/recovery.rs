use super::*;

#[gpui::test]
fn retains_change_notice(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        fixture.install(0);
        let (panel, visual) = mount(&fixture, cx);
        open_package(visual, &panel, "project-summary");
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("Watching file changes")
        });
        fixture.transport.mode.store(3, Ordering::SeqCst);
        control(visual, 130., 64.);
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        std::fs::write(
            fixture.directory.path().join("project/another.txt"),
            "Concurrent external change",
        )
        .unwrap();
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("summary-watch-changed")
        });
        fixture.transport.release.cancel();
        toast(visual, "Report saved");
        assert!(panel.read_with(visual, |_, cx| {
            !snapshot(&panel, cx).contains("summary-status-saved")
        }));
        assert!(panel.read_with(visual, |_, cx| {
            snapshot(&panel, cx).contains("summary-watch-changed")
        }));
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn survives_view_close(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        fixture.install(0);
        let (panel, visual) = mount(&fixture, cx);
        open_package(visual, &panel, "project-summary");
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        let cache = panel.read_with(visual, |panel, _| {
            panel.mounted.as_ref().unwrap().cache_root().to_path_buf()
        });
        fixture.transport.mode.store(3, Ordering::SeqCst);
        control(visual, 130., 64.);
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        assert!(visual.debug_bounds("plugin-back").is_none());
        visual.update(|_, cx| panel.update(cx, |panel, cx| panel.back(cx)));
        wait(visual, |_| {
            !cache.exists() && fixture.transport.files.load(Ordering::SeqCst) == 0
        });
        let report = fixture.directory.path().join("project/project-summary.md");
        wait(visual, |_| report.exists());
        let original = fixture
            .transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .find(|request| matches!(request.command, Command::WriteFile { .. }))
            .unwrap()
            .clone();
        // File creation precedes the authoritative completion; do not race publication.
        wait(visual, |_| {
            matches!(
                fixture.runtime.block_on(fixture.binding.client.outcome(&original)).unwrap(),
                sailry_protocol::RequestOutcome::Completed(result)
                    if matches!(*result, Ok(sailry_protocol::Output::FileWritten(_)))
            )
        });
        std::fs::write(&report, "Keep later content").unwrap();
        fixture.transport.release.cancel();
        let result = fixture
            .runtime
            .block_on(fixture.binding.client.execute(original))
            .unwrap();
        assert!(matches!(result, sailry_protocol::Output::FileWritten(_)));
        assert_eq!(
            std::fs::read_to_string(&report).unwrap(),
            "Keep later content"
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn recovers_lost_receipt(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        fixture.install(0);
        let (panel, visual) = mount(&fixture, cx);
        open_package(visual, &panel, "project-summary");
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        fixture.transport.mode.store(1, Ordering::SeqCst);
        control(visual, 130., 64.);
        toast(visual, "Result unconfirmed — check the original request");
        assert!(panel.read_with(visual, |_, cx| {
            !snapshot(&panel, cx).contains("summary-status-unknown")
        }));
        let report = fixture.directory.path().join("project/project-summary.md");
        assert!(
            std::fs::read_to_string(&report)
                .unwrap()
                .contains("notes.txt")
        );
        std::fs::write(&report, "External replacement 中文 🙂").unwrap();
        visual.update(|window, cx| window.clear_notifications(cx));
        control(visual, 130., 64.);
        toast(visual, "Report saved");
        assert!(panel.read_with(visual, |_, cx| {
            !snapshot(&panel, cx).contains("summary-status-saved")
        }));
        assert_eq!(
            std::fs::read_to_string(&report).unwrap(),
            "External replacement 中文 🙂"
        );
        let requests = fixture.transport.requests.lock().unwrap();
        let writes: Vec<_> = requests
            .iter()
            .filter(|request| matches!(request.command, Command::WriteFile { .. }))
            .collect();
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0], writes[1]);
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn requires_refresh(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        fixture.install(0);
        let (panel, visual) = mount(&fixture, cx);
        open_package(visual, &panel, "project-summary");
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        let report = fixture.directory.path().join("project/project-summary.md");
        std::fs::write(&report, "External report").unwrap();
        control(visual, 130., 64.);
        toast(visual, "The report changed — refresh before saving");
        assert!(panel.read_with(visual, |_, cx| {
            !snapshot(&panel, cx).contains("summary-status-conflict")
        }));
        assert_eq!(std::fs::read_to_string(&report).unwrap(), "External report");
        visual.update(|window, cx| window.clear_notifications(cx));
        control(visual, 40., 64.);
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("summary-status-ready")
        });
        control(visual, 24., 24.);
        wait(visual, |cx| snapshot(&panel, cx).contains("No changes"));
        control(visual, 130., 64.);
        toast(visual, "Report saved");
        assert!(panel.read_with(visual, |_, cx| {
            !snapshot(&panel, cx).contains("summary-status-saved")
        }));
        assert!(
            std::fs::read_to_string(&report)
                .unwrap()
                .contains("No changes")
        );
        let large = "Complete content\n".repeat(10000);
        std::fs::write(&report, &large).unwrap();
        visual.update(|window, cx| window.clear_notifications(cx));
        control(visual, 40., 64.);
        toast(visual, "The existing report is too large to replace here");
        assert!(panel.read_with(visual, |_, cx| {
            !snapshot(&panel, cx).contains("summary-status-tooLarge")
        }));
        let count = fixture.transport.requests.lock().unwrap().len();
        control(visual, 130., 64.);
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), count);
        assert_eq!(std::fs::read_to_string(&report).unwrap(), large);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn recovers_failed_loads(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        fixture.install(0);
        let (panel, visual) = mount(&fixture, cx);
        fixture.transport.mode.store(2, Ordering::SeqCst);
        open_package(visual, &panel, "project-summary");
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        assert!(visual.debug_bounds("plugin-back").is_none());
        visual.update(|_, cx| panel.update(cx, |panel, cx| panel.back(cx)));
        fixture.transport.release.cancel();
        wait(visual, |cx| {
            !panel.read(cx).loading && panel.read(cx).selected.is_none()
        });
        assert!(panel.read_with(visual, |panel, _| panel.mounted.is_none()));
        std::fs::write(
            fixture
                .directory
                .path()
                .join("project/package/dev.sailry.platform/desktop/main.js"),
            "export default",
        )
        .unwrap();
        let updated = fixture.install(1);
        wait(visual, |cx| {
            panel
                .read(cx)
                .metadata
                .read(cx)
                .entries
                .values()
                .any(|info| info.summary == updated.summary)
        });
        open_package(visual, &panel, "project-summary");
        wait(visual, |cx| {
            panel.read(cx).error == Some("plugins_view_load_failed")
        });
        assert!(panel.read_with(visual, |panel, _| panel.mounted.is_none()));
        assert_eq!(fixture.transport.files.load(Ordering::SeqCst), 0);
        fixture.package();
        let updated = fixture.install(2);
        assert!(visual.debug_bounds("plugin-back").is_none());
        visual.update(|_, cx| panel.update(cx, |panel, cx| panel.back(cx)));
        wait(visual, |cx| {
            panel
                .read(cx)
                .metadata
                .read(cx)
                .entries
                .values()
                .any(|info| info.summary == updated.summary)
        });
        open_package(visual, &panel, "project-summary");
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
