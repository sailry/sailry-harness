use super::*;
use crate::conversation::{
    disclosure::{self, Detail},
    surface::{self, diff},
};
use gpui_kit::component::{button::ButtonVariants as _, collapsible::Collapsible};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_client::conversation::tools::{Call, RowState, State};
use sailry_protocol::conversation::{ApprovalState, Page};

mod content;
mod details;
mod inspector;
pub(super) mod output;
pub(super) mod progress;
mod projection;
#[cfg(test)]
mod tests;

impl View {
    pub(super) fn tools(
        &self,
        calls: &[&Call],
        continuing: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(snapshot) = &self.history.snapshot else {
            return v_flex().into_any_element();
        };
        let hydrated: Vec<_> = calls
            .iter()
            .map(|call| self.detailed(call, &snapshot.page))
            .collect();
        let calls: Vec<_> = hydrated.iter().collect();
        let calls = calls.as_slice();
        let Some(first) = calls.first() else {
            return v_flex().into_any_element();
        };
        let sequence = calls.len() > 1
            && !calls
                .iter()
                .all(|call| call.name == first.name && call.progress.is_none());
        if !sequence {
            return self.tool_groups(calls, continuing, cx);
        }
        let page = &self.history.snapshot.as_ref().unwrap().page;
        let summary = self
            .preparing_summary(group_summary(calls, page), calls, page)
            .continuing(continuing);
        let turn = first.turn;
        let key = format!("tool-sequence-{}", first.source.key());
        let content_id = format!("live-{turn}-{key}-content");
        let open = self
            .expanded
            .get(&(turn, key.clone()))
            .copied()
            .unwrap_or(false);
        Collapsible::new()
            .open(open)
            .w_full()
            .min_w_0()
            .child(
                disclosure::trigger(
                    format!("live-{turn}-{key}"),
                    summary.icon,
                    summary.label,
                    summary.detail,
                    open,
                    cx,
                )
                .on_click(
                    cx.listener(move |view, _, _, cx| view.expand(turn, key.clone(), !open, cx)),
                ),
            )
            .content(
                disclosure::body(
                    v_flex().when(open, |body| {
                        body.child(self.tool_groups(calls, continuing, cx))
                    }),
                    cx,
                )
                .py_2()
                .debug_selector(move || content_id.clone()),
            )
            .into_any_element()
    }

    fn tool_groups(&self, calls: &[&Call], continuing: bool, cx: &mut Context<Self>) -> AnyElement {
        let mut groups = Vec::new();
        let mut start = 0;
        while start < calls.len() {
            let end = if calls[start].progress.is_some() {
                start + 1
            } else {
                start
                    + calls[start..]
                        .iter()
                        .take_while(|call| call.name == calls[start].name)
                        .count()
            };
            let group = &calls[start..end];
            groups.push(
                if group.len() == 1
                    && !projection::grouped(group[0], &self.history.snapshot.as_ref().unwrap().page)
                {
                    self.tool_rows(group, continuing && end == calls.len(), cx)
                } else {
                    self.tool_group(group, continuing && end == calls.len(), cx)
                },
            );
            start = end;
        }
        let content = v_flex().w_full().min_w_0().gap_2().children(groups);
        content.into_any_element()
    }

