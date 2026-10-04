pub(crate) use super::attachments::tests::{
    Control as AttachmentControl, control as attachment_control,
};
use super::*;
use core::prelude::v1::test;
use sailry_link::{Link, NetworkScope, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{
    ProviderId,
    conversation::{Model, ModelApi, Part},
};
use std::time::{Duration, Instant};

pub(super) use crate::agent_fixture as support;

mod compaction;
mod configuration;
mod context;
mod files;
pub(crate) mod fixture;
mod manual_compaction;
mod message_display;
mod outgoing;
mod progress;
mod recovery;
mod retry;
mod statistics;
mod streaming;
mod tools;
mod unassigned;
mod web_search;

pub(crate) fn contributions(view: &View) -> &Entity<crate::plugins::contributions::Registry> {
    &view.contributions
}

#[track_caller]
pub(crate) fn wait(cx: &mut VisualTestContext, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.executor().advance_clock(Duration::from_millis(10));
        cx.run_until_parked();
        if cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            predicate(cx)
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "conversation update deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn open_models(cx: &mut VisualTestContext) {
    click(cx, "live-chat-model");
    settle_models(cx);
    click(cx, "model-controls-choose");
    settle_models(cx);
}

fn settle_models(cx: &mut VisualTestContext) {
    // Kit motion uses wall time; the executor clock only drives async timers.
    let duration = cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        cx.theme().motion_tokens().duration_normal
    });
    cx.run_until_parked();
    cx.executor().advance_clock(duration);
    std::thread::sleep(duration);
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

