use super::*;

#[gpui_kit::test]
fn confirmation_does_not_grant_write_access(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (panel, visual) = publication::mount(&fixture, cx);
        let (controller, id, input) = publication::open(&panel, visual);
        publication::replace(&panel, visual, "A read-only scope cannot discard\n");
        let (reply, mut receive) = tokio::sync::oneshot::channel();
        visual.update(|_, cx| {
            let panel = panel.read(cx);
            let context = plugin::Context {
                invocation: None,
                turn: None,
                surface: plugin::desktop::Surface::Workspace,
                package: panel.selected.clone().unwrap(),
                worktree: panel.binding.worktree,
                session: None,
            };
            controller.update(cx, |_, cx| {
                cx.emit(Request {
                    context,
                    write: false,
                    stop: CancellationToken::new(),
                    operation: Operation::Close {
                        id,
                        discard: false,
                        confirm: true,
                    },
                    reply: RefCell::new(Some(reply)),
                })
            });
        });
        visual.run_until_parked();
        let fault = receive.try_recv().unwrap().unwrap_err();
        assert_eq!(fault.code, ErrorCode::PermissionDenied);
        assert!(!visual.has_pending_prompt());
        assert!(controller.read_with(visual, |controller, _| {
            controller.documents.contains_key(&id)
        }));
        assert_eq!(
            input.read_with(visual, |input, _| input.value().to_string()),
            "A read-only scope cannot discard\n"
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(controller);
        drop(input);
        fixture.close();
    }
}
