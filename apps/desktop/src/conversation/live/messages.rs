use super::*;
use gpui_kit::component::{
    bubble::{Bubble, BubbleVariant},
    button::{Button, ButtonVariants},
    clipboard::Clipboard,
    collapsible::Collapsible,
    message::{
        Message, MessageAlignment, MessageContent, MessageFooter, MessageGroup, MessageHeader,
    },
    separator::Separator,
    shimmer::ShimmerText,
};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_protocol::conversation::Part;

mod blocks;
pub(super) mod changes;
mod citations;
mod compaction;
mod document;
mod files;
mod frame;
pub(super) use frame::elapsed;
mod grounding;
mod retry;
mod user;
mod work;
use blocks::Block;
pub(super) use user::{Editing, sent_input};

impl View {
    pub(super) fn user_content(
        &self,
        turn: TurnId,
        prompt: String,
        references: &[sailry_protocol::conversation::reference::Reference],
        attachments: Vec<super::attachments::images::ImageSource>,
        cx: &mut Context<Self>,
    ) -> MessageContent {
        let editing = self.editing_turn(turn);
        MessageContent::new()
            .when(!attachments.is_empty(), |content| {
                content.child(
                    div()
                        .min_w_0()
                        .w_full()
                        .max_w(px(crate::conversation::layout::USER_MESSAGE_WIDTH))
                        .debug_selector(move || format!("live-user-attachments-{turn}"))
                        .child(super::attachments::source_links(
                            attachments,
                            &self.binding,
                            &self.images,
                            MessageAlignment::End,
                            cx,
                        )),
                )
            })
            .when(
                !prompt.is_empty() || !references.is_empty() || editing,
                |content| {
                    let bubble = Bubble::new()
                        .with_variant(BubbleVariant::Muted)
                        .max_w(px(crate::conversation::layout::USER_MESSAGE_WIDTH))
                        .when(editing, |bubble| bubble.w_full())
                        .content(
                            crate::theme::messages::user(cx)
                                .when(editing, |content| content.w_full())
                                .child(
                                    div()
                                        .debug_selector(move || format!("live-user-bubble-{turn}"))
                                        .when(
                                            prompt.is_empty() && !references.is_empty(),
                                            |content| {
                                                content.child(
                                                    self.sent_references(turn, references, cx),
                                                )
                                            },
                                        )
                                        .when(!prompt.is_empty() || editing, |content| {
                                            content
                                                .child(self.user_text(turn, prompt, references, cx))
                                        }),
                                ),
                        );
                    content.bubble(bubble)
                },
            )
    }

    pub(super) fn markdown(
        &self,
        id: String,
        text: String,
        turn: TurnId,
        cx: &mut Context<Self>,
    ) -> crate::content::markdown::View {
        self.markdown_at(id, text, self.turn_worktree(turn), false, cx)
    }

