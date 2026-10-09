use super::*;
use crate::{theme::DialogStyle as _, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    form::Field,
    input::{Input, InputState},
};
use sailry_protocol::backup::Record;

pub(super) struct Restore {
    owner: Entity<Panel>,
    record: Record,
    pub(super) target: Entity<InputState>,
    error: Option<&'static str>,
}

pub(super) fn open(owner: Entity<Panel>, record: Record, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| {
        crate::feedback::observe(window, cx, |form: &Restore, _| {
            form.error.into_iter().collect()
        });
        Restore {
            owner,
            record,
            target: cx.new(|cx| InputState::new(window, cx).placeholder(tr("backup_target"))),
            error: None,
        }
    });
    #[cfg(test)]
    form.read(cx)
        .owner
        .clone()
        .update(cx, |panel, _| panel.restore = Some(form.downgrade()));
    window.open_dialog(cx, move |dialog, window, cx| {
        let restore = form.clone();
        dialog
            .form_title(tr("backup_restore"))
            .w((window.viewport_size().width - px(48.)).min(px(560.)))
            .child(form.clone())
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("backup-restore-close")
                            .label(tr("close"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("backup-restore-confirm")
                            .danger()
                            .label(tr("backup_restore"))
                            .debug_selector(|| "backup-restore-confirm".into())
                            .disabled(form.read(cx).owner.read(cx).locked())
                            .on_click(move |_, window, cx| {
                                if restore.update(cx, |form, cx| form.submit(window, cx)) {
                                    window.close_dialog(cx);
                                }
                            }),
                    ),
            )
    });
}

impl Restore {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.owner.read(cx).active {
            self.error = Some("backup_target_changed");
            cx.notify();
            return false;
        }
        let target = self.target.read(cx).value().trim().to_owned();
        if !std::path::Path::new(&target).is_absolute() {
            self.error = Some("backup_invalid_target");
            cx.notify();
            return false;
        }
        let Some(settings) = self.owner.read(cx).settings.clone() else {
            return false;
        };
        let id = self.record.id.clone();
        self.owner.update(cx, |panel, cx| {
            panel.submit(
                Command::RestoreBackup {
                    id,
                    expected_revision: settings.revision,
                    target,
                },
                window,
                cx,
            )
        })
    }
}
impl Render for Restore {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(div().text_sm().child(tr("backup_restore_consequence")))
            .child(
                Field::new().label(tr("backup_target")).child(
                    div()
                        .debug_selector(|| "backup-target".into())
                        .child(Input::new(&self.target).disabled(self.owner.read(cx).locked())),
                ),
            )
    }
}
