use super::*;
use crate::conversation::live::{ChatFixture, View};
use core::prelude::v1::test;
use gpui_kit::component::Root;

#[gpui::test]
fn restores_local_and_remote_composers_without_execution(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::preferences::init(cx);
    });
    for remote in [false, true] {
        let fixture = ChatFixture::with_tools(remote, vec![]);
        let directory = tempfile::tempdir().unwrap();
        let mut saved = snapshot(fixture.binding.client.target(), Some(fixture.session.id));
        saved.draft.identity.project = fixture.binding.project;
        saved.draft.identity.worktree = fixture.binding.worktree;
        saved.draft.text = "/plan restored draft @资料".into();
        saved.sources.push(Local::Image {
            name: "paste.png".into(),
            format: ImageFormat::Png,
            bytes: Arc::from(b"isolated image fixture".as_slice()),
        });
        Capture {
            snapshots: vec![saved.clone()],
            expected: Arc::new(Mutex::new(None)),
        }
        .save(directory.path())
        .unwrap();
        let (expected, pending) = load(directory.path()).unwrap();
        let marker = pending[0].marker.clone();
        cx.update(|cx| {
            cx.set_global(Cache {
                pending,
                expected: Arc::new(Mutex::new(expected)),
                ..Default::default()
            })
        });
        let mut view = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let chat = cx.new(|cx| {
                View::new(
                    fixture.binding.clone(),
                    Some(fixture.session.clone()),
                    window,
                    cx,
                )
            });
            view = Some(chat.clone());
            Root::new(chat, window, cx)
        });
        let view = view.unwrap();
        crate::conversation::live::tests::wait(visual, |cx| {
            view.read(cx).connected() && marker.is_file()
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| view.focus(window, cx));
            window.draw(cx).clear(cx);
        });
        visual.simulate_keystrokes("left");
        visual.simulate_keystrokes("right");
        visual.run_until_parked();
        visual.update(|_, cx| {
            let recovered = view.read(cx).snapshot_draft(cx).unwrap().unwrap();
            assert_eq!(recovered.draft.text, saved.draft.text);
            assert_eq!(recovered.draft.references, saved.draft.references);
            assert_eq!(recovered.sources.len(), 1);
            // Cached draft preferences cannot configure an existing Node session.
            let sailry_protocol::Output::Snapshot(node) =
                fixture.execute(sailry_protocol::Command::Snapshot)
            else {
                panic!("snapshot expected")
            };
            assert_eq!(node.sessions[0].config.mode, fixture.session.config.mode);
            assert!(cx.global::<Cache>().pending.is_empty());
            assert_eq!(fixture.task_requests(), 0);
        });
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn leaves_other_node_drafts_pending(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::preferences::init(cx);
    });
    let fixture = ChatFixture::with_tools(false, vec![]);
    let directory = tempfile::tempdir().unwrap();
    let mut saved = snapshot(NodeId([9; 32]), Some(fixture.session.id));
    saved.draft.identity.project = fixture.binding.project;
    saved.draft.identity.worktree = fixture.binding.worktree;
    Capture {
        snapshots: vec![saved],
        expected: Arc::new(Mutex::new(None)),
    }
    .save(directory.path())
    .unwrap();
    let (expected, pending) = load(directory.path()).unwrap();
    let marker = pending[0].marker.clone();
    cx.update(|cx| {
        cx.set_global(Cache {
            pending,
            expected: Arc::new(Mutex::new(expected)),
            ..Default::default()
        })
    });
    let mut view = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let chat = cx.new(|cx| {
            View::new(
                fixture.binding.clone(),
                Some(fixture.session.clone()),
                window,
                cx,
            )
        });
        view = Some(chat.clone());
        Root::new(chat, window, cx)
    });
    let view = view.unwrap();
    crate::conversation::live::tests::wait(visual, |cx| view.read(cx).connected());
    visual.update(|_, cx| {
        assert!(view.read(cx).snapshot_draft(cx).unwrap().is_none());
        assert_eq!(cx.global::<Cache>().pending.len(), 1);
        assert!(!marker.exists());
    });
    visual.update(|window, _| window.remove_window());
    fixture.close();
}

#[gpui::test]
fn restores_configuration_without_creating_session(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::preferences::init(cx);
    });
    let fixture = ChatFixture::with_tools(true, vec![]);
    let directory = tempfile::tempdir().unwrap();
    let mut saved = snapshot(fixture.binding.client.target(), None);
    saved.draft.identity.project = fixture.binding.project;
    saved.draft.identity.worktree = fixture.binding.worktree;
    saved.draft.text = "restored new conversation".into();
    let mut configuration = fixture.session.config.clone();
    configuration.mode = sailry_protocol::WorkMode::Plan;
    configuration.permission = sailry_protocol::Permission::Project;
    saved.draft.configuration = Some(configuration.clone());
    Capture {
        snapshots: vec![saved],
        expected: Arc::new(Mutex::new(None)),
    }
    .save(directory.path())
    .unwrap();
    let (expected, pending) = load(directory.path()).unwrap();
    let marker = pending[0].marker.clone();
    cx.update(|cx| {
        cx.set_global(Cache {
            pending,
            expected: Arc::new(Mutex::new(expected)),
            ..Default::default()
        })
    });
    let mut view = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let chat = cx.new(|cx| View::new(fixture.binding.clone(), None, window, cx));
        view = Some(chat.clone());
        Root::new(chat, window, cx)
    });
    let view = view.unwrap();
    crate::conversation::live::tests::wait(visual, |cx| {
        view.read(cx).connected() && marker.is_file()
    });
    visual.update(|_, cx| {
        let recovered = view.read(cx).snapshot_draft(cx).unwrap().unwrap();
        assert_eq!(recovered.draft.configuration, Some(configuration));
        assert_eq!(recovered.draft.mode, Some(sailry_protocol::WorkMode::Plan));
        assert_eq!(recovered.draft.text, "restored new conversation");
        let sailry_protocol::Output::Snapshot(node) =
            fixture.execute(sailry_protocol::Command::Snapshot)
        else {
            panic!("snapshot expected")
        };
        assert_eq!(node.sessions.len(), 1);
        assert_eq!(fixture.task_requests(), 0);
    });
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