    fn markdown_at(
        &self,
        id: String,
        text: String,
        worktree: Option<WorktreeId>,
        answer: bool,
        cx: &mut Context<Self>,
    ) -> crate::content::markdown::View {
        let owner = cx.entity().downgrade();
        let images = self.images.clone();
        let root = self
            .node
            .snapshot
            .as_ref()
            .and_then(|node| {
                node.worktrees
                    .iter()
                    .find(|entry| Some(entry.id) == worktree)
            })
            .map(|entry| entry.path.clone());
        let image_owner = owner.clone();
        let selection_owner = owner.clone();
        let file_owner = owner.clone();
        let file_root = root.clone();
        let file_id = id.clone();
        let image_id = id.clone();
        let image_binding = {
            let mut binding = self.binding.clone();
            binding.worktree = worktree;
            binding
        };
        let mut file_binding = self.binding.clone();
        file_binding.worktree = worktree;
        // Keep parsed content and selection when virtual rows leave the viewport.
        // History updates remeasure the list; pointer selection must not do so.
        let state = self
            .texts
            .borrow_mut()
            .entry(id)
            .or_insert_with(|| cx.new(|cx| crate::content::markdown::State::new(text.clone(), cx)))
            .clone();
        state.update(cx, |state, cx| state.set_source(text, cx));
        let mut typography = crate::content::markdown::typography::Typography::of(cx);
        typography.caption = typography.body;
        if answer {
            typography.body = typography.body.with_leading(24. / 14.);
        }
        crate::content::markdown::View::new(&state)
            .link_hover_underline(true)
            .link_underline(false)
            .foreground(crate::theme::messages::foreground("assistant", cx))
            .strong_color(answer.then_some(cx.theme().primary))
            .typography(
                typography.scaled(f32::from(self.message_text_size) / typography.body.size()),
            )
            .on_selection(move |excerpt, _, cx| {
                _ = selection_owner.update(cx, |view, cx| view.select_excerpt(excerpt, cx));
            })
            .on_file(move |index, url, label, _| {
                let file = crate::content::files::from_link(url, label, file_root.as_deref()?)?;
                let source = file_owner.upgrade()?;
                Some(
                    crate::content::files::Card {
                        id: format!("{file_id}-file-{index}-{url}").into(),
                        file,
                        binding: file_binding.clone(),
                        source,
                    }
                    .into_any_element(),
                )
            })
            .on_image(move |url, urls, position, cx| {
                use crate::content::images::{self, ImageSource};
                use crate::conversation::file_reference::{self, Target};
                let workspace = |url: &str| match file_reference::resolve(url, root.as_deref()?)? {
                    Target::Workspace { path, .. } => Some(path),
                    _ => None,
                };
                if let (Some(worktree), Some(path)) = (worktree, workspace(url))
                    && images::displayable(&path)
                {
                    let source = ImageSource::File {
                        worktree,
                        path,
                        context: None,
                    };
                    if !images::Images::failed(&images, &source, cx) {
                        let mut index = 0;
                        let gallery: Vec<_> = urls
                            .iter()
                            .enumerate()
                            .filter_map(|(image_index, url)| {
                                let path =
                                    workspace(url).filter(|path| images::displayable(path))?;
                                if image_index < position {
                                    index += 1;
                                }
                                Some(ImageSource::File {
                                    worktree,
                                    path,
                                    context: None,
                                })
                            })
                            .collect();
                        let selector = format!("image-card-{}", source.id());
                        return div()
                            .max_w_full()
                            .debug_selector(move || selector.clone())
                            .child(images::card(
                                &source,
                                &images,
                                gallery,
                                index,
                                urls.len() > 1,
                                cx,
                            ))
                            .into_any_element();
                    }
                }
                if url.starts_with("https://") || url.starts_with("http://") {
                    return gpui_kit::img(SharedString::from(url.to_owned()))
                        .max_w_full()
                        .into_any_element();
                }
                if let Some(file) = root
                    .as_deref()
                    .and_then(|root| crate::content::files::from_image(url, "", root))
                    && let Some(source) = image_owner.upgrade()
                {
                    return crate::content::files::Card {
                        id: format!("{image_id}-image-{url}").into(),
                        file,
                        binding: image_binding.clone(),
                        source,
                    }
                    .into_any_element();
                }
                let link = SharedString::from(url.to_owned());
                let owner = image_owner.clone();
                div()
                    .flex()
                    .self_start()
                    .max_w_full()
                    .child(
                        Button::new(format!("image-source-{url}"))
                            .link()
                            .label(url.to_owned())
                            .on_click(move |_, _, cx| {
                                let _ = owner.update(cx, |_, cx| {
                                    cx.emit(Event::Link(worktree, link.clone()));
                                });
                            }),
                    )
                    .into_any_element()
            })
            .on_link_click(move |link, _, cx| {
                let _ = owner.update(cx, |_, cx| {
                    cx.emit(Event::Link(worktree, link.clone()));
                });
            })
    }

