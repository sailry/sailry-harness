use super::*;
use core::prelude::v1::test;

#[test]
fn orders_explicit_levels_and_budgets() {
    let mut choices = [Effort::High, Effort::Disabled, Effort::Max, Effort::Low];
    choices.sort_by_key(|effort| order(*effort));
    assert_eq!(
        choices,
        [Effort::Disabled, Effort::Low, Effort::High, Effort::Max]
    );
    assert!(order(Effort::Budget(1024)) < order(Effort::Budget(8192)));
}

use crate::conversation::live::tests::{
    fixture::{self, Fixture, init, tap},
    wait,
};

#[gpui::test]
fn selects_models_and_levels(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let mut provider = snapshot.providers[0].clone();
        let mut alternate = provider.models[0].clone();
        alternate.id = "alternate-model".into();
        provider.models.push(alternate);
        let mut unsupported = provider.models[0].clone();
        unsupported.id = "no-reasoning".into();
        unsupported.reasoning = false;
        unsupported.efforts = vec![Effort::Default];
        unsupported.default_effort = Effort::Default;
        provider.models.push(unsupported);
        fixture.execute(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        });
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).configured()
        });
        tap(visual, "live-chat-model");
        let compact = visual.debug_bounds("model-controls").unwrap();
        assert!(visual.debug_bounds("model-effort-flow").is_some());
        let track = visual.debug_bounds("model-effort-track").unwrap();
        let position = point(track.left() + px(1.), track.center().y);
        let thumb = visual.debug_bounds("model-effort-thumb").unwrap();
        assert!(thumb.left() >= compact.left() && thumb.right() <= compact.right());
        assert_eq!(thumb.right(), track.right());
        assert_eq!(
            visual.debug_bounds("model-effort-fill").unwrap().right(),
            thumb.center().x
        );
        visual.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: thumb.center(),
            click_count: 1,
            ..Default::default()
        });
        for position in [thumb.center() - point(px(8.), px(0.)), position] {
            visual.simulate_mouse_move(position, Some(MouseButton::Left), Modifiers::default());
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
        }
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            view.read_with(visual, |view, _| view.session.as_ref().unwrap().revision),
            1
        );
        assert!(visual.debug_bounds("model-effort-flow").is_none());
        visual.simulate_event(MouseUpEvent {
            button: MouseButton::Left,
            position,
            click_count: 1,
            ..Default::default()
        });
        wait(visual, |cx| {
            !view.read(cx).busy()
                && view.read(cx).session.as_ref().unwrap().config.effort == Effort::Low
        });
        let thumb = visual.debug_bounds("model-effort-thumb").unwrap();
        assert!(thumb.left() >= compact.left() && thumb.right() <= compact.right());
        assert_eq!(thumb.left(), track.left());
        assert!(visual.debug_bounds("model-effort-fill").is_none());
        assert_eq!(
            view.read_with(visual, |view, _| view.session.as_ref().unwrap().revision),
            2
        );
        assert!(visual.debug_bounds("model-controls").is_some());
        tap(visual, "model-controls-reset");
        wait(visual, |cx| {
            !view.read(cx).busy()
                && view.read(cx).session.as_ref().unwrap().config.effort == Effort::High
        });
        visual.simulate_keystrokes("home");
        wait(visual, |cx| {
            !view.read(cx).busy()
                && view.read(cx).session.as_ref().unwrap().config.effort == Effort::Low
        });
        visual.simulate_keystrokes("right");
        wait(visual, |cx| {
            !view.read(cx).busy()
                && view.read(cx).session.as_ref().unwrap().config.effort == Effort::High
        });
        assert!(visual.debug_bounds("model-effort-flow").is_some());
        visual.simulate_keystrokes("home");
        wait(visual, |cx| {
            !view.read(cx).busy()
                && view.read(cx).session.as_ref().unwrap().config.effort == Effort::Low
        });
        visual.simulate_keystrokes("end");
        wait(visual, |cx| {
            !view.read(cx).busy()
                && view.read(cx).session.as_ref().unwrap().config.effort == Effort::High
        });
        tap(visual, "model-controls-choose");
        advance(visual, 60);
        let expanding = visual.debug_bounds("model-controls").unwrap();
        assert_eq!(expanding.size.width, compact.size.width);
        assert!(expanding.size.height > compact.size.height);
        advance(visual, 200);
        let expanded = visual.debug_bounds("model-controls").unwrap();
        assert_eq!(expanded.size.width, compact.size.width);
        assert!(expanded.size.height > expanding.size.height);
        assert!(visual.debug_bounds("composer-model-picker").is_some());
        assert!(visual.debug_bounds("model-effort-track").is_none());
        assert!(visual.debug_bounds("composer-model-search").is_none());
        let picker = visual.debug_bounds("composer-model-picker").unwrap();
        let navigation = visual.debug_bounds("composer-model-navigation").unwrap();
        assert_eq!(navigation.top(), picker.top());
        assert_eq!(navigation.bottom(), picker.bottom());
        tap(visual, "composer-model-effort");
        visual.simulate_keystrokes("down enter");
        wait(visual, |cx| {
            !view.read(cx).busy()
                && view.read(cx).session.as_ref().unwrap().config.effort == Effort::Low
        });
        assert!(visual.debug_bounds("composer-model-picker").is_some());
        tap(visual, "model-controls-back");
        advance(visual, 200);
        assert_eq!(
            visual.debug_bounds("model-controls").unwrap().size,
            compact.size
        );
        assert_eq!(
            visual
                .debug_bounds("model-effort-track")
                .unwrap()
                .size
                .width,
            track.size.width
        );
        assert!(visual.debug_bounds("model-effort-track").is_some());
        assert!(visual.debug_bounds("model-effort-flow").is_none());
        visual.simulate_keystrokes("end");
        wait(visual, |cx| {
            !view.read(cx).busy()
                && view.read(cx).session.as_ref().unwrap().config.effort == Effort::High
        });
        tap(visual, "model-controls-choose");
        advance(visual, 200);
        tap(visual, "composer-model-option-0-alternate-model");
        wait(visual, |cx| {
            !view.read(cx).busy()
                && view.read(cx).session.as_ref().unwrap().config.model == "alternate-model"
        });
        assert!(view.read_with(visual, |view, cx| view.model_controls.read(cx).models));
        assert!(visual.debug_bounds("composer-model-picker").is_some());
        tap(visual, "composer-model-effort");
        visual.simulate_keystrokes("down enter");
        wait(visual, |cx| {
            !view.read(cx).busy()
                && view.read(cx).session.as_ref().unwrap().config.effort == Effort::Low
        });
        assert!(visual.debug_bounds("composer-model-picker").is_some());
        tap(visual, "model-controls-back");
        advance(visual, 200);
        assert!(visual.debug_bounds("model-controls").is_some());
        assert!(visual.debug_bounds("model-effort-track").is_some());
        assert!(visual.debug_bounds("composer-model-picker").is_none());
        visual.update(|window, cx| {
            cx.set_reduce_motion(true);
            window.refresh();
            window.draw(cx).clear(cx);
        });
        assert!(visual.debug_bounds("model-effort-flow").is_none());
        tap(visual, "model-controls-choose");
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            visual.debug_bounds("model-controls").unwrap().size,
            expanded.size
        );
        tap(visual, "model-controls-back");
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            visual.debug_bounds("model-controls").unwrap().size,
            compact.size
        );
        tap(visual, "model-controls-choose");
        tap(visual, "composer-model-option-0-no-reasoning");
        wait(visual, |cx| {
            !view.read(cx).busy()
                && view.read(cx).session.as_ref().unwrap().config.model == "no-reasoning"
        });
        tap(visual, "model-controls-back");
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            visual.debug_bounds("model-controls").unwrap().size,
            compact.size
        );
        let revision = view.read_with(visual, |view, _| view.session.as_ref().unwrap().revision);
        tap(visual, "model-effort-track");
        tap(visual, "model-controls-reset");
        visual.simulate_keystrokes("end");
        assert_eq!(
            view.read_with(visual, |view, _| view.session.as_ref().unwrap().revision),
            revision
        );
        visual.update(|window, cx| {
            cx.set_reduce_motion(false);
            window.remove_window();
        });
        drop(view);
        fixture.close();
    }
}

fn advance(cx: &mut VisualTestContext, millis: u64) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(millis));
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}
