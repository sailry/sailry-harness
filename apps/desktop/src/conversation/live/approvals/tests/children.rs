use super::*;
use crate::conversation::live::subagents::fixture::Fixture as ChildFixture;

#[gpui::test]
fn resolves_children_in_parent(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = ChildFixture::new(remote);
        fixture.start();
        let binding = Binding {
            client: fixture.client.clone(),
            defaults: fixture.client.clone(),
            runtime: fixture.runtime.clone(),
            project: fixture.session.project,
            worktree: Some(fixture.session.worktree),
            host: "Fixture".into(),
            project_name: "Parent approvals".into(),
            branch: "main".into(),
        };
        let (parent, visual) = open(cx, binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            parent
                .read(cx)
                .approvals
                .children
                .values()
                .filter(|child| {
                    child.history.calls.iter().any(|call| {
                        call.approval
                            .as_ref()
                            .is_some_and(|approval| approval.state == ApprovalState::Pending)
                    })
                })
                .count()
                == 2
        });
        let approvals = parent.read_with(visual, |view, _| {
            view.approvals
                .children
                .values()
                .flat_map(|child| {
                    child
                        .history
                        .snapshot
                        .as_ref()
                        .unwrap()
                        .page
                        .approvals
                        .iter()
                })
                .filter(|approval| approval.state == ApprovalState::Pending)
                .cloned()
                .collect::<Vec<_>>()
        });
        assert!(visual.debug_bounds("live-approval-trigger").is_some());
        assert!(!fixture.directory.path().join("project/child.txt").exists());
        for approval in &approvals {
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("live-approval-child-{}", approval.id).into_boxed_str()
                    ))
                    .is_some()
            );
        }
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        tap(visual, "live-chat-input");
        visual.simulate_input("Keep the parent draft 中文 🙂");
        // A disconnected child projection must not authorize its stale call.
        parent.update(visual, |view, _| {
            view.approvals
                .children
                .get_mut(&approvals[0].session)
                .unwrap()
                .history
                .connected = false;
        });
        visual.update(|window, cx| {
            parent.update(cx, |view, cx| {
                view.resolve_approval(approvals[0].id, Decision::Approve, window, cx);
                assert!(view.approvals.attempt.is_none());
                view.approvals
                    .children
                    .get_mut(&approvals[0].session)
                    .unwrap()
                    .history
                    .connected = true;
            })
        });
        tap(visual, "live-approval-trigger");
        tap(
            visual,
            &format!("live-approval-approve-{}", approvals[0].id),
        );
        wait(visual, |cx| {
            !parent.read(cx).approvals.pending
                && fixture.page(approvals[0].session).runs[0].status == Status::Completed
        });
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/child.txt")).unwrap(),
            "Delegated Unicode content 中文 🙂"
        );
        assert_eq!(
            parent.read_with(visual, |view, cx| view.draft(cx).to_string()),
            "Keep the parent draft 中文 🙂"
        );
        visual.update(|window, _| window.remove_window());
        drop(parent);
        // Reopening the parent recovers pending child approvals without opening a child.
        let (parent, visual) = open(cx, binding, fixture.session.clone());
        wait(visual, |cx| {
            parent
                .read(cx)
                .approvals
                .children
                .get(&approvals[1].session)
                .is_some_and(|child| {
                    child.history.calls.iter().any(|call| {
                        call.approval.as_ref().is_some_and(|approval| {
                            approval.id == approvals[1].id
                                && approval.state == ApprovalState::Pending
                        })
                    })
                })
        });
        tap(visual, &format!("live-approval-reject-{}", approvals[1].id));
        wait(visual, |cx| {
            !parent.read(cx).approvals.pending
                && parent.read(cx).active().is_none()
                && fixture.page(approvals[1].session).runs[0].status == Status::Completed
        });
        assert_eq!(
            fixture
                .page(approvals[0].session)
                .approvals
                .iter()
                .find(|approval| approval.id == approvals[0].id)
                .unwrap()
                .state,
            ApprovalState::Approved
        );
        assert_eq!(
            fixture
                .page(approvals[1].session)
                .approvals
                .iter()
                .find(|approval| approval.id == approvals[1].id)
                .unwrap()
                .state,
            ApprovalState::Denied
        );
        assert!(visual.debug_bounds("live-approval-trigger").is_none());
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 4);
        visual.update(|window, _| window.remove_window());
        drop(parent);
        fixture.close();
    }
}