    pub(super) fn message(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(turn) = self.rows.get(index).copied() else {
            return div().into_any_element();
        };
        if self
            .outgoing
            .as_ref()
            .is_some_and(|message| message.row == turn)
            && self.history.snapshot.as_ref().is_none_or(|snapshot| {
                !snapshot.page.runs.iter().any(|run| {
                    run.turn == turn && run.status != Status::Queued && run.input.is_some()
                })
            })
        {
            return self.outgoing_message(cx);
        }
        let footers = self
            .turn_changes(turn, window, cx)
            .map(IntoElement::into_any_element);
        let Some(snapshot) = &self.history.snapshot else {
            return div().into_any_element();
        };
        let Some(run) = snapshot.page.runs.iter().find(|run| run.turn == turn) else {
            return div().into_any_element();
        };
        if run.kind == sailry_protocol::conversation::RunKind::Compaction {
            return self.compaction_turn(run, &snapshot.page, cx);
        }
        let users: Vec<_> = sailry_client::conversation::turns::group(&snapshot.page, turn)
            .into_iter()
            .filter(|run| run.continuation.is_none() && !run.notice)
            .filter_map(|run| {
                let turn = run.turn;
                let sent_ms = snapshot
                    .page
                    .entries
                    .iter()
                    .find(|entry| entry.turn == turn && entry.author == "user")
                    .map(|entry| entry.timestamp_ms);
                let input = sailry_client::conversation::turns::input(&snapshot.page, turn);
                let mut prompt = input
                    .as_ref()
                    .map(|input| input.message.text.clone())
                    .unwrap_or_default();
                let mut references = input
                    .as_ref()
                    .map(|input| input.message.references.clone())
                    .unwrap_or_default();
                let mut attachments: Vec<_> = input
                    .as_ref()
                    .into_iter()
                    .flat_map(|input| &input.attachments)
                    .map(attachments::images::ImageSource::from)
                    .collect();
                // Admission and canonical user content arrive independently.
                if sent_ms.is_none()
                    && run.input.is_none()
                    && let Some(outgoing) = self
                        .outgoing
                        .as_ref()
                        .filter(|outgoing| outgoing.turn == Some(turn))
                        .or_else(|| self.sent.get(&turn))
                {
                    prompt = outgoing.input.text.clone();
                    references = outgoing.input.references.clone();
                    attachments = outgoing.attachments.clone();
                }
                if prompt.is_empty() && attachments.is_empty() && references.is_empty() {
                    return None;
                }
                Some(
                    Message::new()
                        .alignment(MessageAlignment::End)
                        .footer(self.user_footer(turn, &prompt, sent_ms, cx))
                        .content(self.user_content(turn, prompt, &references, attachments, cx)),
                )
            })
            .collect();
        let blocks = blocks::collect(turn, &self.history);
        let phase = self
            .history
            .connected
            .then(|| frame::phase(run, &snapshot.page, &blocks))
            .flatten();
        let continuing_tools =
            self.history.connected && frame::continuing_tools(run, &snapshot.page, &blocks);
        let blocks = if self.message_display == crate::preferences::MessageDisplay::Compact {
            blocks::compact(blocks)
        } else {
            blocks
        };
        let copy = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Text(_, text) => Some(text.to_string()),
                Block::Search(entry) => Some(grounding::answer(entry)),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let retrying = frame::active(run.status) && self.history.connected;
        let last_block = blocks.len().saturating_sub(1);
        let recovered = retry::recovered(&blocks);
        let answer_from = work::answer_from(&blocks);
        let work_open = self.work_open(turn, frame::active(run.status));
        let rows = blocks
            .into_iter()
            .enumerate()
            .map(|(index, block)| match block {
                Block::Text(key, text) => div()
                    .w_full()
                    .min_w_0()
                    .debug_selector(move || format!("live-turn-text-{turn}"))
                    .child(self.markdown_at(
                        format!("{turn}-{key}"),
                        citations::presentation(&text).into_owned(),
                        self.turn_worktree(turn),
                        index >= answer_from,
                        cx,
                    ))
                    .into_any_element(),
                Block::Search(entry) => grounding::button(entry, cx),
                Block::Images(images) => attachments::image_links(
                    &images.into_iter().cloned().collect::<Vec<_>>(),
                    snapshot.page.session,
                    &self.binding,
                    &self.images,
                    cx,
                ),
                Block::Thinking(key, text) => {
                    let id = format!("{turn}-{key}");
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
                            crate::conversation::disclosure::trigger(
                                id.clone(),
                                IconName::Bot,
                                tr("turn_reasoning"),
                                crate::conversation::disclosure::Detail::default(),
                                open,
                                cx,
                            )
                            .on_click(cx.listener(
                                move |view, _, _, cx| view.expand(turn, key.clone(), !open, cx),
                            )),
                        )
                        .content(crate::conversation::surface::thought(
                            id.clone(),
                            self.markdown(format!("{id}-text"), text.into(), turn, cx),
                        ))
                        .into_any_element()
                }
                Block::Tools(calls) => {
                    self.tools(&calls, continuing_tools && index == last_block, cx)
                }
                Block::Children(children) => {
                    self.child_summary(turn, &children, continuing_tools && index == last_block, cx)
                }
                Block::Compaction(key, text) => self.compaction(turn, key, text, cx),
                Block::CompactionFailure(key, failure) => {
                    self.compaction_failure(turn, key, &failure, cx)
                }
                Block::Question(call) => {
                    self.question_history(call, &self.history.snapshot.as_ref().unwrap().page, cx)
                }
                Block::Resource => div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("chat_resource_pending"))
                    .into_any_element(),
                Block::Retry(key, retry) => {
                    let recovered = recovered.contains(&key);
                    self.recovery(
                        turn,
                        key,
                        retry,
                        recovered,
                        retrying && index == last_block,
                        cx,
                    )
                }
            })
            .collect();
        let content = MessageContent::new()
            .gap(cx.theme().spacing_tokens().lg)
            .children(crate::conversation::surface::work(
                rows,
                answer_from,
                work_open,
                cx,
            ));
        let content = content
            .when_some(phase, |content, phase| {
                content.child(
                    h_flex()
                        .id(format!("{turn}-phase"))
                        .debug_selector(move || format!("live-turn-phase-{turn}-{phase}"))
                        .gap_2()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .aria_label(tr(phase))
                        .child(crate::ui::thinking::render(
                            format!("live-turn-avatar-{turn}"),
                            phase,
                            crate::conversation::disclosure::ICON_SIZE
                                .to_pixels(cx.theme().font_size),
                        ))
                        .child(
                            ShimmerText::new(tr(phase))
                                .id(format!("{turn}-thinking"))
                                .into_any_element(),
                        ),
                )
            })
            .children(self.published_files(turn, cx))
            .children(footers.into_iter().map(|footer| {
                div()
                    .w_full()
                    .min_w_0()
                    .pt(cx.theme().spacing_tokens().sm)
                    .child(footer)
            }))
            .when_some(run.error.as_ref(), |content, error| {
                let open = self
                    .expanded
                    .get(&(turn, "error".into()))
                    .copied()
                    .unwrap_or(false);
                content.child(
                    Collapsible::new()
                        .open(open)
                        .w_full()
                        .child(
                            crate::conversation::disclosure::trigger(
                                format!("live-turn-error-{turn}"),
                                IconName::TriangleAlert,
                                tr(failure_key(error)),
                                crate::conversation::disclosure::Detail::default()
                                    .state(None, true),
                                open,
                                cx,
                            )
                            .on_click(cx.listener(
                                move |view, _, _, cx| {
                                    view.expand(turn, "error".into(), !open, cx);
                                },
                            )),
                        )
                        .content(crate::conversation::disclosure::body(
                            crate::conversation::surface::result(
                                &format!("{turn}-error"),
                                div()
                                    .id(format!("{turn}-error-scroll"))
                                    .debug_selector(move || {
                                        format!("live-turn-error-reason-{turn}")
                                    })
                                    .text_sm()
                                    .min_w_0()
                                    .max_h_40()
                                    .overflow_y_scroll()
                                    .child(gpui_kit::base::SelectableText::new(
                                        format!("{turn}-error-text"),
                                        error.message.clone(),
                                    ))
                                    .into_any_element(),
                                Vec::<AnyElement>::new(),
                                cx,
                            ),
                            cx,
                        )),
                )
            });
        let status = frame::status(run, self.history.connected);
        let elapsed = (self.history.connected || !frame::active(run.status))
            .then(|| frame::elapsed(run, chrono::Utc::now().timestamp_millis()))
            .flatten();
        let label = elapsed.as_ref().map_or_else(
            || tr(status),
            |elapsed| format!("{} {elapsed}", tr(status)).into(),
        );
        let status_row = h_flex()
            .id(format!("turn-status-{turn}"))
            .gap_2()
            .child(tr(status))
            .children(elapsed);
        let header = if answer_from > 0 {
            crate::conversation::surface::trigger(
                format!("live-turn-work-{turn}"),
                label,
                status_row,
                work_open,
                cx,
            )
            .on_click(cx.listener(move |view, _, _, cx| {
                view.expand(turn, "work".to_owned(), !work_open, cx)
            }))
            .into_any_element()
        } else {
            status_row.into_any_element()
        };
        let footer = MessageFooter::new().content_inset(false).w_full().child(
            h_flex()
                .id(format!("{turn}-footer"))
                .w_full()
                .group(format!("footer-{turn}"))
                .debug_selector(move || format!("live-turn-footer-{turn}"))
                .gap_2()
                .when(!copy.is_empty(), |row| {
                    row.child(
                        Clipboard::new(format!("{turn}-copy"))
                            .value(copy.clone())
                            .tooltip(tr("turn_copy")),
                    )
                })
                .when(!copy.is_empty(), |row| {
                    let id = format!("{turn}-read");
                    let active = crate::reader::active(&id, cx);
                    row.child(
                        Button::new(id.clone())
                            .ghost()
                            .small()
                            .icon(if active {
                                gpui_kit::assets::IconName::Square
                            } else {
                                gpui_kit::assets::IconName::Volume2
                            })
                            .tooltip(tr(if active { "reader_stop" } else { "reader_read" }))
                            .accessibility_label(tr(if active {
                                "reader_stop"
                            } else {
                                "reader_read"
                            }))
                            .debug_selector(move || format!("live-turn-read-{turn}"))
                            .on_click(move |_, window, cx| {
                                crate::reader::toggle(id.clone(), &copy, window, cx)
                            }),
                    )
                })
                .when(!self.readonly() && !run.notice, |row| {
                    row.child(self.turn_menu(turn, cx))
                })
                .when(
                    !self.readonly() && !run.notice && super::resend::resendable(run.status),
                    |row| row.child(self.resend_button(turn, cx)),
                )
                .when(!frame::active(run.status), |row| {
                    row.child(self.export_menu(turn, cx))
                })
                .when_some(frame::finished(run), |row, time| {
                    let full = frame::full_time(run).unwrap_or_else(|| time.clone());
                    row.child(
                        h_flex()
                            .id(format!("{turn}-timestamp"))
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .opacity(0.)
                            .group_hover(format!("footer-{turn}"), |style| style.opacity(1.))
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(full.clone())
                                    .build(window, cx)
                            })
                            .child(time),
                    )
                }),
        );
        div()
            .id(format!("{turn}-row"))
            .debug_selector(move || format!("live-turn-{turn}"))
            .w_full()
            .min_w_0()
            .max_w(px(super::super::CONTENT_WIDTH))
            .mx_auto()
            .px(if self.sidebar { px(12.) } else { px(24.) })
            .child(
                MessageGroup::new()
                    .gap(px(10.))
                    .pb(px(28.))
                    .children(users)
                    .when(run.joined.is_none(), |group| {
                        group.child(
                            Message::new()
                                .header(
                                    MessageHeader::new().content_inset(false).w_full().child(
                                        v_flex()
                                            .debug_selector(move || {
                                                format!("live-turn-status-{turn}-{status}")
                                            })
                                            .text_sm()
                                            .w_full()
                                            .gap_2()
                                            .child(header)
                                            .child(Separator::horizontal()),
                                    ),
                                )
                                .content(crate::theme::messages::assistant(content, cx))
                                .when(!frame::active(run.status), |message| message.footer(footer)),
                        )
                    }),
            )
            .into_any_element()
    }
}

