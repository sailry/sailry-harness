use super::*;
use core::prelude::v1::test;

struct Frame(Entity<Form>);

impl Render for Frame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child(self.0.clone())
    }
}

#[gpui::test]
fn terminal_outcomes_use_toasts(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    for (result, key) in [
        (Ok(()), "files_download_done"),
        (
            Err("files_download_destination"),
            "files_download_destination",
        ),
        (Err("files_download_cancelled"), "files_download_cancelled"),
    ] {
        let mut entity = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let form = cx.new(|_| Form {
                path: "Attachment.txt".into(),
                destination: "/fixture/Attachment.txt".into(),
                cancel: CancellationToken::new(),
                status: Status::Preparing,
            });
            entity = Some(form.clone());
            Root::new(cx.new(|_| Frame(form)), window, cx)
        });
        let form = entity.unwrap();
        visual.update(|window, cx| {
            form.update(cx, |form, cx| {
                form.accept(
                    Status::Receiving {
                        copied: 12,
                        size: 24,
                    },
                    window,
                    cx,
                );
                form.accept(Status::Publishing, window, cx);
            });
            assert!(window.notifications(cx).is_empty());
            form.update(cx, |form, cx| {
                form.accept(Status::Finished(result), window, cx)
            });
            assert_eq!(crate::feedback::tests::summary(window, cx), tr(key));
        });
        crate::feedback::tests::settle(visual);
        let original = visual.update(|window, cx| window.notifications(cx));
        assert_eq!(original.len(), 1);
        assert!(visual.debug_bounds("file-download-finished").is_some());
        assert!(visual.debug_bounds(key).is_none());
        visual.update(|window, cx| {
            form.update(cx, |form, cx| {
                form.accept(Status::Finished(result), window, cx);
                assert_eq!(form.path, "Attachment.txt");
                assert_eq!(form.destination, "/fixture/Attachment.txt");
            });
            assert_eq!(window.notifications(cx), original);
        });
        visual.update(|window, _| window.remove_window());
    }
}
