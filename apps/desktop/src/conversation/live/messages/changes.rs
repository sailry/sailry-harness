//! Completed-turn file summary with navigation to the shared Git panel.
use super::*;
use crate::conversation::surface;
use crate::feedback::Recovery as _;
use crate::{resources::checkpoints::Restore, shell::Shell};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    list::ListItem,
    tooltip::Tooltip,
};
use sailry_protocol::conversation::checkpoint::TurnDiff;

const DELTA_WIDTH: f32 = 88.;
const PREVIEW_FILES: usize = 3;

fn file_label(content: impl IntoElement, cx: &App) -> Div {
    div()
        .text_size(cx.theme().typography_tokens().sm.size)
        .child(content)
}

pub(in crate::conversation::live) struct Card {
    owner: WeakEntity<View>,
    turn: TurnId,
    data: Option<TurnDiff>,
    failed: bool,
    loading: bool,
    expanded: bool,
    task: Option<Task<()>>,
    restore: Option<Entity<Restore>>,
    reviews: std::collections::BTreeMap<String, Entity<crate::resources::hunks::Review>>,
}

impl View {
    pub(crate) fn undo_changes(
        &mut self,
        shell: WeakEntity<Shell>,
        turn: TurnId,
        path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = cx.entity().downgrade();
        let binding = self.binding();
        let Some(session) = self.session() else {
            return;
        };
        if let Some(card) = self.changes.get(&turn) {
            card.update(cx, |card, cx| {
                if card.restore.is_none() {
                    let restore = cx.new(|_| Restore::new(shell, source, binding, session, turn));
                    cx.observe(&restore, |card, _, cx| card.changed(cx))
                        .detach();
                    card.restore = Some(restore);
                }
                let restore = card.restore.as_ref().unwrap().clone();
                window.defer(cx, move |window, cx| {
                    restore.update(cx, |restore, cx| restore.start(path, window, cx))
                });
            });
        }
    }

    pub(in crate::conversation::live) fn turn_changes(
        &mut self,
        turn: TurnId,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Card>> {
        let snapshot = self.history.snapshot.as_ref()?;
        let run = snapshot.page.runs.iter().find(|run| run.turn == turn)?;
        if snapshot
            .page
            .runs
            .iter()
            .rev()
            .find(|run| run.kind == sailry_protocol::conversation::RunKind::Task)
            .map(|run| run.turn)
            != Some(turn)
            || self
                .retry
                .as_ref()
                .is_some_and(|attempt| attempt.is_submission())
            || frame::active(run.status)
            || !self.history.calls.iter().any(|call| call.turn == turn)
        {
            return None;
        }
        if let Some(card) = self.changes.get(&turn) {
            return card.read(cx).visible().then(|| card.clone());
        }
        let owner = cx.entity().downgrade();
        let card = cx.new(|cx| {
            crate::feedback::observe_with(
                window,
                cx,
                |card: &Card, _| {
                    card.failed
                        .then_some("turn_changes_failed")
                        .into_iter()
                        .collect()
                },
                |_, key, cx| {
                    let owner = cx.weak_entity();
                    gpui_kit::component::notification::Notification::error(tr(key)).scoped_action(
                        move |_, _, cx| {
                            let owner = owner.clone();
                            crate::feedback::retry("retry-changes").on_click(cx.listener(
                                move |toast, _, window, cx| {
                                    toast.dismiss(window, cx);
                                    _ = owner.update(cx, |card, cx| card.retry(cx));
                                },
                            ))
                        },
                    )
                },
            );
            let mut card = Card {
                owner,
                turn,
                data: None,
                failed: false,
                loading: false,
                expanded: false,
                task: None,
                restore: None,
                reviews: std::collections::BTreeMap::new(),
            };
            card.load(
                self.binding.clone(),
                snapshot.page.session,
                self.stop.clone(),
                cx,
            );
            card
        });
        self.changes.insert(turn, card.clone());
        card.read(cx).visible().then_some(card)
    }
}

impl Card {
    fn visible(&self) -> bool {
        self.data
            .as_ref()
            .is_some_and(|data| !data.files.is_empty() || data.partial)
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        if let Some((binding, session, stop)) = self
            .owner
            .read_with(cx, |view, _| {
                view.session
                    .as_ref()
                    .map(|session| (view.binding.clone(), session.id, view.stop.clone()))
            })
            .ok()
            .flatten()
        {
            self.load(binding, session, stop, cx);
        }
    }

