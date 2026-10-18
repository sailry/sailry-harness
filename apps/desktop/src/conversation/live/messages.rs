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
mod grounding;
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
        cx: &mut Context<Self>,
    ) -> crate::content::markdown::View {
        self.markdown_at(id, text, self.binding.worktree, false, cx)
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
                                let _ =
                                    owner.update(cx, |_, cx| cx.emit(Event::Link(link.clone())));
                            }),
                    )
                    .into_any_element()
            })
            .on_link_click(move |link, _, cx| {
                let _ = owner.update(cx, |_, cx| cx.emit(Event::Link(link.clone())));
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
        let automatic = run.continuation.is_some() || run.notice;

        let mut prompt = String::new();
        let mut sent_ms = None;
        let mut attachments = Vec::new();
        let mut references = Vec::new();
        for entry in snapshot
            .page
            .entries
            .iter()
            .filter(|entry| entry.turn == turn)
        {
            if entry.author == "user" {
                sent_ms.get_or_insert(entry.timestamp_ms);
                for part in &entry.parts {
                    if let Part::Text(text) = part {
                        prompt.push_str(text);
                    } else if let Part::Attachment(attachment) = part {
                        attachments.push(attachments::images::ImageSource::from(attachment));
                    } else if let Part::Reference(reference) = part {
                        references.push(reference.clone());
                    }
                }
            }
        }
        if sent_ms.is_none()
            && let Some(input) = sailry_client::conversation::turns::input(&snapshot.page, turn)
        {
            prompt = input.message.text;
            references = input.message.references;
            attachments = input
                .attachments
                .iter()
                .map(attachments::images::ImageSource::from)
                .collect();
        }
        // Admission and canonical user content arrive independently. Keep the
        // local prompt in its admitted turn until the user event replaces it.
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
        let blocks = blocks::collect(turn, &self.history);
        let user_footer = self.user_footer(turn, &prompt, sent_ms, cx);
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
                        true,
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
                            self.markdown(format!("{id}-text"), text.into(), cx),
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
                Block::Question(call) => {
                    self.question_history(call, &self.history.snapshot.as_ref().unwrap().page, cx)
                }
                Block::Resource => div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("chat_resource_pending"))
                    .into_any_element(),
                Block::Retry(attempt, limit) => {
                    let active = retrying && index == last_block;
                    let label = rust_i18n::t!(
                        if active {
                            "chat_model_reconnecting"
                        } else {
                            "chat_model_retried"
                        },
                        attempt = attempt,
                        limit = limit
                    )
                    .to_string();
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .debug_selector(move || format!("live-model-retry-{turn}-{attempt}"))
                        .text_color(cx.theme().muted_foreground)
                        .child(Icon::new(IconName::Network).size_4())
                        .child(if active {
                            ShimmerText::new(label)
                                .id(format!("{turn}-retry-shimmer"))
                                .into_any_element()
                        } else {
                            div().child(label).into_any_element()
                        })
                        .into_any_element()
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
                    .children(self.background_notices(turn, &run.notices, cx))
                    .when(
                        !automatic
                            && (!prompt.is_empty()
                                || !attachments.is_empty()
                                || !references.is_empty()),
                        |group| {
                            group.child(
                                Message::new()
                                    .alignment(MessageAlignment::End)
                                    .footer(user_footer)
                                    .content(self.user_content(
                                        turn,
                                        prompt,
                                        &references,
                                        attachments,
                                        cx,
                                    )),
                            )
                        },
                    )
                    .child(
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
                    ),
            )
            .into_any_element()
    }
}

pub(crate) fn failure_key(error: &sailry_protocol::Fault) -> &'static str {
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
    let (code, category) = model_failure(&error.message).unwrap_or_default();
    match code {
        "model.openai_responses.unauthorized" | "model.openai_responses.forbidden" => {
            return "chat_model_unauthorized";
        }
        "model.openai_responses.rate_limited" => return "chat_model_rate_limited",
        "model.openai_responses.subscription_sharing_usage_limit_exceeded" => {
            return "chat_subscription_limit";
        }
        "model.openai_responses.subscription_sharing_usage_unavailable" => {
            return "chat_subscription_unavailable";
        }
        "model.openai_responses.parse" => return "chat_response_invalid",
        "model.response_timeout" => return "chat_response_timeout",
        _ => {}
    }
    match category {
        Some("unauthorized" | "forbidden") => "chat_model_unauthorized",
        Some("rate_limited") => "chat_model_rate_limited",
        Some("timeout") => "chat_response_timeout",
        _ if error.code == sailry_protocol::ErrorCode::Unavailable => "chat_model_failed",
        _ if error.code == sailry_protocol::ErrorCode::OutcomeUnknown => "chat_execution_unknown",
        _ => super::actions::error_key(error),
    }
}

/// Splits Node's public model diagnostic into the ADK code and optional HTTP category.
fn model_failure(message: &str) -> Option<(&str, Option<&str>)> {
    let (code, rest) = message
        .lines()
        .next()?
        .strip_prefix("Agent execution failed (")?
        .split_once(')')?;
    let category = rest
        .strip_prefix("; HTTP ")
        .and_then(|rest| rest.split_once("; "))
        .map(|(_, category)| category);
    Some((code, category))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use sailry_protocol::{ErrorCode, Fault};

    #[test]
    fn classifies_response_failures() {
        for (code, status, key) in [
            ("unauthorized", 401, "chat_model_unauthorized"),
            ("forbidden", 403, "chat_model_unauthorized"),
            ("rate_limited", 429, "chat_model_rate_limited"),
        ] {
            let message =
                format!("Agent execution failed (model.openai_responses.{code}); HTTP {status}");
            assert_eq!(
                failure_key(&Fault::new(ErrorCode::Unavailable, message.clone())),
                key
            );
            assert_eq!(
                failure_key(&Fault::new(
                    ErrorCode::Unavailable,
                    format!("{message}\n\n{{\"message\":\"Provider explanation\"}}")
                )),
                key
            );
        }
        for (message, key) in [
            (
                "Agent execution failed (model.openai_responses.subscription_sharing_usage_limit_exceeded)",
                "chat_subscription_limit",
            ),
            (
                "Agent execution failed (model.openai_responses.subscription_sharing_usage_unavailable)",
                "chat_subscription_unavailable",
            ),
        ] {
            for message in [
                message.to_string(),
                format!("{message}; HTTP 429; rate_limited"),
            ] {
                assert_eq!(
                    failure_key(&Fault::new(ErrorCode::Unavailable, message)),
                    key
                );
            }
        }
        assert_eq!(
            failure_key(&Fault::new(
                ErrorCode::Unavailable,
                "Agent execution failed (model.openai_responses.parse)"
            )),
            "chat_response_invalid"
        );
        for (category, key) in [
            ("unauthorized", "chat_model_unauthorized"),
            ("forbidden", "chat_model_unauthorized"),
            ("rate_limited", "chat_model_rate_limited"),
            ("timeout", "chat_response_timeout"),
            ("internal", "chat_model_failed"),
        ] {
            let message = format!(
                "Agent execution failed (model.openai_compat.api_error); HTTP 400; {category}"
            );
            assert_eq!(
                failure_key(&Fault::new(ErrorCode::Unavailable, message)),
                key
            );
        }
        assert_eq!(
            failure_key(&Fault::new(ErrorCode::Unavailable, "unavailable")),
            "chat_model_failed"
        );
        assert_eq!(
            failure_key(&Fault::new(ErrorCode::OutcomeUnknown, "unknown")),
            "chat_execution_unknown"
        );
    }
}
