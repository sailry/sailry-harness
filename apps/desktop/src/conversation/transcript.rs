use super::turn::{Block, Status, Turn};
use crate::{shell::Shell, tr};
use gpui_kit::component::{
    bubble::{Bubble, BubbleContent, BubbleVariant},
    clipboard::Clipboard,
    collapsible::Collapsible,
    message::{
        Message, MessageAlignment, MessageContent, MessageFooter, MessageGroup, MessageHeader,
    },
    separator::Separator,
    shimmer::ShimmerText,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
mod user;

// Block indices use 0..len; this slot belongs to the enclosing Work disclosure.
const WORK: usize = usize::MAX;

fn answer_from(blocks: &[Block]) -> usize {
    let boundary = blocks
        .iter()
        .position(|block| {
            matches!(
                block,
                Block::Approval(_) | Block::Interaction(_) | Block::Error(_)
            )
        })
        .unwrap_or(blocks.len());
    blocks[..boundary]
        .iter()
        .rposition(|block| {
            matches!(
                block,
                Block::Reasoning(_) | Block::Tools(_) | Block::Subagents(_)
            )
        })
        .map_or(0, |index| index + 1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Location {
    pub session: (usize, usize),
    pub turn: usize,
    pub child: Option<super::subagent::Key>,
}

impl Location {
    pub fn main(session: (usize, usize), turn: usize) -> Self {
        Self {
            session,
            turn,
            child: None,
        }
    }

    fn id(self) -> String {
        let id = format!("turn-{}-{}-{}", self.session.0, self.session.1, self.turn);
        match self.child {
            Some(child) => format!("{id}-child-{}-{}", child.id, child.generation),
            None => id,
        }
    }

    fn selector(self, part: &str, suffix: Option<usize>) -> String {
        let mut id = match self.child {
            Some(child) => format!("child-{}-{}-{part}", child.id, child.generation),
            None if part.is_empty() => format!("turn-{}", self.turn),
            None => format!("turn-{part}-{}", self.turn),
        };
        if let Some(suffix) = suffix {
            id.push_str(&format!("-{suffix}"));
        }
        id
    }
}

impl Shell {
    pub(super) fn turn_view(
        &self,
        key: (usize, usize),
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.transcript_view(Location::main(key, index), cx)
    }

    pub(crate) fn transcript_turn(&self, location: Location) -> Option<&Turn> {
        let turn = self
            .conversations
            .get(&location.session)?
            .turns
            .get(location.turn)?;
        match location.child {
            Some(child) => turn
                .subagents()
                .find(|agent| agent.key == child)
                .map(|agent| &agent.turn),
            None => Some(turn),
        }
    }

    pub(crate) fn transcript_view(&self, location: Location, cx: &mut Context<Self>) -> AnyElement {
        let Some(turn) = self.transcript_turn(location) else {
            return div().into_any_element();
        };
        let id = location.id();
        let answer_from = answer_from(&turn.blocks);
        let work_open = self
            .transcript_expanded(location)
            .get(&WORK)
            .copied()
            .unwrap_or(turn.status.active());
        let rows = turn
            .blocks
            .iter()
            .enumerate()
            .map(|(block, content)| {
                let block_id = format!("{id}-{block}");
                match content {
                    Block::Subagents(agents) => self.subagent_summary(location, agents, cx),
                    Block::Approval(request) => self.approval_summary(block_id, request, cx),
                    Block::Interaction(request) => self.interaction_summary(block_id, request, cx),
                    Block::Error(text) => div()
                        .id(block_id)
                        .role(Role::Alert)
                        .w_full()
                        .debug_selector(move || location.selector("error", None))
                        .text_sm()
                        .text_color(cx.theme().warning)
                        .child(text.clone())
                        .into_any_element(),
                    Block::Text(text) => div()
                        .w_full()
                        .min_w_0()
                        .debug_selector(move || location.selector("text", Some(block)))
                        .child(self.linked_message(block_id, text.clone(), cx))
                        .into_any_element(),
                    Block::Reasoning(text) => self.turn_group(
                        location,
                        block,
                        tr("turn_reasoning"),
                        IconName::Bot,
                        super::surface::thought(
                            block_id.clone(),
                            self.linked_message(block_id, text.clone(), cx),
                        )
                        .into_any_element(),
                        cx,
                    ),
                    Block::Tools(calls) => {
                        let content = v_flex()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .children(calls.iter().enumerate().map(|(call, tool)| {
                                let title = h_flex()
                                    .min_w_0()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_color(match tool.status {
                                                Status::Completed => cx.theme().success,
                                                Status::Failed => cx.theme().danger,
                                                _ => cx.theme().muted_foreground,
                                            })
                                            .child(Icon::new(status_icon(tool.status)).size_3()),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .child(super::surface::tool_label(&tool.name)),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .font_family(cx.theme().mono_font_family.clone())
                                            .debug_selector(move || {
                                                location.selector("tool-link", Some(call))
                                            })
                                            .child(
                                                self.linked_message(
                                                    format!("{block_id}-{call}-target"),
                                                    format!("[{}]({})", tool.target, tool.target)
                                                        .into(),
                                                    cx,
                                                ),
                                            ),
                                    );
                                if tool.result.is_empty() {
                                    return div().px_1().text_xs().child(title).into_any_element();
                                }
                                v_flex()
                                    .child(title)
                                    .child(super::surface::result(
                                        &format!("{block_id}-{call}"),
                                        div()
                                            .px_3()
                                            .py_2()
                                            .child(self.linked_message(
                                                format!("{block_id}-{call}-result"),
                                                tool.result.clone(),
                                                cx,
                                            ))
                                            .into_any_element(),
                                        [],
                                        cx,
                                    ))
                                    .into_any_element()
                            }))
                            .into_any_element();
                        self.turn_group(
                            location,
                            block,
                            rust_i18n::t!("turn_tools_count", count = calls.len())
                                .to_string()
                                .into(),
                            status_icon(
                                calls
                                    .iter()
                                    .find(|call| call.status != Status::Completed)
                                    .map_or(Status::Completed, |call| call.status),
                            ),
                            content,
                            cx,
                        )
                    }
                }
            })
            .collect();
        let content = MessageContent::new()
            .gap(cx.theme().spacing_tokens().lg)
            .children(super::surface::work(rows, answer_from, work_open, cx));
        let content = content.when(turn.status == Status::Cancelled, |content| {
            content.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("turn_cancelled_by_user")),
            )
        });
        let footer = if turn.status.active() {
            MessageFooter::new().content_inset(false).child(
                h_flex()
                    .id(format!("{id}-status"))
                    .debug_selector(move || location.selector("phase", None))
                    .gap_2()
                    .aria_label(tr(turn.status.phase_label()))
                    .child(
                        div()
                            .debug_selector(move || location.selector("orb", None))
                            .child(crate::ui::thinking::render(
                                format!("{id}-orb"),
                                turn.status.phase_label(),
                                px(20.),
                            )),
                    )
                    .when(matches!(turn.status, Status::Running(_)), |row| {
                        row.child(
                            ShimmerText::new(tr(turn.status.phase_label()))
                                .id(format!("{id}-phase")),
                        )
                    })
                    .when(!matches!(turn.status, Status::Running(_)), |row| {
                        row.child(tr(turn.status.phase_label()))
                    }),
            )
        } else {
            MessageFooter::new().content_inset(false).w_full().child(
                h_flex()
                    .id(format!("{id}-footer"))
                    .w_full()
                    .group(format!("{id}-footer"))
                    .debug_selector(move || location.selector("footer", None))
                    .gap_2()
                    .when_some(turn.finished_at.clone(), |row, time| {
                        row.child(
                            div()
                                .id(format!("{id}-time"))
                                .opacity(0.)
                                .group_hover(format!("{id}-footer"), |style| style.opacity(1.))
                                .child(time),
                        )
                    })
                    .when(!turn.copy_text().is_empty(), |row| {
                        row.child(
                            div()
                                .id(format!("{id}-copy-label"))
                                .debug_selector(move || location.selector("copy", None))
                                .aria_label(tr("turn_copy"))
                                .child(
                                    Clipboard::new(format!("{id}-copy"))
                                        .tooltip(tr("turn_copy"))
                                        .value(turn.copy_text()),
                                ),
                        )
                    }),
            )
        };
        let label: SharedString =
            format!("{} {}", tr(turn.status.label()), turn.duration_label()).into();
        let status_row = h_flex()
            .w_full()
            .gap_2()
            .child(label.clone())
            .child(div().ml_auto().child(tr("turn_preview")));
        let header = if answer_from > 0 {
            super::surface::trigger(
                location.selector("work", None),
                label,
                status_row,
                work_open,
                cx,
            )
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.expand_transcript_group(location, WORK, !work_open, cx)
            }))
            .into_any_element()
        } else {
            status_row.into_any_element()
        };
        div()
            .id(format!("{id}-row"))
            .debug_selector(move || location.selector("", None))
            .w_full()
            .min_w_0()
            .max_w(px(super::CONTENT_WIDTH))
            .mx_auto()
            .px_6()
            .child(
                MessageGroup::new()
                    .gap(px(10.))
                    .pb(px(28.))
                    .child(
                        Message::new()
                            .alignment(MessageAlignment::End)
                            .footer(self.user_footer(location, turn, cx))
                            .content(
                                MessageContent::new().bubble(
                                    Bubble::new()
                                        .with_variant(BubbleVariant::Muted)
                                        .max_w(px(440.))
                                        .content(
                                            BubbleContent::new()
                                                .rounded(cx.theme().radius)
                                                .px(px(14.))
                                                .py(px(9.))
                                                .when(location.child.is_none(), |bubble| {
                                                    bubble.when_some(
                                                        self.sent_references(
                                                            location.session,
                                                            location.turn,
                                                            cx,
                                                        ),
                                                        |bubble, references| {
                                                            bubble.child(references)
                                                        },
                                                    )
                                                })
                                                .child(self.linked_message(
                                                    format!("{id}-prompt"),
                                                    turn.prompt.clone(),
                                                    cx,
                                                ))
                                                .when(
                                                    turn.options
                                                        .as_ref()
                                                        .is_some_and(|options| options.attachment),
                                                    |bubble| {
                                                        bubble.child(
                                                            h_flex()
                                                                .gap_2()
                                                                .text_xs()
                                                                .child(
                                                                    Icon::new(IconName::File)
                                                                        .size_4(),
                                                                )
                                                                .child(tr("composer_example_file")),
                                                        )
                                                    },
                                                ),
                                        ),
                                ),
                            ),
                    )
                    .child(
                        Message::new()
                            .header(
                                MessageHeader::new().content_inset(false).w_full().child(
                                    v_flex()
                                        .debug_selector(move || location.selector("header", None))
                                        .text_sm()
                                        .w_full()
                                        .gap_2()
                                        .child(header)
                                        .child(Separator::horizontal()),
                                ),
                            )
                            .content(content)
                            .footer(footer),
                    ),
            )
            .into_any_element()
    }

    fn transcript_expanded(&self, location: Location) -> &std::collections::BTreeMap<usize, bool> {
        match (&self.side_resource, location.child) {
            (Some(crate::resources::SideResource::Subagent(panel)), Some(_))
                if panel.location == location =>
            {
                &panel.expanded
            }
            _ => {
                &self
                    .transcript_turn(location)
                    .expect("rendered turn exists")
                    .expanded
            }
        }
    }

    fn turn_group(
        &self,
        location: Location,
        block_index: usize,
        title: SharedString,
        icon: IconName,
        content: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let turn = self
            .transcript_turn(location)
            .expect("rendered turn exists");
        let expanded = self.transcript_expanded(location);
        let open = expanded.get(&block_index).copied().unwrap_or_else(|| {
            matches!(&turn.blocks[block_index], Block::Tools(calls) if calls.iter().any(|call| call.status.active()))
        });
        Collapsible::new()
            .open(open)
            .w_full()
            .min_w_0()
            .child(
                super::disclosure::trigger(
                    location.selector("group", Some(block_index)),
                    icon,
                    title,
                    crate::conversation::disclosure::Detail::default(),
                    open,
                    cx,
                )
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.expand_transcript_group(location, block_index, !open, cx);
                })),
            )
            .content(div().min_w_0().pt_1().pb_2().child(content))
            .into_any_element()
    }

    fn expand_transcript_group(
        &mut self,
        location: Location,
        block: usize,
        open: bool,
        cx: &mut Context<Self>,
    ) {
        if location.child.is_some() {
            if let Some(crate::resources::SideResource::Subagent(panel)) = &mut self.side_resource
                && panel.location == location
            {
                panel.expanded.insert(block, open);
                panel.scroller.update(cx, |state, cx| state.remeasure(cx));
            }
        } else if let Some(thread) = self.conversations.get_mut(&location.session)
            && let Some(turn) = thread.turns.get_mut(location.turn)
        {
            turn.expanded.insert(block, open);
            thread.scroller.update(cx, |state, cx| {
                state.remeasure_items(location.turn..location.turn + 1, cx)
            });
        }
        cx.notify();
    }
}

pub(super) fn status_icon(status: Status) -> IconName {
    match status {
        Status::Completed => IconName::Check,
        Status::Failed => IconName::TriangleAlert,
        Status::Cancelled => IconName::CircleX,
        Status::Waiting => IconName::CircleUser,
        Status::Queued | Status::Running(_) => IconName::LoaderCircle,
    }
}
