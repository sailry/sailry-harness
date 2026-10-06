use super::*;
use core::prelude::v1::test;
use fixture::Fixture;
use gpui_kit::{component::*, *};
use sailry_protocol::Command;
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

mod activity;
mod browser;
mod connections;
mod conversation;
mod files;
mod game;
mod git;
mod loading;
mod navigation;
mod recovery;
mod reminders;
mod scheduled;
mod scopes;
mod session;
mod settings;
mod terminals;
mod theme;
mod worktrees;

pub(in crate::plugins) fn init(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    rust_i18n::set_locale("en");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
    rust_i18n::set_locale("en");
}

#[track_caller]
pub(in crate::plugins) fn wait(cx: &mut VisualTestContext, predicate: impl Fn(&App) -> bool) {
    wait_with(cx, Duration::from_millis(10), predicate);
}

#[track_caller]
fn wait_with(cx: &mut VisualTestContext, cadence: Duration, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.executor().advance_clock(cadence);
        cx.run_until_parked();
        if cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            predicate(cx)
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "plugin update deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[track_caller]
fn toast(cx: &mut VisualTestContext, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.executor().advance_clock(Duration::from_millis(10));
        cx.run_until_parked();
        let current = cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            (!window.notifications(cx).is_empty())
                .then(|| crate::feedback::tests::summary(window, cx))
        });
        if current.as_deref() == Some(expected) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "expected toast {expected:?}, received {current:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(cx.update(crate::feedback::tests::summary), expected,);
}

struct Harness(Entity<Panel>);
impl Render for Harness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(px(crate::preview::HEADER_HEIGHT))
                    .child(Panel::heading(&self.0, cx))
                    .children(self.0.read(cx).header()),
            )
            .child(self.0.clone())
    }
}

fn mount<'a>(
    fixture: &Fixture,
    cx: &'a mut TestAppContext,
) -> (Entity<Panel>, &'a mut VisualTestContext) {
    let mut panel = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let source = cx.new(|cx| {
            crate::conversation::live::View::new(
                fixture.binding.clone(),
                Some(fixture.session.clone()),
                window,
                cx,
            )
        });
        let view = cx.new(|cx| Panel::new(source, cx));
        panel = Some(view.clone());
        let content = cx.new(|cx| {
            cx.observe(&view, |_, _, cx| cx.notify()).detach();
            Panel::observe_notifications(&view, window, cx);
            Harness(view)
        });
        Root::new(content, window, cx)
    });
    let panel = panel.unwrap();
    wait(visual, |cx| {
        panel.read(cx).connected && panel.read(cx).metadata.read(cx).settled()
    });
    (panel, visual)
}

#[track_caller]
pub(in crate::plugins) fn click(cx: &mut VisualTestContext, selector: &'static str) {
    crate::conversation::live::tests::fixture::tap(cx, selector);
}

fn open_package(visual: &mut VisualTestContext, panel: &Entity<Panel>, name: &str) {
    let index = panel.read_with(visual, |panel, cx| {
        panel
            .metadata
            .read(cx)
            .entries
            .values()
            .filter(|info| {
                info.summary.enabled
                    && info.extension.as_ref().is_some_and(|extension| {
                        extension
                            .desktop
                            .as_ref()
                            .is_some_and(|desktop| desktop.entry.is_some())
                    })
            })
            .position(|info| info.summary.name == name)
            .expect("installed package entry")
    });
    click(
        visual,
        Box::leak(format!("plugin-open-{index}").into_boxed_str()),
    );
}

pub(crate) fn diagnostics(panel: &Entity<Panel>, cx: &App) -> String {
    let state = panel.read(cx);
    format!(
        "connected={}, loading={}, selected={:?}, metadata={:?}, packages={:?}; {}",
        state.connected,
        state.loading,
        state.selected,
        state.metadata.read(cx).errors,
        state
            .metadata
            .read(cx)
            .entries
            .iter()
            .map(|(name, info)| (name, info.summary.reference()))
            .collect::<Vec<_>>(),
        snapshot(panel, cx)
    )
}

pub(in crate::plugins) fn snapshot(panel: &Entity<Panel>, cx: &App) -> String {
    let state = panel.read(cx);
    assert!(
        state.error.is_none(),
        "plugin {:?}: {:?}",
        state.selected,
        state.error
    );
    let Some(mounted) = &state.mounted else {
        return String::new();
    };
    let script = mounted
        .root
        .read(cx)
        .content()
        .clone()
        .downcast::<gpui_shell::ScriptView>()
        .unwrap();
    let script = script.read(cx);
    assert!(script.build_error().is_none(), "{:?}", script.build_error());
    script
        .snapshot()
        .map(|snapshot| snapshot.debug_tree())
        .unwrap_or_default()
}

fn control(cx: &mut VisualTestContext, x: f32, y: f32) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = cx.debug_bounds("plugin-panel").unwrap();
    cx.simulate_click(bounds.origin + point(px(x), px(y)), Modifiers::default());
    cx.run_until_parked();
}

#[gpui::test]
fn completes_panel_lifecycle(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote);
        fixture.package();
        session::mcp(&fixture);
        let package = fixture.install(0);
        let server = session::run(&mut fixture, &package);
        let (panel, visual) = mount(&fixture, cx);
        click(visual, "plugin-open-0");
        wait(visual, |cx| snapshot(&panel, cx).contains("notes.txt"));
        let script = panel.read_with(visual, |panel, cx| {
            panel
                .mounted
                .as_ref()
                .unwrap()
                .root
                .read(cx)
                .content()
                .clone()
        });
        let script = script
            .downcast::<gpui_shell::ScriptView>()
            .unwrap()
            .downgrade();
        wait(visual, |_| {
            fixture.transport.files.load(Ordering::SeqCst) == 1
        });
        control(visual, 130., 64.);
        let report = fixture.directory.path().join("project/project-summary.md");
        wait(visual, |_| report.exists());
        assert!(
            std::fs::read_to_string(&report)
                .unwrap()
                .contains("notes.txt")
        );
        std::fs::write(
            fixture.directory.path().join("project/another.txt"),
            "External change",
        )
        .unwrap();
        wait(visual, |cx| snapshot(&panel, cx).contains("Files changed"));
        fixture.execute(Command::SetPluginEnabled {
            name: package.summary.name,
            expected_revision: 1,
            enabled: false,
        });
        wait(visual, |cx| {
            panel.read(cx).mounted.is_none()
                && panel.read(cx).error == Some("plugins_view_unavailable")
        });
        wait(visual, |_| {
            script.upgrade().is_none() && fixture.transport.files.load(Ordering::SeqCst) == 0
        });
        session::disabled(&fixture);
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        assert!(visual.debug_bounds("plugin-back").is_none());
        visual.update(|_, cx| panel.update(cx, |panel, cx| panel.back(cx)));
        wait(visual, |cx| panel.read(cx).selected.is_none());
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
