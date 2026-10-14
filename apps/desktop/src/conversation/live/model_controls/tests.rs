use super::*;
use core::prelude::v1::test;
use gpui_kit::test::TestWindowExt as _;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{ErrorCode, Fault, Topic};
use std::sync::Mutex;
use tokio::sync::Semaphore;

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
fn keeps_raw_supported_values(cx: &mut TestAppContext) {
    let before = rust_i18n::locale().to_string();
    init(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let mut provider = snapshot.providers[0].clone();
        provider.api = sailry_protocol::conversation::ModelApi::Responses;
        provider.models[0].efforts = vec![
            Effort::Low,
            Effort::Medium,
            Effort::High,
            Effort::XHigh,
            Effort::Max,
        ];
        provider.models[0].custom_efforts = true;
        fixture.execute(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        });
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).configured()
        });
        for locale in ["en", "zh-CN"] {
            rust_i18n::set_locale(locale);
            tap(visual, "live-chat-model");
            tap(visual, "model-controls-choose");
            wait(visual, |_| true);
            let current = view.read_with(visual, |view, _| view.config.as_ref().unwrap().effort);
            let selector = Box::leak(
                format!(
                    "composer-model-effort-value-{}",
                    sailry_protocol::plugin::models::effort_key(current)
                )
                .into_boxed_str(),
            );
            assert!(visual.debug_bounds(selector).is_some());
            for (index, (effort, raw)) in [
                (Effort::Low, "low"),
                (Effort::Medium, "medium"),
                (Effort::High, "high"),
                (Effort::XHigh, "xhigh"),
                (Effort::Max, "max"),
            ]
            .into_iter()
            .enumerate()
            {
                tap(visual, "composer-model-effort");
                visual.update(|window, cx| {
                    window.render_frame(cx);
                    let mut menu = window.within("popup-menu");
                    let labels = (0usize..5)
                        .map(|index| {
                            let item = menu.find(index);
                            assert_eq!(item.role(), Some(Role::MenuItem));
                            assert!(item.visible(), "{item:?}");
                            item.label().unwrap().to_owned()
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(labels, ["low", "medium", "high", "xhigh", "max"]);
                    assert!(menu.try_find(5usize).is_none());
                    for unsupported in ["default", "none", "minimal", "-1", "1024"] {
                        assert!(!labels.iter().any(|label| label == unsupported));
                    }
                    menu.click(index, cx);
                });
                wait(visual, |cx| {
                    !view.read(cx).busy()
                        && view.read(cx).session.as_ref().unwrap().config.effort == effort
                });
                let selector =
                    Box::leak(format!("composer-model-effort-value-{raw}").into_boxed_str());
                assert!(visual.debug_bounds(selector).is_some());
                assert!(visual.debug_bounds("composer-model-picker").is_some());
            }
            visual.simulate_keystrokes("escape");
            wait(visual, |_| true);
        }
        assert_eq!(fixture.task_requests(), 0);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
    rust_i18n::set_locale(&before);
    cx.update(|cx| cx.set_reduce_motion(false));
}

#[gpui::test]
fn selects_models_and_levels(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let mut provider = snapshot.providers[0].clone();
        provider.models[0].display_name = Some("Readable fixture name".into());
        let mut alternate = provider.models[0].clone();
        alternate.id = "alternate-model".into();
        alternate.display_name = Some("Readable alternate name".into());
        provider.models.push(alternate);
        let mut unsupported = provider.models[0].clone();
        unsupported.id = "no-reasoning".into();
        unsupported.display_name = None;
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
        assert_eq!(
            view.read_with(visual, |view, _| view.model_label()),
            "Readable fixture name"
        );
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
        assert_eq!(
            view.read_with(visual, |view, _| view.model_label()),
            "Readable alternate name"
        );
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

struct Gate {
    inner: Arc<dyn Transport>,
    entered: Semaphore,
    release: Semaphore,
    requests: Mutex<Vec<(u64, SessionConfig)>>,
    reject: bool,
}

impl Gate {
    fn new(inner: Arc<dyn Transport>, reject: bool) -> Arc<Self> {
        Arc::new(Self {
            inner,
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            requests: Mutex::new(Vec::new()),
            reject,
        })
    }

    fn requests(&self) -> Vec<(u64, SessionConfig)> {
        self.requests.lock().unwrap().clone()
    }
}

impl Transport for Gate {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let captured = match &request.command {
                Command::SetSessionConfig {
                    expected_revision,
                    config,
                    ..
                } => Some((*expected_revision, config.clone())),
                _ => None,
            };
            let Some(captured) = captured else {
                return self.inner.dispatch(request).await;
            };
            self.requests.lock().unwrap().push(captured);
            if self.reject {
                self.entered.add_permits(1);
                self.release.acquire().await.unwrap().forget();
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "injected configuration conflict",
                ));
            }
            let admission = self.inner.dispatch(request).await?;
            self.entered.add_permits(1);
            self.release.acquire().await.unwrap().forget();
            Ok(admission)
        })
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
}

