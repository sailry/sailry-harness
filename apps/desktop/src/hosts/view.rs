use super::*;

impl Installer {
    pub(super) fn footer(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.form {
            return self
                .editor
                .update(cx, |editor, cx| editor.footer(cx).into_any_element());
        }
        DialogFooter::new()
            .w_full()
            .child(
                Button::new("host-install-cancel")
                    .label(tr(if self.error.is_some() {
                        "close"
                    } else {
                        "settings_cancel"
                    }))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cancel();
                        cx.emit(DismissEvent);
                    })),
            )
            .when(self.key.is_some(), |footer| {
                footer.child(
                    Button::new("host-install-trust")
                        .primary()
                        .label(tr("ssh_trust"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            let profile = this.profile.as_ref().unwrap();
                            let command = Command::TrustSsh {
                                profile: profile.id,
                                expected_revision: profile.revision,
                                key: this.key.take().unwrap(),
                            };
                            this.execute(command, window, cx);
                        })),
                )
            })
            .into_any_element()
    }
}

impl Render for Installer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = v_flex().pt_4().min_w_0().gap_3();
        if let Some(error) = &self.error {
            let detail = error.message.clone();
            return body
                .when(self.form, |body| body.child(self.editor.clone()))
                .child(
                    Button::new("host-install-error")
                        .ghost()
                        .label(tr("ssh_details"))
                        .on_click(move |_, window, cx| {
                            window.open_dialog(cx, {
                                let detail = detail.clone();
                                move |dialog, _, _| {
                                    dialog
                                        .form_title(tr("ssh_details"))
                                        .child(div().whitespace_normal().child(detail.clone()))
                                }
                            });
                        }),
                )
                .into_any_element();
        }
        if self.form {
            return body.child(self.editor.clone()).into_any_element();
        }
        if let Some(key) = &self.key {
            return body
                .child(tr(if self.changed {
                    "ssh_key_changed"
                } else {
                    "ssh_key_unknown"
                }))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .whitespace_normal()
                        .child(key.fingerprint.clone()),
                )
                .into_any_element();
        }
        let (label, percent) = match self.progress {
            InstallProgress::Connecting => ("connection_connecting", 5.),
            InstallProgress::Detecting => ("host_install_detecting", 10.),
            InstallProgress::Uploading { sent, total } => (
                "host_install_uploading",
                10. + 65. * sent as f32 / total.max(1) as f32,
            ),
            InstallProgress::Installing => ("host_install_starting", 80.),
            InstallProgress::Pairing => ("host_install_pairing", 95.),
        };
        body.py_6()
            .items_center()
            .gap_4()
            .child(spinner::Spinner::new())
            .child(tr(label))
            .child(
                div()
                    .w_full()
                    .debug_selector(|| "host-install-progress".into())
                    .child(Progress::new("host-install-progress").value(percent)),
            )
            .into_any_element()
    }
}