pub(super) fn click(cx: &mut VisualTestContext, selector: &'static str) {
    if matches!(
        selector,
        "chat-retry"
            | "chat-sync-retry"
            | "live-approval-retry"
            | "queue-retry"
            | "live-search-retry"
            | "live-load-older"
    ) {
        crate::feedback::tests::settle(cx);
    }
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let bounds = cx
        .debug_bounds(selector)
        .expect("control after pointer layout");
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

pub(super) fn provider(endpoint: &str, name: &str) -> Provider {
    Provider {
        options: None,
        id: ProviderId::new(),
        revision: 0,
        name: name.into(),
        api: ModelApi::ChatCompletions,
        authentication: sailry_protocol::Authentication::ApiKey,
        endpoint: endpoint.into(),
        enabled: true,
        credential: None,
        default_model: name.into(),
        models: vec![Model {
            id: name.into(),
            context: 4096,
            output: 128,
            vision: false,
            tools: false,
            reasoning: true,
            web_search: false,
            generates: vec![],
            efforts: vec![sailry_protocol::Effort::Low, sailry_protocol::Effort::High],
            custom_efforts: false,
            default_effort: sailry_protocol::Effort::High,
        }],
    }
}

#[gpui::test]
fn resumes_after_close(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let node = runtime
            .block_on(Node::start(directory.path().join("node")))
            .unwrap();
        let controller = runtime
            .block_on(Link::controller(
                directory.path().join("controller"),
                NetworkScope::default(),
            ))
            .unwrap();
        let address = runtime
            .block_on(
                controller
                    .handle()
                    .pair(node.link().invite().unwrap().ticket()),
            )
            .unwrap();
        let transport: Arc<dyn Transport> = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Arc::new(Client::new(transport));
        let fast = runtime.block_on(support::Server::start(false));
        let slow = runtime.block_on(support::Server::start(true));
        let first = provider(&fast.endpoint, "fast-model");
        let second = provider(&slow.endpoint, "slow-model");
        for provider in [&first, &second] {
            runtime
                .block_on(client.execute(client.prepare(Command::SaveProvider {
                    provider: provider.clone(),
                    expected_revision: 0,
                    secret: None,
                })))
                .unwrap();
        }
        let Output::Project(project) = runtime
            .block_on(client.execute(client.prepare(Command::RegisterProject {
                name: "Chat fixture".into(),
                path: directory.path().to_str().unwrap().into(),
            })))
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.project == Some(project.id) && tree.main)
            .unwrap()
            .id;
        let binding = Binding {
            client: client.clone(),
            defaults: client.clone(),
            runtime: runtime.clone(),
            project: Some(project.id),
            worktree: Some(worktree),
            host: "Fixture Node".into(),
            project_name: "Chat fixture".into(),
            branch: directory.path().to_str().unwrap().into(),
        };
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| View::new(binding.clone(), None, window, cx));
            entity = Some(view.clone());
            Root::new(cx.new(|_| fixture::Harness(view)), window, cx)
        });
        let view = entity.unwrap();
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).configured()
        });
        assert!(view.read_with(visual, |view, _| view.session.is_none()));
        click(visual, "live-chat-input");
        visual.simulate_input("请检查 中文 🙂");
        visual.simulate_keystrokes("shift-enter");
        visual.run_until_parked();
        assert!(view.read_with(visual, |view, _| view.session.is_none()));
        visual.simulate_input("second line");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .any(|run| run.status == Status::Completed)
                })
        });
        let session = view.read_with(visual, |view, cx| {
            assert_eq!(view.error, None);
            assert!(view.input.read(cx).value().is_empty());
            let snapshot = view.history.snapshot.as_ref().unwrap();
            assert!(
                snapshot
                    .page
                    .entries
                    .iter()
                    .any(|entry| entry.author == "user"
                        && entry.parts == [Part::Text("请检查 中文 🙂\nsecond line".into())])
            );
            assert!(
                snapshot
                    .page
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| part == &Part::Text("answer-fast-model".into()))
            );
            view.session.clone().unwrap()
        });
        assert_eq!(fast.requests.lock().unwrap().len(), 1);
        open_models(visual);
        click(visual, "composer-model-option-1-slow-model");
        wait(visual, |cx| {
            view.read(cx).session.as_ref().is_some_and(|session| {
                session.revision == 2 && session.config.provider == second.id
            }) && !view.read(cx).pending
        });
        visual.simulate_keystrokes("escape");
        click(visual, "live-chat-input");
        visual.simulate_input("slow prompt");
        click(visual, "live-chat-send");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| !snapshot.drafts.is_empty())
        });
        click(visual, "live-chat-send");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.drafts.is_empty()
                        && snapshot
                            .page
                            .runs
                            .iter()
                            .any(|run| run.status == Status::Cancelled)
                })
        });
        let cancelled = view.read_with(visual, |view, _| {
            view.history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .runs
                .iter()
                .find(|run| run.status == Status::Cancelled)
                .unwrap()
                .turn
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let selector = Box::leak(format!("live-turn-more-{cancelled}").into_boxed_str());
        assert!(visual.debug_bounds(selector).is_none());
        click(visual, "live-chat-input");
        visual.simulate_input("survives closing");
        click(visual, "live-chat-send");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| !snapshot.drafts.is_empty())
        });
        let resume = view.read_with(visual, |view, _| view.session.clone().unwrap());
        visual.update(|window, _| window.remove_window());
        drop(view);
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| View::new(binding, Some(resume), window, cx));
            entity = Some(view.clone());
            Root::new(cx.new(|_| fixture::Harness(view)), window, cx)
        });
        let view = entity.unwrap();
        wait(visual, |cx| {
            view.read(cx).connected()
                && view
                    .read(cx)
                    .history
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| !snapshot.drafts.is_empty())
        });
        assert_eq!(slow.requests.lock().unwrap().len(), 2);
        assert_eq!(
            view.read_with(visual, |view, _| view.session()),
            Some(session.id)
        );
        click(visual, "live-chat-send");
        wait(visual, |cx| {
            view.read(cx).active().is_none() && !view.read(cx).pending
        });
        visual.update(|window, _| window.remove_window());
        runtime.block_on(node.shutdown()).unwrap();
        runtime.block_on(controller.close()).unwrap();
    }
}

mod dictation;