    fn tool_group(&self, calls: &[&Call], continuing: bool, cx: &mut Context<Self>) -> AnyElement {
        let page = &self.history.snapshot.as_ref().unwrap().page;
        let first = calls[0];
        let grouped = projection::group(self, calls, page, cx).filter(|group| group.count > 0);
        let turn = first.turn;
        let key = format!("tool-group-{}", first.source.key());
        let summary = self
            .preparing_summary(group_summary(calls, page), calls, page)
            .continuing(continuing);
        let open = self
            .expanded
            .get(&(turn, key.clone()))
            .copied()
            .unwrap_or(false);
        let detail = summary.detail;
        if !expandable(calls, page) {
            return self.activity(
                calls[0],
                format!("live-{turn}-{key}"),
                Summary {
                    icon: summary.icon,
                    label: summary.label,
                    detail,
                },
                cx,
            );
        }
        let content = if open {
            match grouped {
                Some(group) => group.content,
                None => self.tool_rows(calls, continuing, cx),
            }
        } else {
            v_flex().into_any_element()
        };
        let content_id = format!("live-{turn}-{key}-content");
        let references: Vec<_> = calls
            .iter()
            .filter_map(|call| call.detail_reference(page))
            .collect();
        Collapsible::new()
            .open(open)
            .w_full()
            .min_w_0()
            .child(
                disclosure::trigger(
                    format!("live-{turn}-{key}"),
                    summary.icon.clone(),
                    summary.label.clone(),
                    detail,
                    open,
                    cx,
                )
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.expand_tools(turn, key.clone(), !open, references.clone(), cx)
                })),
            )
            .content(
                disclosure::body(content, cx)
                    .py_2()
                    .debug_selector(move || content_id.clone()),
            )
            .into_any_element()
    }

    fn tool_rows(&self, calls: &[&Call], continuing: bool, cx: &mut Context<Self>) -> AnyElement {
        let page = &self.history.snapshot.as_ref().unwrap().page;
        v_flex()
            .w_full()
            .min_w_0()
            .gap_2()
            .children(calls.iter().enumerate().map(|(index, call)| {
                let continuing = continuing && index + 1 == calls.len();
                let key = format!("tools-{}", call.source.key());
                let turn = call.turn;
                if call.progress.is_some() {
                    return self.tool(call, page, continuing, cx);
                }
                let summary = self
                    .preparing_summary(summary(call, page), &[call], page)
                    .continuing(continuing);
                if !call.details(page) {
                    return self.activity(call, format!("live-{turn}-{key}"), summary, cx);
                }
                let has_result = call.details(page);
                let references = call.detail_reference(page).into_iter().collect::<Vec<_>>();
                let open = has_result
                    && self
                        .expanded
                        .get(&(turn, key.clone()))
                        .copied()
                        .unwrap_or(false);
                Collapsible::new()
                    .open(open)
                    .w_full()
                    .min_w_0()
                    .child(
                        disclosure::trigger(
                            format!("live-{turn}-{key}"),
                            summary.icon,
                            summary.label,
                            summary.detail,
                            open,
                            cx,
                        )
                        .on_click(cx.listener(move |view, _, _, cx| {
                            if has_result {
                                view.expand_tools(turn, key.clone(), !open, references.clone(), cx);
                            }
                        })),
                    )
                    .content(
                        disclosure::body(
                            v_flex().when(open, |body| {
                                body.child(self.tool(call, page, continuing, cx))
                            }),
                            cx,
                        )
                        .pt_1()
                        .pb_2(),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    fn tool(
        &self,
        call: &Call,
        page: &Page,
        continuing: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = format!("{}-{}", call.turn, call.source.key());
        if let Some(progress) = &call.progress {
            return progress::render(
                &key,
                progress,
                continuing || call.state == State::Running,
                self.history.connected && progress::active(page, call.turn),
                cx,
            );
        }
        if call.detail.is_none() && call.details(page) {
            let references = call.detail_reference(page).into_iter().collect::<Vec<_>>();
            self.request_details(references, false, cx);
            return h_flex()
                .p_2()
                .child(self.copy_result(&key, call, page, cx))
                .children(
                    call.detail_reference(page)
                        .filter(|reference| {
                            matches!(
                                self.details.state(reference),
                                sailry_client::conversation::details::State::Loading
                                    | sailry_client::conversation::details::State::Failed {
                                        retry_at: Some(_),
                                        ..
                                    }
                            )
                        })
                        .map(|_| gpui_kit::component::spinner::Spinner::new()),
                )
                .into_any_element();
        }
        let approval = call.approval.as_ref().map(|approval| approval.state);
        let denied = approval == Some(ApprovalState::Denied);
        let result = call.result(page).filter(|_| !denied);
        let arguments = call.arguments(page);
        let path = arguments
            .and_then(|arguments| arguments["path"].as_str())
            .filter(|path| !path.is_empty())
            .map(|path| SharedString::from(path.to_owned()));
        let card = result.map(|result| {
            let selector = format!("live-tool-result-{key}");
            let images = call.images(page);
            if let Some(content) = call.content()
                && output::failure(call.failure.as_ref(), Some(result)).is_none()
            {
                return div()
                    .id(selector.clone())
                    .test_support()
                    .min_w_0()
                    .debug_selector(move || selector.clone())
                    .child(self.structured(&key, call, page, content, result, cx))
                    .into_any_element();
            }
            if !images.is_empty()
                && call.presentation != sailry_protocol::tool::Presentation::Content
            {
                return v_flex()
                    .id(selector.clone())
                    .test_support()
                    .min_w_0()
                    .gap_2()
                    .debug_selector(move || selector.clone())
                    .child(attachments::image_links(
                        images,
                        page.session,
                        &self.binding,
                        &self.images,
                        cx,
                    ))
                    .when_some(
                        output::failure(call.failure.as_ref(), Some(result)),
                        |column, error| {
                            let selector = format!("live-tool-error-{key}");
                            column.child(
                                surface::notice(error, cx)
                                    .when_some(outcome_tone(call, result, cx), |notice, tone| {
                                        notice.text_color(tone)
                                    })
                                    .debug_selector(move || selector.clone()),
                            )
                        },
                    )
                    .into_any_element();
            }
            let body = if output::fault(call.failure.as_ref(), Some(result)).is_some() {
                self.generic(&key, call, path.clone(), result, cx)
            } else if let Some(body) = projection::render(self, &key, call, page, result, cx) {
                body
            } else if let Some(progress) = &call.progress {
                progress::render(
                    &key,
                    progress,
                    continuing || call.state == State::Running,
                    self.history.connected && progress::active(page, call.turn),
                    cx,
                )
            } else {
                self.generic(&key, call, path.clone(), result, cx)
            };
            v_flex()
                .id(selector.clone())
                .test_support()
                .min_w_0()
                .debug_selector(move || selector.clone())
                .child(body)
                .when(!images.is_empty(), |column| {
                    column.child(attachments::image_links(
                        images,
                        page.session,
                        &self.binding,
                        &self.images,
                        cx,
                    ))
                })
                .into_any_element()
        });
        v_flex()
            .min_w_0()
            .gap_1()
            .map(|column| match card {
                Some(card) => column.child(card),
                None => column,
            })
            .into_any_element()
    }

    fn copy_result(
        &self,
        key: &str,
        call: &Call,
        page: &Page,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let reference = call.detail_reference(page);
        let selector = format!("live-tool-copy-{key}");
        // Kit's Clipboard value_fn is synchronous. A Kit button keeps the same
        // control while the shared Client loads every detail page before copying.
        gpui_kit::component::button::Button::new(selector.clone())
            .icon(IconName::Copy)
            .ghost()
            .xsmall()
            .tooltip(tr("tool_copy"))
            .accessibility_label(tr("tool_copy"))
            .disabled(reference.is_none())
            .on_click(cx.listener(move |view, _, _, cx| {
                cx.stop_propagation();
                if let Some(reference) = reference.clone() {
                    view.load_detail(reference, details::Action::Copy, cx);
                }
            }))
            .into_any_element()
    }

    fn activity(
        &self,
        call: &Call,
        id: String,
        mut summary: Summary,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        summary.detail.hide_caret = true;
        if let Some(path) = activity_path(call) {
            let node = self.binding.client.target();
            let turn = call.turn;
            let worktree = self.turn_worktree(turn);
            let path = path.to_owned();
            let reference = self
                .history
                .snapshot
                .as_ref()
                .and_then(|snapshot| call.detail_reference(&snapshot.page));
            let loaded = call.detail.is_some();
            return disclosure::trigger(id, summary.icon, summary.label, summary.detail, false, cx)
                .w_full()
                .on_click(cx.listener(move |view, _, _, cx| {
                    if view.binding.client.target() == node
                        && view.turn_worktree(turn) == worktree
                        && let Some(worktree) = worktree
                    {
                        if !loaded && let Some(reference) = reference.clone() {
                            view.load_detail(reference, details::Action::Open(worktree), cx);
                        } else {
                            cx.emit(Event::FileAt(worktree, path.clone()));
                        }
                    }
                }))
                .into_any_element();
        }
        disclosure::summary(id, summary.icon, summary.label, summary.detail, cx).into_any_element()
    }

    fn preparing_summary(&self, mut summary: Summary, calls: &[&Call], page: &Page) -> Summary {
        if let Some(stage) = preparation(calls, page, &self.python) {
            summary.detail.state = Some(crate::settings::python_stage(stage));
        }
        summary
    }

    fn path_result(
        &self,
        key: &str,
        turn: TurnId,
        path: Option<SharedString>,
        content: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(path) = path else { return content };
        let node = self.binding.client.target();
        let worktree = self.turn_worktree(turn);
        gpui_kit::base::Button::new(format!("{key}-open"))
            .w_full()
            .min_w_0()
            .items_start()
            .justify_start()
            .line_height(relative(1.5))
            .accessibility_label(path.clone())
            .child(content)
            .on_click(cx.listener(move |view, event, _, cx| {
                if !stationary(event) {
                    return;
                }
                if view.binding.client.target() == node
                    && view.turn_worktree(turn) == worktree
                    && let Some(worktree) = worktree
                {
                    cx.emit(Event::FileAt(worktree, path.to_string()));
                }
            }))
            .into_any_element()
    }

    fn generic(
        &self,
        key: &str,
        call: &Call,
        path: Option<SharedString>,
        result: &serde_json::Value,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fault = output::failure(call.failure.as_ref(), Some(result));
        let body = output::tool_preview(call.failure.as_ref(), result);
        let mut actions = Vec::new();
        let copy = self
            .history
            .snapshot
            .as_ref()
            .and_then(|snapshot| inspector::copy(call, &snapshot.page))
            .unwrap_or_else(|| body.copy_text());
        if !copy.is_empty() {
            if let Some(snapshot) = &self.history.snapshot {
                actions.push(self.copy_result(key, call, &snapshot.page, cx));
            }
        }
        let tone = outcome_tone(call, result, cx);
        let notices = v_flex().children(body.notices.iter().map(|notice| {
            surface::notice(tr(notice), cx).when_some(tone, |notice, tone| notice.text_color(tone))
        }));
        let content = if fault.is_some() && !body.text.is_empty() {
            let selector = format!("live-tool-error-{key}");
            v_flex().child(
                surface::notice(body.text, cx)
                    .when_some(tone, |notice, tone| notice.text_color(tone))
                    .debug_selector(move || selector.clone()),
            )
        } else if body.text.is_empty() {
            v_flex().child(notices)
        } else {
            v_flex()
                .child(surface::code(
                    &format!("{key}-output"),
                    &body.text,
                    px(f32::from(self.message_text_size)),
                    cx,
                ))
                .child(notices)
        };
        self.path_result(
            key,
            call.turn,
            path,
            surface::result(key, content.into_any_element(), actions, cx).into_any_element(),
            cx,
        )
    }
}

struct Summary {
    icon: Icon,
    label: SharedString,
    detail: Detail,
}

impl Summary {
    fn continuing(mut self, continuing: bool) -> Self {
        if continuing && !self.detail.pending {
            self.detail.running = true;
        }
        self
    }
}

fn summary(call: &Call, page: &Page) -> Summary {
    let row = call.row(page);
    let icon = row
        .icon
        .as_ref()
        .map(crate::assets::icons::icon)
        .or_else(|| surface::tool_icon(&call.name))
        .unwrap_or_else(|| {
            Icon::new(if call.progress.is_some() {
                IconName::CircleCheck
            } else {
                IconName::Settings2
            })
        });
    let mut detail = row
        .detail
        .as_ref()
        .map(|field| {
            if field.code {
                Detail::mono(SharedString::from(field.text.clone()))
            } else {
                Detail::plain(SharedString::from(field.text.clone()))
            }
        })
        .unwrap_or_default();
    detail.secondary = true;
    detail.state = row.state.map(|state| {
        tr(match state {
            RowState::Denied => "approval_rejected",
            RowState::Cancelled => "turn_cancelled",
            RowState::Interrupted => "tool_interrupted",
            RowState::Failed => "tool_failed",
            RowState::Pending => "approval_pending",
            RowState::NotExecuted => "tool_not_executed",
        })
    });
    detail.failed = row.failed;
    detail.warning = row.warning;
    detail.hide_caret = !row.has_details;
    detail.pending = row.pending;
    detail.running = row.running;
    Summary {
        icon,
        label: row
            .label
            .as_ref()
            .map(|label| SharedString::from(label.label(&rust_i18n::locale()).to_owned()))
            .unwrap_or_else(|| {
                if call.progress.is_some() {
                    tr("task_progress")
                } else {
                    surface::tool_label(&call.name)
                }
            }),
        detail,
    }
}

fn expandable(calls: &[&Call], page: &Page) -> bool {
    calls.len() > 1 || calls.iter().any(|call| call.details(page))
}

fn activity_path(call: &Call) -> Option<&str> {
    let resolved = call.resolved()?;
    resolved
        .output
        .as_ref()
        .filter(|output| output.paths.len() == 1)
        .map(|output| output.paths[0].as_str())
        .or_else(|| {
            resolved
                .input
                .target
                .as_ref()
                .map(|target| target.text.as_str())
        })
        .filter(|path| !path.is_empty())
}

fn stationary(event: &ClickEvent) -> bool {
    if let ClickEvent::Mouse(mouse) = event {
        let moved = mouse.up.position - mouse.down.position;
        return moved.x.abs() <= px(3.) && moved.y.abs() <= px(3.);
    }
    true
}

fn outcome_tone(call: &Call, result: &serde_json::Value, cx: &App) -> Option<Hsla> {
    call.failure.as_ref()?;
    if output::expected(call.failure.as_ref(), Some(result)).is_some() {
        return None;
    }
    Some(if output::is_error(call.failure.as_ref(), Some(result)) {
        cx.theme().danger
    } else {
        cx.theme().warning
    })
}

/// Keep the last tool's title while aggregating the group's outcome.
fn group_summary(calls: &[&Call], page: &Page) -> Summary {
    let mut summaries: Vec<_> = calls.iter().map(|call| summary(call, page)).collect();
    let pending = summaries.iter().any(|item| item.detail.pending);
    let running = summaries.iter().any(|item| item.detail.running);
    let failed = !pending && !running && summaries.iter().any(|item| item.detail.failed);
    let warning =
        !pending && !running && !failed && summaries.iter().any(|item| item.detail.warning);
    let mut latest = summaries.pop().unwrap();
    latest.detail.pending = pending;
    latest.detail.running = running;
    let latest_failed = latest.detail.failed;
    latest.detail.failed = failed;
    latest.detail.warning = warning;
    latest.detail.hide_caret = false;
    if failed && !latest_failed {
        latest.detail.state = Some(tr("tool_failed"));
    }
    latest
}

fn preparation(
    calls: &[&Call],
    page: &Page,
    python: &sailry_client::python::View,
) -> Option<sailry_protocol::python::Stage> {
    calls.iter().rev().find_map(|call| {
        if call.name != "load_skill" || call.state != State::Running {
            return None;
        }
        let skill = call.summary(page)?.resource.as_deref()?;
        let (package, _) = skill.split_once(':')?;
        python.package(package).map(|progress| progress.stage)
    })
}
