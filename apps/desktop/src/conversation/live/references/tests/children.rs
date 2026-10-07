use super::*;
use crate::conversation::live::subagents::fixture::Fixture as ChildFixture;

#[gpui::test]
fn keeps_child_read_only(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = ChildFixture::with_children(remote, 1);
        fixture.start();
        let binding = Binding {
            client: fixture.client.clone(),
            defaults: fixture.client.clone(),
            runtime: fixture.runtime.clone(),
            project: fixture.session.project,
            worktree: Some(fixture.session.worktree),
            host: "Fixture".into(),
            project_name: "Guidance picker".into(),
            branch: "main".into(),
        };
        let (parent, visual) = fixture::open(cx, binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            parent
                .read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.page.children.first().is_some_and(|child| {
                        parent.read(cx).child_session(child.run.session).is_some()
                    })
                })
        });
        let child = parent.read_with(visual, |parent, _| {
            parent.history.snapshot.as_ref().unwrap().page.children[0].clone()
        });
        parent.update(visual, |parent, cx| {
            let page =
                Arc::make_mut(&mut Arc::make_mut(parent.history.snapshot.as_mut().unwrap()).page);
            let mut pending = child.clone();
            pending.run.turn = TurnId::new();
            pending.run.sequence += 100;
            pending.run.status = Status::Queued;
            page.children.push(pending.clone());
            assert_eq!(
                parent.child(child.run.session).unwrap().run.turn,
                child.run.turn
            );
            assert_eq!(parent.child_rows(child.origin.turn).len(), 1);
            let page =
                Arc::make_mut(&mut Arc::make_mut(parent.history.snapshot.as_mut().unwrap()).page);
            page.children.last_mut().unwrap().run.status = Status::Running;
            assert_eq!(
                parent.child(child.run.session).unwrap().run.turn,
                pending.run.turn
            );
            assert_eq!(parent.child_rows(child.origin.turn).len(), 1);
            cx.notify();
        });
        let session = parent.read_with(visual, |parent, _| {
            parent.child_session(child.run.session).unwrap()
        });
        visual.update(|window, _| window.remove_window());
        drop(parent);
        let (view, visual) = fixture::open(cx, binding, session.clone());
        wait(visual, |cx| view.read(cx).connected());
        assert!(visual.debug_bounds("child-guide-input").is_none());
        assert!(visual.debug_bounds("child-guide").is_none());
        assert!(visual.debug_bounds("child-guide-attach").is_none());
        assert!(visual.debug_bounds("live-chat-input").is_none());
        assert!(visual.debug_bounds("live-chat-model").is_none());
        assert!(visual.debug_bounds("live-chat-mode").is_none());
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.references.dismiss();
                view.input.update(cx, |input, cx| {
                    input.set_value("Do not submit normally", window, cx)
                });
                view.send(window, cx);
                let mut config = session.config.clone();
                config.effort = Effort::High;
                view.configure(config, window, cx);
                assert!(!view.pending && view.retry.is_none());
                assert_eq!(view.config.as_ref(), Some(&session.config));
            })
        });
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