pub(crate) fn failure_key(error: &sailry_protocol::Fault) -> &'static str {
    match error.code {
        sailry_protocol::ErrorCode::Cancelled => return "turn_cancelled",
        sailry_protocol::ErrorCode::OutcomeUnknown => return "chat_execution_unknown",
        _ => {}
    }
    if let Some(sailry_protocol::FaultDetails::ModelRecovery { reason }) = &error.details {
        return retry::stop_key(*reason);
    }
    if let Some(sailry_protocol::FaultDetails::SystemDiagnostic(report)) = &error.details {
        use sailry_protocol::notification::DiagnosticReason;
        if report.reason == DiagnosticReason::Cancelled {
            return "turn_cancelled";
        }
        if report.reason == DiagnosticReason::Timeout {
            return "plugin_response_timeout";
        }
        return "plugin_callback_failed";
    }
    if let Some(sailry_protocol::FaultDetails::ModelRequest { reason }) = &error.details {
        use sailry_protocol::ModelFailure;
        return match reason {
            ModelFailure::Request => "chat_model_failed",
            ModelFailure::Unauthorized => "chat_model_unauthorized",
            ModelFailure::RateLimited => "chat_model_rate_limited",
            ModelFailure::Timeout => "chat_response_timeout",
            ModelFailure::InvalidResponse => "chat_response_invalid",
            ModelFailure::SharedUsageLimit => "chat_subscription_limit",
            ModelFailure::SharedUsageUnavailable => "chat_subscription_unavailable",
        };
    }
    match error.message.as_str() {
        "context summary timed out" => return "chat_compact_timeout",
        "context summary did not reduce the conversation" => return "chat_compact_no_reduction",
        "context summary exceeds the size limit" => return "chat_compact_too_large",
        "context summary returned non-text output" => return "chat_response_invalid",
        "context summary was not completed" | "context summary stream ended before completion" => {
            return "chat_compact_incomplete";
        }
        _ => {}
    }
    if error.code == sailry_protocol::ErrorCode::Unavailable {
        return "turn_failed";
    }
    super::actions::error_key(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use sailry_protocol::{ErrorCode, Fault, FaultDetails, ModelFailure};

    #[test]
    fn classifies_model_failures() {
        for (reason, key) in [
            (ModelFailure::Request, "chat_model_failed"),
            (ModelFailure::Unauthorized, "chat_model_unauthorized"),
            (ModelFailure::RateLimited, "chat_model_rate_limited"),
            (ModelFailure::Timeout, "chat_response_timeout"),
            (ModelFailure::InvalidResponse, "chat_response_invalid"),
            (ModelFailure::SharedUsageLimit, "chat_subscription_limit"),
            (
                ModelFailure::SharedUsageUnavailable,
                "chat_subscription_unavailable",
            ),
        ] {
            let mut fault = Fault::new(ErrorCode::Unavailable, "Provider explanation");
            fault.details = Some(FaultDetails::ModelRequest { reason });
            assert_eq!(failure_key(&fault), key);
            assert_eq!(fault.message, "Provider explanation");
        }
    }

    #[test]
    fn leaves_untyped_failures_generic() {
        for message in [
            "unavailable",
            "Max iterations (100) exceeded",
            "Agent execution failed (model.openai_responses.unauthorized); HTTP 401",
        ] {
            assert_eq!(
                failure_key(&Fault::new(ErrorCode::Unavailable, message)),
                "turn_failed"
            );
        }
    }
}
