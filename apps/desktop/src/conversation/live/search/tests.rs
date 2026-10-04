use super::*;
use crate::conversation::live::tests::{
    fixture::{Fixture, init as init_fixture, tap},
    wait,
};
use core::prelude::v1::test;
use sailry_protocol::{Command, Output};
use std::time::{Duration, Instant};

mod lifecycle;
mod rewind;

fn init(cx: &mut TestAppContext) {
    init_fixture(cx);
    cx.update(crate::shell::init);
}

struct Harness(Entity<View>);

impl Render for Harness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .key_context("SailryConversation")
            .on_action(
                cx.listener(|harness, _: &crate::shell::FindConversation, window, cx| {
                    harness.0.update(cx, |view, cx| view.find(window, cx));
                }),
            )
            .child(
                h_flex()
                    .justify_end()
                    .h_12()
                    .child(self.0.read(cx).header_controls()),
            )
            .child(div().flex_1().min_h_0().child(self.0.clone()))
    }
}

fn open(
    cx: &mut TestAppContext,
    binding: Binding,
    session: Session,
) -> (Entity<View>, &mut VisualTestContext) {
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| View::new(binding, Some(session), window, cx));
        entity = Some(view.clone());
        let harness = cx.new(|_| Harness(view));
        Root::new(harness, window, cx)
    });
    (entity.unwrap(), visual)
}

fn submit(fixture: &Fixture, message: String) -> TurnId {
    let Output::QueuedTurn(turn) = fixture.execute(Command::SubmitTurn {
        session: fixture.session.id,
        expected_revision: 1,
        message: message.into(),
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
            "search fixture completion deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    turn.id
}

fn searched(view: &Entity<View>, cx: &mut VisualTestContext, count: usize) {
    wait(cx, |cx| {
        let search = view.read(cx).search.read(cx);
        let results = search.list.read(cx).delegate();
        !results.loading && results.matches.len() == count
    });
}

fn search_divider(visual: &mut VisualTestContext, visible: bool) {
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = visual.debug_bounds("list-search").unwrap();
    visual.update(|window, _| {
        let bounds = bounds.scale(window.scale_factor());
        let borders: Vec<_> = window
            .painted_quads()
            .into_iter()
            .filter(|quad| quad.bounds == bounds)
            .map(|quad| quad.border_widths.bottom)
            .collect();
        if visible {
            assert!(borders.iter().any(|width| *width > Default::default()));
        } else {
            assert!(borders.iter().all(|width| *width == Default::default()));
        }
    });
}

#[gpui::test]
fn finds_old_turns(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        let first = submit(&fixture, "First match ÄBC 中文 🙂 literal .*".into());
        for index in 1..45 {
            submit(&fixture, format!("History {index} 中文 🙂"));
        }
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).connected()
                && crate::conversation::live::tests::contributions(view.read(cx))
                    .read(cx)
                    .ready(cx)
        });
        assert_eq!(view.read_with(visual, |view, _| view.rows.len()), 20);
        tap(visual, "live-chat-input");
        visual.simulate_input("Preserve draft 中文 🙂");
        // Resolve the textarea's deferred measurement before recording the viewport.
        for _ in 0..2 {
            wait(visual, |_| true);
        }
        let viewport = visual.debug_bounds("live-history-viewport").unwrap();
        assert!(visual.update(|window, cx| {
            let focus = view.read(cx).input.focus_handle(cx);
            focus.is_focused(window)
                && !window
                    .bindings_for_action_in(&crate::shell::FindConversation, &focus)
                    .is_empty()
        }));
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx)).as_ref(),
            "Preserve draft 中文 🙂"
        );
        visual.simulate_keystrokes("secondary-f");
        wait(visual, |cx| view.read(cx).search.read(cx).open);
        let collapsed = visual.debug_bounds("live-search-panel").unwrap();
        assert!(collapsed.size.height < px(50.));
        search_divider(visual, false);
        assert!(visual.debug_bounds("live-search-case").is_none());
        assert!(visual.debug_bounds("live-search-refresh").is_none());
        assert!(visual.debug_bounds("live-search-close").is_none());
        visual.simulate_input("äbc 中文 🙂 literal .*");
        searched(&view, visual, 1);
        search_divider(visual, true);
        assert_eq!(
            visual.debug_bounds("live-history-viewport").unwrap(),
            viewport
        );
        view.read_with(visual, |view, cx| {
            let search = view.search.read(cx);
            assert_eq!(search.list.read(cx).delegate().matches[0].turn, first);
        });
        assert!(visual.update(|window, cx| {
            view.read(cx)
                .search
                .read(cx)
                .list
                .focus_handle(cx)
                .is_focused(window)
        }));
        assert!(
            visual
                .debug_bounds("live-search-panel")
                .unwrap()
                .size
                .height
                > collapsed.size.height
        );
        lifecycle::query(&view, visual, "No matching text");
        searched(&view, visual, 0);
        search_divider(visual, true);
        assert_eq!(
            visual
                .debug_bounds("live-search-panel")
                .unwrap()
                .size
                .height,
            collapsed.size.height + px(1.)
        );
        lifecycle::query(&view, visual, "");
        searched(&view, visual, 0);
        search_divider(visual, false);
        assert_eq!(
            visual
                .debug_bounds("live-search-panel")
                .unwrap()
                .size
                .height,
            collapsed.size.height
        );
        lifecycle::query(&view, visual, "äbc 中文 🙂 literal .*");
        searched(&view, visual, 1);
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| view.read(cx).rows.first() == Some(&first));
        wait(visual, |cx| !view.read(cx).search.read(cx).open);
        assert!(view.read_with(visual, |view, cx| {
            !view.scroller.read(cx).is_following_tail()
        }));
        assert!(visual.debug_bounds("live-search-panel").is_none());
        assert!(
            visual.update(|window, cx| view.read(cx).input.focus_handle(cx).is_focused(window))
        );
        let bounds = visual
            .debug_bounds(Box::leak(format!("live-turn-{first}").into_boxed_str()))
            .unwrap();
        assert!((bounds.origin.y - viewport.origin.y).abs() < px(30.));
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value().to_string()),
            "Preserve draft 中文 🙂"
        );
        tap(visual, "live-search");
        visual.simulate_keystrokes("escape");
        wait(visual, |cx| !view.read(cx).search.read(cx).open);
        assert!(
            visual.update(|window, cx| view.read(cx).input.focus_handle(cx).is_focused(window))
        );
        assert_eq!(fixture.task_requests(), 45);
        fixture.close();
    }
}

