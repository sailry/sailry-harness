use super::{
    Candidate, Entry, State,
    changes::{Changes, Delta},
};
use crate::{
    plugins::{emblem, metadata},
    theme::DialogStyle,
    tr,
    ui::modal_list,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    spinner::Spinner,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sailry_protocol::plugin::{Info, Summary, updates::Blocked};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

pub(super) struct Selection {
    pub rows: BTreeMap<String, Row>,
    pub locked: bool,
    pub(super) window: Option<WindowId>,
}

pub(super) struct Row {
    info: Info,
    previous: Summary,
    changes: Changes,
    blocked: Option<Blocked>,
    pub selected: bool,
    pub(super) expanded: bool,
    pub status: Status,
}

impl Row {
    fn new(entry: &Entry) -> Option<Self> {
        let previous = entry.previous.as_ref()?;
        let candidate = entry.candidate.as_ref().map(Candidate::info);
        if candidate.is_none() && entry.blocked.is_none() {
            return None;
        }
        let info = candidate.unwrap_or(previous);
        Some(Self {
            info: info.clone(),
            previous: previous.summary.clone(),
            changes: Changes::between(previous, info),
            blocked: entry.blocked.clone(),
            selected: candidate.is_some(),
            expanded: false,
            status: if candidate.is_some() {
                Status::Ready
            } else {
                Status::Blocked
            },
        })
    }

    fn version(&self) -> String {
        format!(
            "{} → {}",
            self.previous.version.as_deref().unwrap_or("—"),
            if self.status == Status::Blocked {
                self.blocked
                    .as_ref()
                    .map(|blocked| blocked.version.as_str())
                    .unwrap_or("—")
            } else {
                self.info.summary.version.as_deref().unwrap_or("—")
            }
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Status {
    Ready,
    Blocked,
    Updating,
    Succeeded,
    Failed,
}

impl Candidate {
    fn info(&self) -> &Info {
        match self {
            Self::Node(info) => info,
            Self::Upload(package) => &package.info,
        }
    }
}

impl State {
    pub(in crate::settings::plugins) fn open(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows: BTreeMap<_, _> = self
            .entries
            .iter()
            .filter(|(_, entry)| !entry.manual)
            .filter_map(|(name, entry)| Row::new(entry).map(|row| (name.clone(), row)))
            .collect();
        if rows.is_empty() {
            return;
        }
        self.dialog = Some(Selection {
            rows,
            locked: false,
            window: None,
        });
        self.present(window, cx);
    }

    pub(super) fn present(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = &mut self.dialog else {
            return;
        };
        if selection.window.is_some_and(|selected| {
            cx.windows()
                .iter()
                .any(|window| window.window_id() == selected)
        }) || window.has_active_dialog(cx)
        {
            return;
        }
        selection.window = Some(window.window_handle().window_id());
        let entity = cx.entity();
        window.open_dialog(cx, move |dialog, window, cx| {
            let state = entity.read(cx);
            let selected = state.dialog.as_ref().map_or(0, |selection| {
                selection
                    .rows
                    .values()
                    .filter(|row| row.selected && row.status == Status::Ready)
                    .count()
            });
            let locked = state
                .dialog
                .as_ref()
                .is_some_and(|selection| selection.locked);
            let close = entity.clone();
            let later = entity.clone();
            let apply = entity.clone();
            dialog
                .form_title(tr("plugins_updates_title"), cx)
                .w((window.viewport_size().width - px(48.)).min(px(640.)))
                .max_h(window.viewport_size().height * 0.85)
                .overlay_closable(false)
                .close_button(!locked)
                .on_close(move |_, _, cx| {
                    close.update(cx, |state, cx| {
                        state.dialog = None;
                        state.popup.dismissed();
                        cx.notify();
                    })
                })
                .child(entity.clone())
                .footer(
                    DialogFooter::new()
                        .w_full()
                        .gap_2()
                        .child(
                            Button::new("plugins-updates-later")
                                .label(tr("plugins_updates_later"))
                                .disabled(locked)
                                .debug_selector(|| "plugins-updates-later".into())
                                .on_click(move |_, window, cx| {
                                    later.update(cx, |state, cx| {
                                        state.dialog = None;
                                        state.popup.dismissed();
                                        cx.notify();
                                    });
                                    window.close_dialog(cx);
                                }),
                        )
                        .child(
                            Button::new("plugins-updates-apply")
                                .primary()
                                .label(
                                    rust_i18n::t!("plugins_updates_selected", count = selected)
                                        .to_string(),
                                )
                                .disabled(locked || selected == 0)
                                .debug_selector(|| "plugins-updates-apply".into())
                                .on_click(move |_, window, cx| {
                                    apply.update(cx, |state, cx| state.apply(None, window, cx))
                                }),
                        ),
                )
        });
        crate::ui::overlays::Owned::bind(self, |state| &mut state.popup, window, cx);
    }
}

impl Render for State {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self
            .dialog
            .as_ref()
            .is_some_and(|selection| selection.locked);
        v_flex()
            .id("plugins-update-list")
            .text_sm()
            .w_full()
            .min_w_0()
            .gap_3()
            .debug_selector(|| "plugins-update-dialog".into())
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.binding.label.clone()),
            )
            .child(
                modal_list::List::new("plugin-update-rows").rows(
                    self.dialog
                        .iter()
                        .flat_map(|selection| &selection.rows)
                        .map(|(name, row)| {
                            let selection_name = name.clone();
                            let expand_name = name.clone();
                            let retry_name = name.clone();
                            let retryable = self.batch.as_ref().map_or_else(
                                || self.entries.contains_key(name),
                                |batch| {
                                    batch
                                        .entries
                                        .front()
                                        .is_some_and(|entry| entry.current.name == *name)
                                },
                            );
                            let interactive = !locked && row.status == Status::Ready;
                            let notes = row.expanded.then(|| {
                                v_flex()
                                    .w_full()
                                    .gap_2()
                                    .when(row.status != Status::Blocked, |view| {
                                        view.when_some(
                                            row.info.release_notes.as_ref(),
                                            |view, notes| {
                                                view.child(
                                                    div().font_medium().child(
                                                        row.info
                                                            .summary
                                                            .version
                                                            .clone()
                                                            .unwrap_or_else(|| "—".into()),
                                                    ),
                                                )
                                                .child(super::super::notes::view(notes))
                                            },
                                        )
                                    })
                                    .when_some(row.blocked.as_ref(), |view, blocked| {
                                        view.when_some(
                                            blocked.release_notes.as_ref(),
                                            |view, notes| {
                                                view.child(
                                                    div()
                                                        .font_medium()
                                                        .child(blocked.version.clone()),
                                                )
                                                .child(super::super::notes::view(notes))
                                            },
                                        )
                                    })
                                    .when(row.status != Status::Blocked, |view| {
                                        view.child(
                                            div()
                                                .text_sm()
                                                .font_medium()
                                                .child(tr("plugins_update_changes")),
                                        )
                                    })
                                    .children(
                                        [
                                            ("plugins_update_tools", &row.changes.tools, false),
                                            ("plugins_update_skills", &row.changes.skills, false),
                                            (
                                                "plugins_update_settings",
                                                &row.changes.settings,
                                                false,
                                            ),
                                            ("plugins_update_actions", &row.changes.actions, true),
                                        ]
                                        .into_iter()
                                        .filter(|(_, delta, _)| !delta.empty())
                                        .map(
                                            |(key, delta, permissions)| {
                                                delta_view(key, delta, permissions, cx)
                                            },
                                        ),
                                    )
                            });
                            let blocked = row.blocked.as_ref().map(|blocked| {
                                h_flex()
                                    .w_full()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!(
                                                "{} · {}",
                                                blocked.version,
                                                super::super::compatibility::message(
                                                    &blocked.required,
                                                    &self.binding,
                                                    cx
                                                )
                                            )),
                                    )
                                    .children(super::super::compatibility::action(
                                        SharedString::from(format!("plugin-update-sailry-{name}")),
                                        &self.binding,
                                        cx,
                                    ))
                            });
                            let details = (blocked.is_some() || notes.is_some()).then(|| {
                                v_flex().w_full().gap_3().children(blocked).children(notes)
                            });
                            let status = match row.status {
                                Status::Updating => Some(Spinner::new().small().into_any_element()),
                                Status::Succeeded => Some(
                                    Icon::new(IconName::CircleCheck)
                                        .text_color(cx.theme().success)
                                        .into_any_element(),
                                ),
                                Status::Failed => Some(
                                    h_flex()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            Icon::new(IconName::CircleX)
                                                .text_color(cx.theme().danger),
                                        )
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "plugin-update-retry-{name}"
                                            )))
                                            .ghost()
                                            .icon(Icon::empty().path("icons/rotate-ccw.svg"))
                                            .tooltip(tr("plugins_retry"))
                                            .accessibility_label(tr("plugins_retry"))
                                            .disabled(!retryable || self.applying())
                                            .debug_selector({
                                                let name = name.clone();
                                                move || format!("plugin-update-retry-{name}")
                                            })
                                            .on_click(cx.listener(move |state, _, window, cx| {
                                                if let Some(batch) = &state.batch {
                                                    if batch.entries.front().is_some_and(|entry| {
                                                        entry.current.name == retry_name
                                                    }) {
                                                        state.send(window, cx);
                                                    }
                                                } else {
                                                    if let Some(row) = state
                                                        .dialog
                                                        .as_mut()
                                                        .and_then(|selection| {
                                                            selection.rows.get_mut(&retry_name)
                                                        })
                                                    {
                                                        row.selected = true;
                                                    }
                                                    state.apply(Some(&retry_name), window, cx);
                                                }
                                            })),
                                        )
                                        .into_any_element(),
                                ),
                                Status::Ready | Status::Blocked => None,
                            };
                            let expand = Button::new(SharedString::from(format!(
                                "plugin-update-expand-{name}"
                            )))
                            .ghost()
                            .icon(if row.expanded {
                                IconName::ChevronUp
                            } else {
                                IconName::ChevronDown
                            })
                            .tooltip(tr("plugins_update_changes"))
                            .accessibility_label(tr("plugins_update_changes"))
                            .debug_selector({
                                let name = name.clone();
                                move || format!("plugin-update-expand-{name}")
                            })
                            .on_click(cx.listener(
                                move |state, _, _, cx| {
                                    if let Some(row) = state
                                        .dialog
                                        .as_mut()
                                        .and_then(|selection| selection.rows.get_mut(&expand_name))
                                    {
                                        row.expanded = !row.expanded;
                                    }
                                    cx.notify();
                                },
                            ));
                            modal_list::Row::new(
                                format!("plugin-update-select-{name}"),
                                metadata::title(&row.info),
                            )
                            .subtitle(row.version())
                            .leading(emblem::grid(
                                name,
                                row.info
                                    .extension
                                    .as_ref()
                                    .and_then(|extension| extension.tint),
                                emblem::glyph(&row.info),
                                row.info.icon.as_deref(),
                                px(32.),
                            ))
                            .checkbox(row.selected)
                            .when_some(status, |item, status| item.trailing(status))
                            .trailing(expand)
                            .when_some(details, |item, details| item.details(details))
                            .when(interactive, |item| {
                                item.on_click(cx.listener(move |state, _, _, cx| {
                                    if let Some(row) = state.dialog.as_mut().and_then(|selection| {
                                        selection.rows.get_mut(&selection_name)
                                    }) {
                                        row.selected = !row.selected;
                                    }
                                    cx.notify();
                                }))
                            })
                        }),
                ),
            )
    }
}

fn delta_view(key: &'static str, delta: &Delta, permissions: bool, cx: &App) -> AnyElement {
    v_flex()
        .gap_1()
        .text_sm()
        .debug_selector(move || key.into())
        .when(permissions, |view| {
            view.rounded_md().bg(cx.theme().warning.opacity(0.12)).p_2()
        })
        .child(div().font_medium().child(tr(key)))
        .children(delta.added.iter().map(|name| {
            div().child(rust_i18n::t!("plugins_update_added", name = name).to_string())
        }))
        .children(delta.removed.iter().map(|name| {
            div()
                .text_color(cx.theme().muted_foreground)
                .child(rust_i18n::t!("plugins_update_removed", name = name).to_string())
        }))
        .into_any_element()
}
