//! Contributions stay hidden until their plugin state arrives for the current session.
use super::*;

const STATUS: &str = "plugin-control-task-notes-refresh";
const CONTEXT: &str = "plugin-control-task-notes-note";

fn chips(fixture: &mut Fixture) {
    install_with(fixture, |manifest| {
        let ui = manifest["extensions"]["dev.sailry.platform"]["ui"]
            .as_array_mut()
            .unwrap();
        for entry in ui {
            match entry["id"].as_str() {
                Some("refresh") => {
                    entry["slot"] = json!("status");
                    entry["overflow"] = json!("auto");
                }
                // Icon-only toolbar controls keep the toolbar on one line.
                Some("focus" | "level" | "actions") => entry["overflow"] = json!("menu"),
                _ => {}
            }
        }
    });
}

fn draw(visual: &mut VisualTestContext) {
    visual
        .executor()
        .advance_clock(std::time::Duration::from_millis(10));
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn top(visual: &mut VisualTestContext) -> gpui::Pixels {
    visual.debug_bounds("composer-surface").unwrap().top()
}

fn hidden(visual: &mut VisualTestContext) {
    for selector in [STATUS, CONTEXT] {
        assert!(visual.debug_bounds(selector).is_none(), "{selector} drawn");
    }
}

/// Draws frames until both chips resolve, asserting the composer never moves.
fn resolve(visual: &mut VisualTestContext, expected: gpui::Pixels, unresolved: bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        draw(visual);
        assert_eq!(top(visual), expected);
        let shown = [STATUS, CONTEXT].map(|selector| visual.debug_bounds(selector).is_some());
        if shown == [true, true] {
            return;
        }
        if unresolved {
            assert_eq!(shown, [false, false], "partial or placeholder frame");
        }
        assert!(
            std::time::Instant::now() < deadline,
            "contribution deadline {shown:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[gpui::test]
fn reserves_row_before_state_arrives(cx: &mut TestAppContext) {
    init(cx);
    cx.update(crate::plugins::init);
    for remote in [false, true] {
        for welcome in [false, true] {
            let bare = Fixture::with_tools(remote, vec![]);
            let session = (!welcome).then(|| bare.session.clone());
            let (_, visual) = crate::conversation::live::tests::fixture::open_session(
                cx,
                bare.binding.clone(),
                session,
            );
            resize(visual, 1400.);
            draw(visual);
            let expected = top(visual);
            hidden(visual);

            let mut fixture = Fixture::with_tools(remote, vec![]);
            chips(&mut fixture);
            let session = (!welcome).then(|| fixture.session.clone());
            let (_, visual) = crate::conversation::live::tests::fixture::open_session(
                cx,
                fixture.binding.clone(),
                session,
            );
            resize(visual, 1400.);
            draw(visual);
            assert_eq!(top(visual), expected, "unresolved row shifts the composer");
            hidden(visual);
            resolve(visual, expected, true);
        }
    }
}

#[gpui::test]
fn never_draws_placeholders_while_switching(cx: &mut TestAppContext) {
    init(cx);
    cx.update(crate::plugins::init);
    for remote in [false, true] {
        let mut fixture = Fixture::with_tools(remote, vec![]);
        chips(&mut fixture);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        resize(visual, 1400.);
        draw(visual);
        let expected = top(visual);
        resolve(visual, expected, false);
        for session in [None, Some(fixture.session.clone())] {
            visual.update(|window, cx| {
                view.update(cx, |view, cx| {
                    let binding = view.binding.clone();
                    *view =
                        View::embedded(binding, session, ComposerOptions::default(), window, cx);
                    cx.notify();
                })
            });
            draw(visual);
            hidden(visual);
            // Welcome and conversation layouts are not required to share a top.
            let expected = top(visual);
            resolve(visual, expected, true);
        }
    }
}