#[gpui::test]
fn retries_selected_target(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        let first = submit(&fixture, "Oldest search target 中文 🙂".into());
        for index in 1..45 {
            submit(&fixture, format!("History {index}"));
        }
        let observed = Arc::new(
            crate::conversation::live::paging::tests::transport::Observed::new(
                fixture.transport.clone(),
            ),
        );
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(observed.clone()));
        let (view, visual) = open(cx, binding, fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("Reading draft 🙂");
        tap(visual, "live-search");
        visual.simulate_input("Oldest search target");
        searched(&view, visual, 1);
        tap(visual, "live-search-result-0");
        wait(visual, |cx| view.read(cx).history.older_error.is_some());
        assert_eq!(
            view.read_with(visual, |view, _| view
                .reveal
                .as_ref()
                .map(|target| target.turn)),
            Some(first)
        );
        assert_eq!(observed.reads(), 1);
        tap(visual, "live-load-older");
        fixture.runtime.block_on(observed.wait_held());
        let live = submit(&fixture, "Live while locating a search result".into());
        wait(visual, |cx| view.read(cx).rows.last() == Some(&live));
        assert!(view.read_with(visual, |view, _| view.history.loading_older));
        observed.release();
        wait(visual, |cx| {
            view.read(cx).reveal.is_none() && view.read(cx).rows.first() == Some(&first)
        });
        assert_eq!(observed.reads(), 3);
        assert_eq!(observed.cancelled_reads(), 0);
        assert!(!view.read_with(visual, |view, cx| {
            view.scroller.read(cx).is_following_tail()
        }));
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value().to_string()),
            "Reading draft 🙂"
        );
        assert_eq!(fixture.task_requests(), 46);
        fixture.close();
    }
}