    fn load(
        &mut self,
        binding: Binding,
        session: SessionId,
        stop: CancellationToken,
        cx: &mut Context<Self>,
    ) {
        let turn = self.turn;
        self.loading = true;
        self.failed = false;
        let job = binding.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                result = binding.client.execute(binding.client.prepare(Command::ReadTurnDiff { session, turn })) => Some(result),
            }
        });
        self.task = Some(cx.spawn(async move |card, cx| {
            let result = job.await;
            let _ = card.update(cx, |card, cx| {
                card.loading = false;
                match result {
                    Ok(Some(Ok(Output::TurnDiff(data))))
                        if data.session == session && data.turn == turn =>
                    {
                        card.data = Some(data)
                    }
                    _ => card.failed = true,
                }
                card.changed(cx);
            });
        }));
    }

    fn changed(&self, cx: &mut Context<Self>) {
        let _ = self.owner.update(cx, |view, cx| {
            if let Some(index) = view.rows.iter().position(|turn| *turn == self.turn) {
                view.scroller.update(cx, |scroller, cx| {
                    scroller.remeasure_items(index..index + 1, cx)
                });
            }
            cx.notify();
        });
        cx.notify();
    }

    fn open(&self, path: Option<String>, cx: &mut Context<Self>) {
        let _ = self.owner.update(cx, |view, cx| {
            if let Some(worktree) = view.turn_worktree(self.turn) {
                cx.emit(Event::GitFile(worktree, path));
            }
        });
    }

    fn review(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(review) = self.reviews.get(path) {
            review.update(cx, |review, cx| review.open(window, cx));
            return;
        }
        let Some((mut binding, session, worktree)) = self
            .owner
            .read_with(cx, |view, _| {
                if !view.connected() {
                    return None;
                }
                Some((
                    view.binding.clone(),
                    view.session()?,
                    view.turn_worktree(self.turn)?,
                ))
            })
            .ok()
            .flatten()
        else {
            return;
        };
        binding.worktree = Some(worktree);
        let path = path.to_owned();
        let review = cx.new(|_| {
            crate::resources::hunks::Review::new(
                self.owner.clone(),
                binding,
                session,
                self.turn,
                path.clone(),
            )
        });
        self.reviews.insert(path, review.clone());
        review.update(cx, |review, cx| review.open(window, cx));
    }
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let turn = self.turn;
        let Some(data) = &self.data else {
            return div().into_any_element();
        };
        if data.files.is_empty() && !data.partial {
            return div().into_any_element();
        }
        let count = data.files.len();
        let (added, removed) = data.files.iter().fold((0, 0), |(a, r), file| {
            (a + file.additions, r + file.deletions)
        });
        let header = h_flex()
            .w_full()
            .min_w_0()
            .p_3()
            .gap_3()
            .child(
                div()
                    .size_10()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(cx.theme().radius_2xl())
                    .bg(cx.theme().muted_foreground.opacity(0.12))
                    .text_color(cx.theme().muted_foreground)
                    .child(Icon::new(IconName::FileText).size_5()),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .child(rust_i18n::t!("turn_changes_count", count = count).to_string()),
                    )
                    .child(surface::diff::badge(added, removed, cx)),
            )
            .child(
                Button::new("undo-changes")
                    .outline()
                    .h_8()
                    .accessibility_label(tr("turn_changes_undo"))
                    .child(tr("turn_changes_undo"))
                    .debug_selector(move || format!("live-turn-undo-{turn}"))
                    .disabled(self.restore.as_ref().is_some_and(|state| {
                        let state = state.read(cx);
                        state.pending || state.uncertain() || state.restored(None)
                    }))
                    .on_click(cx.listener(|card, _, _, cx| {
                        let _ = card
                            .owner
                            .update(cx, |_, cx| cx.emit(Event::UndoChanges(card.turn, None)));
                    })),
            )
            .child(
                Button::new("review-changes")
                    .primary()
                    .h_8()
                    .label(tr("turn_changes_review"))
                    .debug_selector(move || format!("live-turn-review-{turn}"))
                    .on_click(cx.listener(|card, _, _, cx| card.open(None, cx))),
            );
        let visible = if self.expanded {
            count
        } else {
            count.min(PREVIEW_FILES)
        };
        // GPUI clips overflow to rectangular bounds; round the row's own paint
        // where it touches the card's bottom edge, inside the one-pixel border.
        let bottom_radius = (cx.theme().radius_2xl() - px(1.)).max(px(0.));
        let files =
            v_flex()
                .w_full()
                .min_w_0()
                .children(
                    data.files
                        .iter()
                        .take(visible)
                        .enumerate()
                        .map(|(index, file)| {
                            let path = file.path.clone();
                            let directory_end = path.rfind('/').map_or(0, |index| index + 1);
                            let tooltip_path = path.clone();
                            let hunk_path = path.clone();
                            let undo_path = path.clone();
                            let group: SharedString = format!("changed-file-{turn}-{index}").into();
                            let restored = self
                                .restore
                                .as_ref()
                                .is_some_and(|state| state.read(cx).restored(Some(&undo_path)));
                            let undo_label = tr(if restored {
                                "turn_changes_undone"
                            } else {
                                "turn_changes_undo"
                            });
                            ListItem::new(format!("diff-file-{index}"))
                                .group(group.clone())
                                .role(Role::Link)
                                .aria_label(path.clone())
                                .tab_index(0)
                                .tooltip(move |window, cx| {
                                    Tooltip::new(tooltip_path.clone()).build(window, cx)
                                })
                                .debug_selector(move || format!("live-turn-file-{turn}-{index}"))
                                .w_full()
                                .min_w_0()
                                .h_10()
                                .px_3()
                                .py_0()
                                .when(
                                    index + 1 == count && count <= PREVIEW_FILES && !data.partial,
                                    |row| row.rounded_b(bottom_radius),
                                )
                                .when(index > 0, |row| {
                                    row.border_t_1().border_color(cx.theme().border)
                                })
                                .focus_visible(|style| style.bg(cx.theme().tokens.list_hover))
                                .child(
                                    h_flex()
                                        .relative()
                                        .h_7()
                                        .w_full()
                                        .min_w_0()
                                        .child(
                                            file_label(
                                                StyledText::new(path.clone()).with_highlights([(
                                                    0..directory_end,
                                                    HighlightStyle {
                                                        color: Some(cx.theme().muted_foreground),
                                                        ..Default::default()
                                                    },
                                                )]),
                                                cx,
                                            )
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_left()
                                            .debug_selector(move || {
                                                format!("live-turn-file-label-{turn}-{index}")
                                            }),
                                        )
                                        .child(
                                            h_flex()
                                                .group_hover(group.clone(), |style| {
                                                    style.opacity(0.)
                                                })
                                                .w(px(DELTA_WIDTH))
                                                .flex_shrink_0()
                                                .justify_end()
                                                .debug_selector(move || {
                                                    format!("live-turn-delta-{turn}-{index}")
                                                })
                                                .child(surface::diff::badge(
                                                    file.additions,
                                                    file.deletions,
                                                    cx,
                                                )),
                                        )
                                        .child(
                                            Button::new(format!("review-hunks-{index}"))
                                                .absolute()
                                                .right_8()
                                                .top_0()
                                                .ghost()
                                                .small()
                                                .bg(cx.theme().tokens.list_hover)
                                                .opacity(0.)
                                                .group_hover(group.clone(), |style| {
                                                    style.opacity(1.)
                                                })
                                                .focus_visible(|style| style.opacity(1.))
                                                .icon(IconName::FileText)
                                                .tooltip(tr("hunks_review"))
                                                .accessibility_label(tr("hunks_review"))
                                                .debug_selector(move || {
                                                    format!("live-turn-hunks-{turn}-{index}")
                                                })
                                                .on_click(cx.listener(
                                                    move |card, _, window, cx| {
                                                        cx.stop_propagation();
                                                        card.review(&hunk_path, window, cx);
                                                    },
                                                )),
                                        )
                                        .child(
                                            Button::new(format!("undo-file-{index}"))
                                                .absolute()
                                                .right_0()
                                                .top_0()
                                                .ghost()
                                                .bg(cx.theme().tokens.list_hover)
                                                .small()
                                                .opacity(0.)
                                                .group_hover(group, |style| style.opacity(1.))
                                                .focus_visible(|style| style.opacity(1.))
                                                .icon(if restored {
                                                    IconName::Check
                                                } else {
                                                    IconName::Undo
                                                })
                                                .tooltip(undo_label.clone())
                                                .accessibility_label(undo_label)
                                                .debug_selector(move || {
                                                    format!("live-turn-undo-file-{turn}-{index}")
                                                })
                                                .disabled(self.restore.as_ref().is_some_and(
                                                    |state| {
                                                        let state = state.read(cx);
                                                        state.pending
                                                            || state.uncertain()
                                                            || state.restored(Some(&undo_path))
                                                    },
                                                ))
                                                .on_click(cx.listener(move |card, _, _, cx| {
                                                    cx.stop_propagation();
                                                    let _ = card.owner.update(cx, |_, cx| {
                                                        cx.emit(Event::UndoChanges(
                                                            card.turn,
                                                            Some(undo_path.clone()),
                                                        ));
                                                    });
                                                })),
                                        ),
                                )
                                .on_click(cx.listener(move |card, _, _, cx| {
                                    card.open(Some(path.clone()), cx)
                                }))
                        }),
                );
        let body = v_flex()
            .w_full()
            .min_w_0()
            .child(files)
            .when(count > PREVIEW_FILES, |body| {
                let group: SharedString = format!("live-turn-files-more-{turn}").into();
                let label: SharedString = if self.expanded {
                    tr("turn_changes_less")
                } else {
                    rust_i18n::t!("turn_changes_more", count = count - PREVIEW_FILES)
                        .to_string()
                        .into()
                };
                body.child(
                    ListItem::new("toggle-files")
                        .group(group.clone())
                        .role(Role::Button)
                        .aria_label(label.clone())
                        .aria_expanded(self.expanded)
                        .tab_index(0)
                        .w_full()
                        .min_w_0()
                        .h_10()
                        .px_3()
                        .py_0()
                        .when(!data.partial, |row| row.rounded_b(bottom_radius))
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().muted.opacity(0.2))
                        .focus_visible(|style| style.bg(cx.theme().tokens.list_hover))
                        .debug_selector(move || format!("live-turn-files-more-{turn}"))
                        .child(
                            h_flex()
                                .min_w_0()
                                .gap_2()
                                .child(file_label(label, cx).debug_selector(move || {
                                    format!("live-turn-files-more-label-{turn}")
                                }))
                                .child(crate::conversation::disclosure::caret(
                                    self.expanded,
                                    group,
                                )),
                        )
                        .on_click(cx.listener(|card, _, _, cx| {
                            card.expanded = !card.expanded;
                            card.changed(cx);
                        })),
                )
            })
            .when(data.partial, |body| {
                body.child(surface::notice(tr("tool_partial"), cx))
            });
        surface::card(Some(header.into_any_element()), body.into_any_element(), cx)
            .bg(cx.theme().muted.opacity(0.2))
            .debug_selector(move || format!("live-turn-changes-{turn}"))
            .into_any_element()
    }
}