fn assert_controls_enabled(
    view: &Entity<View>,
    focus: &FocusHandle,
    visual: &mut VisualTestContext,
) {
    visual.update(|window, cx| {
        window.render_frame(cx);
        for selector in [
            "live-chat-model",
            "model-controls-choose",
            "live-chat-mode",
            "live-chat-permission",
        ] {
            assert_ne!(window.find(selector).disabled(), Some(true), "{selector}");
        }
        assert!(focus.is_focused(window));
        assert!(!view.read(cx).configuration_blocked());
    });
}

#[gpui::test]
fn serializes_latest_while_saving(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let mut provider = snapshot.providers[0].clone();
        provider.models[0].efforts = vec![Effort::Low, Effort::Medium, Effort::High];
        provider.models[0].custom_efforts = true;
        fixture.execute(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        });
        let gate = Gate::new(fixture.transport.clone(), false);
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(gate.clone()));
        let (view, visual) = fixture::open(cx, binding, fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).configured()
        });
        tap(visual, "live-chat-model");
        let focus = view.read_with(visual, |view, cx| {
            view.model_controls.read(cx).focus.clone()
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| view.select_effort(Effort::Low, window, cx))
        });
        wait(visual, |_| gate.entered.available_permits() == 1);
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.select_effort(Effort::Medium, window, cx);
                view.select_effort(Effort::High, window, cx);
            })
        });
        assert_eq!(
            view.read_with(visual, |view, _| view.config.as_ref().unwrap().effort),
            Effort::High
        );
        assert_controls_enabled(&view, &focus, visual);
        gate.release.add_permits(1);
        wait(visual, |_| gate.entered.available_permits() == 2);
        assert_controls_enabled(&view, &focus, visual);
        let requests = gate.requests();
        assert_eq!(
            requests
                .iter()
                .map(|(revision, config)| (*revision, config.effort))
                .collect::<Vec<_>>(),
            [(1, Effort::Low), (2, Effort::High)]
        );
        gate.release.add_permits(1);
        wait(visual, |cx| {
            let view = view.read(cx);
            !view.busy()
                && view.session.as_ref().is_some_and(|session| {
                    session.revision == 3 && session.config.effort == Effort::High
                })
        });
        assert!(visual.update(|window, _| focus.is_focused(window)));
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn rolls_back_rejected_change(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let gate = Gate::new(fixture.transport.clone(), true);
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(gate.clone()));
        let (view, visual) = fixture::open(cx, binding, fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).configured()
        });
        tap(visual, "live-chat-model");
        let focus = view.read_with(visual, |view, cx| {
            view.model_controls.read(cx).focus.clone()
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| view.select_effort(Effort::Low, window, cx))
        });
        wait(visual, |_| gate.entered.available_permits() == 1);
        assert_controls_enabled(&view, &focus, visual);
        gate.release.add_permits(1);
        wait(visual, |cx| {
            let view = view.read(cx);
            !view.pending && view.retry.is_none() && view.error == Some("chat_config_changed")
        });
        view.read_with(visual, |view, _| {
            assert_eq!(
                view.config.as_ref().unwrap(),
                &view.session.as_ref().unwrap().config
            );
            assert_eq!(view.config.as_ref().unwrap().effort, Effort::High);
        });
        assert!(visual.update(|window, cx| !window.notifications(cx).is_empty()));
        assert!(visual.update(|window, _| focus.is_focused(window)));
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
