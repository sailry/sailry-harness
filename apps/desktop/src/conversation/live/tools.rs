use super::*;
use crate::conversation::{
    disclosure::{self, Detail},
    surface::{self, diff},
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    collapsible::Collapsible,
};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_client::conversation::tools::{Call, State};
use sailry_protocol::conversation::{ApprovalState, Page};

mod content;
mod inspector;
pub(super) mod output;
pub(super) mod progress;
mod projection;

impl View {
    pub(super) fn tools(
        &self,
        calls: &[&Call],
        continuing: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
        let grouped = projection::group(self, calls, page, cx);
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
        if grouped.is_none()
            && calls
                .iter()
                .any(|call| call.raw_result(page) || activity_path(call).is_some())
            && !calls.iter().any(|call| has_content(call, page))
        {
            return self.tool_rows(calls, continuing, cx);
        }
        if (grouped.is_none() && !calls.iter().any(|call| has_content(call, page)))
            || grouped.as_ref().is_some_and(|group| group.count == 0)
        {
            return self.activity(
                calls[0],
                page,
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
                .on_click(
                    cx.listener(move |view, _, _, cx| view.expand(turn, key.clone(), !open, cx)),
                ),
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
                if !has_content(call, page) {
                    return self.activity(call, page, format!("live-{turn}-{key}"), summary, cx);
                }
                let has_result = has_content(call, page);
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
                                view.expand(turn, key.clone(), !open, cx);
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
            if let Some(content) = &call.content
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
                                surface::notice(error, cx).debug_selector(move || selector.clone()),
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

    fn copy_result(&self, key: &str, text: String) -> AnyElement {
        let selector = format!("live-tool-copy-{key}");
        h_flex()
            .debug_selector(move || selector.clone())
            .child(surface::copy(format!("{key}-copy"), text))
            .into_any_element()
    }

    fn activity(
        &self,
        call: &Call,
        page: &Page,
        id: String,
        summary: Summary,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let group = SharedString::from(format!("{id}-actions"));
        let mut actions = Vec::new();
        if let Some(path) = activity_path(call) {
            actions.push(self.path_link(&id, call.turn, path.to_owned().into(), cx));
        }
        if call.raw_result(page) {
            actions.push(self.raw_action(call, &id, cx));
        }
        h_flex()
            .group(group.clone())
            .min_w_0()
            .gap_2()
            .child(disclosure::summary(
                id.clone(),
                summary.icon,
                summary.label,
                summary.detail,
                cx,
            ))
            .when(!actions.is_empty(), |row| {
                row.child(surface::Actions {
                    id: format!("{id}-controls").into(),
                    group,
                    actions,
                    absolute: false,
                })
            })
            .into_any_element()
    }

    fn preparing_summary(&self, mut summary: Summary, calls: &[&Call], page: &Page) -> Summary {
        if let Some(stage) = preparation(calls, page, &self.python) {
            summary.detail.state = Some(crate::settings::python_stage(stage));
        }
        summary
    }

    fn path_link(
        &self,
        key: &str,
        turn: TurnId,
        path: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let target = path.clone();
        let worktree = self.turn_worktree(turn);
        Button::new(format!("{key}-path"))
            .ghost()
            .xsmall()
            .icon(IconName::PanelRightOpen)
            .tooltip(path.clone())
            .px_0()
            .min_w_0()
            .accessibility_label(path.clone())
            .on_click(cx.listener(move |_, _, _, cx| {
                if let Some(worktree) = worktree {
                    cx.emit(Event::FileAt(worktree, target.to_string()));
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
        let copy = body.copy_text();
        if !copy.is_empty() {
            actions.push(self.copy_result(key, copy));
        }
        if let Some(path) = path {
            actions.push(self.path_link(key, call.turn, path, cx));
        }
        let notices = v_flex().children(
            body.notices
                .iter()
                .map(|notice| surface::notice(tr(notice), cx)),
        );
        let content = if fault.is_some() && !body.text.is_empty() {
            let selector = format!("live-tool-error-{key}");
            v_flex().child(surface::notice(body.text, cx).debug_selector(move || selector.clone()))
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
        surface::result(key, content.into_any_element(), actions, cx).into_any_element()
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
    let display = call.display(page);
    let icon = display
        .and_then(|display| display.icon.as_ref())
        .map(crate::assets::icons::icon)
        .or_else(|| surface::tool_icon(&call.name))
        .unwrap_or_else(|| {
            Icon::new(if call.progress.is_some() {
                IconName::CircleCheck
            } else {
                IconName::Settings2
            })
        });
    let target = call
        .arguments(page)
        .and_then(|arguments| arguments["path"].as_str())
        .and_then(|value| value.lines().next())
        .filter(|value| !value.is_empty())
        .map(|value| SharedString::from(value.to_owned()))
        .unwrap_or_default();
    let denied = call
        .approval
        .as_ref()
        .is_some_and(|approval| approval.state == ApprovalState::Denied);
    let failure_code = call.failure.as_ref().and_then(|failure| failure.code);
    let failed = output::is_error(call.failure.as_ref(), call.result(page));
    let state = if denied {
        Some(approvals::label(ApprovalState::Denied))
    } else if let Some(code) = failure_code
        && matches!(
            code,
            sailry_protocol::ErrorCode::Cancelled | sailry_protocol::ErrorCode::OutcomeUnknown
        )
    {
        Some(if code == sailry_protocol::ErrorCode::Cancelled {
            "turn_cancelled"
        } else {
            "tool_interrupted"
        })
    } else if failed {
        Some("tool_failed")
    } else if call
        .approval
        .as_ref()
        .is_some_and(|approval| approval.state == ApprovalState::Pending)
    {
        Some("approval_pending")
    } else {
        match call.state {
            State::Waiting | State::Running => None,
            State::Cancelled => Some("turn_cancelled"),
            State::NotExecuted => Some("tool_not_executed"),
            State::Interrupted => Some("tool_interrupted"),
            State::Returned => None,
        }
    };
    let declared = call
        .resolved
        .as_ref()
        .and_then(|resolved| resolved.input.summary.as_ref())
        .and_then(|field| Some((field, field.text.lines().next()?)))
        .map(|(field, text)| {
            if field.code {
                Detail::mono(SharedString::from(text.to_owned()))
            } else {
                Detail::plain(SharedString::from(text.to_owned()))
            }
        });
    let mut detail = declared.unwrap_or_else(|| Detail::plain(target));
    detail.secondary = true;
    detail.state = if call.state == State::Returned && !denied {
        detail.state.or_else(|| state.map(tr))
    } else {
        state.map(tr)
    };
    detail.failed = !denied && failed;
    detail.state_icon = matches!(
        state,
        Some("approval_rejected" | "turn_cancelled" | "tool_interrupted" | "tool_not_executed")
    )
    .then_some(IconName::CircleX);
    detail.hide_caret = !has_content(call, page);
    detail.pending = call
        .approval
        .as_ref()
        .is_some_and(|approval| approval.state == ApprovalState::Pending);
    detail.running = call.state == State::Running
        && !denied
        && !failed
        && call
            .approval
            .as_ref()
            .is_none_or(|approval| approval.state != ApprovalState::Pending);
    if detail.running {
        detail.state = None;
    }
    Summary {
        icon,
        label: display
            .map(|display| SharedString::from(display.label(&rust_i18n::locale()).to_owned()))
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

fn has_content(call: &Call, page: &Page) -> bool {
    call.details(page)
}

fn activity_path(call: &Call) -> Option<&str> {
    let resolved = call.resolved.as_ref()?;
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

/// While parallel calls run, show the latest active call; otherwise show the last result.
fn group_summary(calls: &[&Call], page: &Page) -> Summary {
    let mut summaries: Vec<_> = calls.iter().map(|call| summary(call, page)).collect();
    let pending = summaries.iter().any(|item| item.detail.pending);
    let running = summaries.iter().any(|item| item.detail.running);
    let failed = !pending && !running && summaries.iter().any(|item| item.detail.failed);
    let index = summaries
        .iter()
        .rposition(|item| item.detail.pending || item.detail.running)
        .unwrap_or(summaries.len() - 1);
    let mut latest = summaries.remove(index);
    latest.detail.pending = pending;
    latest.detail.running = running;
    let latest_failed = latest.detail.failed;
    latest.detail.failed = failed;
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
        let skill = call.arguments(page)?.get("skill")?.as_str()?;
        let (package, _) = skill.split_once(':')?;
        python.package(package).map(|progress| progress.stage)
    })
}
