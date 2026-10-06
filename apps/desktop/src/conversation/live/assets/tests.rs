use super::*;
use crate::conversation::live::tests::{
    fixture::{Fixture, init, tap},
    wait,
};
use core::prelude::v1::test;
use std::time::{Duration, Instant};
mod lifecycle;

#[derive(Default)]
struct RetryRegions(std::collections::HashMap<WindowId, (Bounds<Pixels>, Bounds<Pixels>)>);
impl Global for RetryRegions {}

// Observe the real native target's inherited clip without adding a hitbox or changing layout.
pub(super) fn record_retry(bounds: Bounds<Pixels>, window: &Window, cx: &mut App) {
    cx.default_global::<RetryRegions>().0.insert(
        window.window_handle().window_id(),
        (bounds, window.content_mask().bounds),
    );
}

fn visible_retry(cx: &mut VisualTestContext) {
    let (bounds, mask) =
        cx.update(|window, cx| cx.global::<RetryRegions>().0[&window.window_handle().window_id()]);
    assert!(
        mask.contains(&bounds.center()),
        "asset Retry is clipped: bounds={bounds:?}, mask={mask:?}"
    );
}

struct Harness(Entity<View>);
impl Render for Harness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(self.0.read(cx).header_controls())
            .child(div().flex_1().min_h_0().child(self.0.clone()))
    }
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    fixture: &Fixture,
) -> (Entity<View>, &'a mut VisualTestContext) {
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            View::new(
                fixture.binding.clone(),
                Some(fixture.session.clone()),
                window,
                cx,
            )
        });
        entity = Some(view.clone());
        let harness = cx.new(|_| Harness(view));
        Root::new(harness, window, cx)
    });
    (entity.unwrap(), visual)
}

fn submit(fixture: &Fixture, text: String) {
    let Output::QueuedTurn(turn) = fixture.execute(Command::SubmitTurn {
        session: fixture.session.id,
        expected_revision: 1,
        message: text.into(),
    }) else {
        panic!("turn expected")
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let page = fixture
            .runtime
            .block_on(
                fixture
                    .binding
                    .client
                    .read_conversation(fixture.session.id, None, 1),
            )
            .unwrap();
        if page
            .page
            .runs
            .iter()
            .any(|run| run.turn == turn.id && run.status == Status::Completed)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "asset fixture completion deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn browsing_preserves_drafts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::markdown(
                "[Report](report.md)".into(),
            ))
        });
        for index in 0..22 {
            submit(&fixture, format!("[Input](input-{index}.txt)"));
        }
        let (view, visual) = open(cx, &fixture);
        wait(visual, |cx| view.read(cx).connected());
        assert_eq!(view.read_with(visual, |view, _| view.rows.len()), 20);
        tap(visual, "live-chat-input");
        visual.simulate_input("Keep my draft");
        tap(visual, "live-assets");
        wait(visual, |cx| !view.read(cx).assets.read(cx).loading);
        let panel = view.read_with(visual, |view, _| view.assets.clone());
        assert_eq!(panel.read_with(visual, |panel, _| panel.groups.len()), 20);
        tap(visual, "asset-tab-1");
        wait(visual, |cx| !panel.read(cx).loading);
        panel.read_with(visual, |panel, _| {
            assert!(!panel.failed);
            assert_eq!(panel.groups.len(), 20);
            assert!(
                panel
                    .groups
                    .iter()
                    .all(|group| group.kind == Kind::Resource)
            );
        });
        panel.update_in(visual, |panel, window, cx| panel.load(true, window, cx));
        wait(visual, |cx| !panel.read(cx).loading);
        panel.read_with(visual, |panel, _| {
            assert_eq!(panel.groups.len(), 22);
            assert!(panel.before.is_none());
            assert_eq!(
                panel.groups.last().unwrap().items,
                [Target::File {
                    path: "input-0.txt".into()
                }]
            );
        });
        tap(visual, "asset-tab-2");
        wait(visual, |cx| !panel.read(cx).loading);
        assert!(panel.read_with(visual, |panel, _| {
            panel
                .groups
                .iter()
                .all(|group| group.kind == Kind::Artifact)
        }));
        let chosen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let captured = chosen.clone();
        visual.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| {
                if let Event::FileAt(worktree, path) = event {
                    captured.borrow_mut().push((*worktree, path.clone()));
                }
            })
            .detach();
        });
        let selector = panel.read_with(visual, |panel, _| {
            format!("asset-{}-0", panel.groups[0].sequence)
        });
        tap(visual, Box::leak(selector.into_boxed_str()));
        assert_eq!(
            &*chosen.borrow(),
            &[(fixture.session.worktree, "report.md".into())]
        );
        assert!(visual.debug_bounds("asset-panel").is_none());
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx).to_string()),
            "Keep my draft"
        );
        assert_eq!(fixture.task_requests(), 22);
        fixture.close();
    }
}

#[gpui::test]
fn opens_image_preview_and_returns_to_inventory(cx: &mut TestAppContext) {
    init(cx);
    // Kit dialog entrances use wall time, not the fixture's executor clock.
    cx.update(|cx| cx.set_reduce_motion(true));
    for remote in [false, true] {
        let fixture = Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::markdown(
                "![Image](result.png)".into(),
            ))
        });
        image::DynamicImage::new_rgb8(2, 2)
            .save(fixture.directory.path().join("project/result.png"))
            .unwrap();
        submit(&fixture, "Create a picture".into());
        let (view, visual) = open(cx, &fixture);
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-assets");
        let panel = view.read_with(visual, |view, _| view.assets.clone());
        wait(visual, |cx| !panel.read(cx).loading);
        let selector = panel.read_with(visual, |panel, _| {
            format!("asset-{}-0", panel.groups[0].sequence)
        });
        tap(visual, Box::leak(selector.into_boxed_str()));
        assert!(visual.debug_bounds("attachment-image-preview").is_some());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            wait(visual, |_| true);
            if visual.debug_bounds("image-lightbox-image").is_some() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "image preview load deadline (remote={remote})"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        visual.update(|window, cx| window.close_dialog(cx));
        wait(visual, |cx| panel.read(cx).open);
        assert!(visual.debug_bounds("asset-panel").is_some());
        visual.simulate_keystrokes("escape");
        wait(visual, |cx| !panel.read(cx).open);
        fixture.close();
    }
}
