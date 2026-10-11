use super::*;
use sailry_protocol::{
    ApprovalId,
    conversation::{ApprovalState, Status},
};

fn pending(fixture: &Fixture) -> Option<ApprovalId> {
    let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
        session: fixture.session.id,
        before: None,
        limit: 100,
    }) else {
        panic!("history expected");
    };
    history
        .page
        .approvals
        .iter()
        .find(|approval| approval.state == ApprovalState::Pending)
        .map(|approval| approval.id)
}

#[gpui::test]
fn plaza_resolves_through_shared_controls(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture);
        let server=fixture.runtime.block_on(crate::agent_fixture::Server::tools(vec![(
            crate::agent_fixture::plugin_tool("files","write_file"),
            serde_json::json!({"path":"approved.txt","text":"Approved result","expected_revision":null}),
        )]));
        let Output::Snapshot(state) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected");
        };
        let mut provider = state.providers[0].clone();
        provider.endpoint = server.endpoint.clone();
        fixture.execute(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        });
        let (shell, visual) = files::mount(&fixture, remote, cx);
        let panel = open(&shell, &fixture, visual);
        let id = selector(format!("plaza-{}", fixture.session.id));
        let position = selector(format!("plaza-{}-position", fixture.session.id));
        let seated = visual.debug_bounds(position).unwrap();
        fixture.execute(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Write the fixture file".into(),
        });
        wait(visual, |_| pending(&fixture).is_some());
        let approval = pending(&fixture).unwrap();
        // The scene follows the canonical waiting projection without a walking phase.
        wait(visual, |cx| snapshot(&panel, cx).contains("Approvals 1"));
        let plaza = visual.debug_bounds("plaza-atrium").unwrap();
        assert!(plaza.contains(&visual.debug_bounds(position).unwrap().center()));
        assert!(
            visual
                .debug_bounds(selector(format!("plaza-{}-status", fixture.session.id)))
                .is_some()
        );
        click(visual, id);
        wait(visual, |cx| {
            chat(&panel, cx).is_some_and(|chat| chat.read(cx).connected())
        });
        wait(visual, |cx| {
            chat(&panel, cx).is_some_and(|chat| {
                chat.read(cx)
                    .tool_history()
                    .snapshot
                    .as_ref()
                    .is_some_and(|history| {
                        history
                            .page
                            .approvals
                            .iter()
                            .any(|item| item.id == approval && item.state == ApprovalState::Pending)
                    })
            })
        });
        let weak = panel.read_with(visual, |_, cx| chat(&panel, cx).unwrap().downgrade());
        // Dismiss the shared approval popover before closing its sidebar.
        click(visual, "live-approval-trigger");
        assert!(visual.debug_bounds("pending-approvals").is_none());
        click(visual, "plaza-detail-close");
        wait(visual, |cx| {
            chat(&panel, cx).is_none() && weak.upgrade().is_none()
        });
        assert_eq!(pending(&fixture), Some(approval));
        click(visual, id);
        wait(visual, |cx| {
            chat(&panel, cx).is_some_and(|chat| {
                chat.read(cx).connected()
                    && chat
                        .read(cx)
                        .tool_history()
                        .snapshot
                        .as_ref()
                        .is_some_and(|history| {
                            history.page.approvals.iter().any(|item| {
                                item.id == approval && item.state == ApprovalState::Pending
                            })
                        })
            })
        });
        click(
            visual,
            selector(format!("live-approval-approve-{approval}")),
        );
        wait(visual, |_| {
            let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
                session: fixture.session.id,
                before: None,
                limit: 100,
            }) else {
                return false;
            };
            history
                .page
                .approvals
                .iter()
                .any(|item| item.id == approval && item.state == ApprovalState::Approved)
                && history
                    .page
                    .runs
                    .iter()
                    .any(|run| run.status == Status::Completed)
        });
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/approved.txt")).unwrap(),
            "Approved result"
        );
        click(visual, "plaza-detail-close");
        wait(visual, |cx| {
            chat(&panel, cx).is_none() && !snapshot(&panel, cx).contains("Approvals 1")
        });
        assert_eq!(visual.debug_bounds(position).unwrap(), seated);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}
