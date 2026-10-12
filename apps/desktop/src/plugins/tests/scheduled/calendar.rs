use super::*;
use chrono::{Timelike, Utc};
use gpui_kit::test::{TestQueryExt as _, TestWindowExt as _};

fn tap(visual: &mut VisualTestContext, id: impl Into<ElementId>) {
    let id = id.into();
    visual.update(|window, cx| {
        window.render_frame(cx);
        window.find(id.clone());
        window.click(id, cx);
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.render_frame(cx));
}

fn field(visual: &mut VisualTestContext, label: &str) {
    let id = visual.update(|window, cx| {
        window.render_frame(cx);
        window.find_by_label(label).path().last().unwrap().clone()
    });
    tap(visual, id);
}

#[gpui::test]
fn preserves_calendar_edits_and_reports_invalid_zones(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = install(&fixture);
        let (panel, visual) = mount(&fixture, &package, cx);
        tap(visual, "task-create");
        wait(visual, |cx| snapshot(&panel, cx).contains("task-editor"));
        field(visual, "Name");
        visual.simulate_input("Calendar review");
        field(visual, "Task");
        visual.simulate_input("Keep this task");
        let model = format!("{}/fixture", fixture.session.config.provider);
        wait(visual, |cx| snapshot(&panel, cx).contains(&model));
        tap(visual, "task-model");
        tap(
            visual,
            (SharedString::from(format!("model-{model}")), 0usize),
        );
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("task-config-high")
        });
        visual.update(|window, cx| {
            window.render_frame(cx);
            let mut timing = window.within("task-timing");
            let id = timing
                .find_by_label("Weekly")
                .path()
                .last()
                .unwrap()
                .clone();
            timing.click(id, cx);
        });
        wait(visual, |cx| snapshot(&panel, cx).contains("task-weekday-7"));
        visual.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.try_find_by_label("Interval (seconds)").is_none());
        });
        tap(visual, "task-save");
        toast(visual, "Enter a timezone");
        assert!(items(&fixture, &package).is_empty());
        let scheduled = Utc::now() + chrono::Duration::hours(2);
        field(visual, "Time");
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input(&format!(
            "{:02}:{:02}",
            scheduled.hour(),
            scheduled.minute()
        ));
        tap(visual, "task-weekday-6");
        tap(visual, "task-weekday-7");
        field(visual, "Timezone");
        visual.simulate_input("Mars/Olympus");
        tap(visual, "task-save");
        toast(visual, "Action failed");
        assert!(items(&fixture, &package).is_empty());
        visual.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.try_find_by_label("Time").is_some());
        });
        field(visual, "Timezone");
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input("UTC");
        tap(visual, "task-save");
        wait(visual, |cx| {
            !snapshot(&panel, cx).contains("task-editor")
                && snapshot(&panel, cx).contains("Calendar review")
        });
        let saved = items(&fixture, &package).remove(0);
        let timing = json!({"kind":"weekly","data":{"timezone":"UTC","weekdays":[1,2,3,4,5,6,7],"hour":scheduled.hour(),"minute":scheduled.minute()}});
        assert_eq!(saved["timing"], timing);
        let id = saved["id"].as_str().unwrap();
        tap(visual, SharedString::from(format!("task-edit-{id}")));
        wait(visual, |cx| snapshot(&panel, cx).contains("task-editor"));
        field(visual, "Name");
        visual.simulate_keystrokes("secondary-a");
        visual.simulate_input("Renamed calendar review");
        tap(visual, "task-save");
        wait(visual, |cx| !snapshot(&panel, cx).contains("task-editor"));
        let edited = items(&fixture, &package).remove(0);
        assert_eq!(edited["name"], "Renamed calendar review");
        for field in ["timing", "next_ms", "project", "worktree", "config"] {
            assert_eq!(edited[field], saved[field]);
        }
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
