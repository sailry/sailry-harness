use super::*;
use crate::settings::{entry::Entry, group::Group};
use gpui_kit::component::{
    alert::Alert,
    tab::{Tab, TabBar},
};

pub(super) fn open(editor: Entity<Editor>, window: &mut Window, cx: &mut App) {
    let active = editor.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let close = editor.clone();
        let cancel = editor.clone();
        let submit = editor.clone();
        let state = editor.read(cx);
        dialog
            .form_title(tr("skills_install"), cx)
            .w((window.viewport_size().width - px(48.)).min(px(600.)))
            .max_h(window.viewport_size().height * 0.85)
            .overlay_closable(false)
            .on_close(move |_, _, cx| {
                close.update(cx, |editor, _| {
                    editor.closed = true;
                    editor.dialog.dismissed();
                })
            })
            .child(editor.clone())
            .footer(
                DialogFooter::new()
                    .w_full()
                    .gap_2()
                    .child(
                        Button::new("skill-install-cancel")
                            .label(tr("settings_cancel"))
                            .on_click(move |_, window, cx| {
                                cancel.update(cx, |editor, _| {
                                    editor.closed = true;
                                    editor.dialog.dismissed();
                                });
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("skill-install-submit")
                            .debug_selector(|| "skill-install-submit".into())
                            .primary()
                            .loading(state.pending)
                            .disabled(
                                state.pending
                                    || if state.request.is_some() {
                                        false
                                    } else if state.selected_installable() {
                                        state.exceeds_capacity()
                                    } else {
                                        !state.ready(cx)
                                    },
                            )
                            .label(tr(if state.pending && state.discovery.is_some() {
                                "plugins_installing"
                            } else if state.request.is_some() {
                                "plugins_retry"
                            } else if state.selected_installable() {
                                "skills_install"
                            } else {
                                "skills_find"
                            }))
                            .on_click(move |_, window, cx| {
                                submit.update(cx, |editor, cx| editor.submit(window, cx))
                            }),
                    ),
            )
    });
    active.update(cx, |editor, cx| {
        crate::ui::overlays::Owned::bind(editor, |editor| &mut editor.dialog, window, cx)
    });
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.pending || self.request.is_some();
        v_flex()
            .gap_3()
            .debug_selector(|| "skill-install-editor".into())
            .child(
                TabBar::new("skill-install-sources")
                    .segmented()
                    .menu(false)
                    .w_full()
                    .selected_index(self.channel.index())
                    .children(
                        [
                            Channel::Git,
                            Channel::Skills,
                            Channel::Hub,
                            Channel::Directory,
                            Channel::Archive,
                        ]
                        .into_iter()
                        .map(|channel| {
                            Tab::new()
                                .flex_1()
                                .min_w_0()
                                .label(tr(channel.label()))
                                .disabled(locked)
                        }),
                    )
                    .on_click(
                        cx.listener(|editor, index, _, cx| editor.select_channel(*index, cx)),
                    ),
            )
            .child(Alert::info("skill-install-help", tr(self.channel.help())).small())
            .when(self.channel == Channel::Git, |body| {
                body.children(
                    ["skills_repository", "skills_revision", "skills_path"]
                        .into_iter()
                        .enumerate()
                        .map(|(index, key)| {
                            Field::new().label(tr(key)).child(
                                div()
                                    .debug_selector(move || format!("skill-source-{index}"))
                                    .child(
                                        Input::new(&self.fields[index])
                                            .disabled(locked)
                                            .aria_label(tr(key)),
                                    ),
                            )
                        }),
                )
            })
            .when(
                matches!(self.channel, Channel::Skills | Channel::Hub),
                |body| {
                    let index = self.channel.index() - 1;
                    body.child(
                        Field::new().label(tr("skill_source_link")).child(
                            Input::new(&self.links[index])
                                .disabled(locked)
                                .aria_label(tr("skill_source_link")),
                        ),
                    )
                },
            )
            .when_some(self.channel.local(), |body, index| {
                body.child(
                    h_flex()
                        .min_w_0()
                        .gap_2()
                        .child(
                            Button::new("skill-import-choose")
                                .label(tr(if index == 0 {
                                    "skill_source_choose_directory"
                                } else {
                                    "skill_source_choose_archive"
                                }))
                                .disabled(locked)
                                .on_click(cx.listener(|editor, _, window, cx| {
                                    editor.choose_import(window, cx)
                                })),
                        )
                        .when_some(self.imports[index].as_ref(), |row, snapshot| {
                            row.child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(snapshot.label.clone()),
                            )
                        }),
                )
            })
            .when_some(self.discovery.as_ref(), |view, discovery| {
                view.child(
                    v_flex()
                        .id("skill-candidates")
                        .w_full()
                        .min_w_0()
                        .max_h(px(280.))
                        .overflow_y_scrollbar()
                        .child(
                            Group::new("skills_candidates")
                                .outline()
                                .heading(false)
                                .empty(IconName::Inbox, "skills_no_candidates")
                                .children(discovery.skills().iter().enumerate().map(
                                    |(index, candidate)| {
                                        let completed = self.completed.contains(&index);
                                        let installed = self
                                            .known
                                            .iter()
                                            .any(|known| known.name == candidate.name);
                                        let checked = self.selected.contains(&index);
                                        Entry::new(
                                            format!("skill-result-{index}"),
                                            v_flex()
                                                .min_w_0()
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .debug_selector(move || {
                                                            format!("skill-title-{index}")
                                                        })
                                                        .font_medium()
                                                        .whitespace_normal()
                                                        .child(candidate.skill.name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .debug_selector(move || {
                                                            format!("skill-description-{index}")
                                                        })
                                                        .text_sm()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .whitespace_normal()
                                                        .child(candidate.skill.description.clone()),
                                                )
                                                .when(installed, |view| {
                                                    view.child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(tr("skills_already_installed")),
                                                    )
                                                }),
                                        )
                                        .control(
                                            div()
                                                .debug_selector(move || {
                                                    format!("skill-candidate-{index}")
                                                })
                                                .child(
                                                    Checkbox::new(("skill-candidate", index))
                                                        .accessibility_label(
                                                            candidate.skill.name.clone(),
                                                        )
                                                        .checked(checked)
                                                        .disabled(locked || completed)
                                                        .on_click(cx.listener(
                                                            move |editor, checked, _, cx| {
                                                                if *checked {
                                                                    editor.selected.insert(index);
                                                                } else {
                                                                    editor.selected.remove(&index);
                                                                }
                                                                cx.notify();
                                                            },
                                                        )),
                                                ),
                                        )
                                    },
                                )),
                        ),
                )
            })
            .when(self.discovery.is_some(), |view| {
                view.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            rust_i18n::t!("skills_selected", count = self.selected.len())
                                .to_string(),
                        ),
                )
            })
            .when(self.exceeds_capacity(), |view| {
                view.child(
                    div().text_sm().text_color(cx.theme().danger).child(
                        rust_i18n::t!("skills_capacity", count = self.capacity()).to_string(),
                    ),
                )
            })
    }
}
