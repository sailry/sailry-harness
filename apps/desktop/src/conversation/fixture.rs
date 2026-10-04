//! In-memory animation fixtures; replace with Client presentation snapshots after UI acceptance.

use std::time::Duration;

use super::{
    interaction::{Draft, Kind, Request, State},
    turn::{Block, Phase, Status, Target, ToolCall, Turn},
};
use crate::{shell::Shell, tr};
use gpui_kit::component::{
    IconName,
    button::Button,
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::*;

pub(crate) fn sample(index: usize) -> Turn {
    let mut turn = Turn::new(tr("turn_preview"));
    advance(&mut turn, Duration::from_secs(10));
    turn.elapsed = Duration::from_secs(70);
    match index {
        1 => {
            advance(&mut turn, Duration::from_secs(6));
            turn.stop(Status::Failed);
            turn.blocks.push(Block::Error(tr("turn_preview_failure")));
        }
        2 => {
            advance(&mut turn, Duration::from_secs(3));
            turn.stop(Status::Waiting);
            turn.finished_at = None;
        }
        3 => {
            turn.status = Status::Waiting;
            turn.elapsed = Duration::from_secs(3);
            turn.finished_at = None;
            turn.prompt = tr("approval_pending");
            turn.blocks = crate::preview::FILES
                .into_iter()
                .enumerate()
                .map(|(id, target)| {
                    Block::Approval(super::approval::Request {
                        id,
                        generation: 0,
                        prompt: tr("tool_write_file"),
                        target: target.into(),
                        state: super::approval::State::Pending,
                    })
                })
                .collect();
        }
        _ => {}
    }
    turn
}

pub(crate) fn advance(turn: &mut Turn, elapsed: Duration) {
    let ms = elapsed.as_millis();
    turn.elapsed = elapsed;
    turn.status = match ms {
        0..500 => Status::Queued,
        500..2000 => Status::Running(Phase::Thinking),
        2000..4000 => Status::Running(Phase::Tools),
        4000..8000 => Status::Running(Phase::Answer),
        _ => Status::Completed,
    };
    turn.blocks.clear();
    if ms >= 500 {
        turn.blocks.push(Block::Reasoning(tr("preview_notice")));
    }
    if ms >= 2000 {
        turn.blocks.push(Block::Tools(
            crate::preview::FILES
                .into_iter()
                .map(|target| ToolCall {
                    name: "read_file".into(),
                    target: target.into(),
                    result: if ms >= 4000 {
                        tr("preview_notice")
                    } else {
                        "".into()
                    },
                    status: if ms >= 4000 {
                        Status::Completed
                    } else {
                        Status::Running(Phase::Tools)
                    },
                })
                .collect(),
        ));
    }
    if ms >= 4000 {
        let text = tr("preview_notice");
        let count = text.chars().count() * (ms.saturating_sub(4000).min(4000) as usize) / 4000;
        turn.blocks.push(Block::Text(
            text.chars().take(count).collect::<String>().into(),
        ));
    }
    if !turn.status.active() {
        turn.finished_at = Some(tr("turn_preview_time"));
    }
}

impl Shell {
    pub(crate) fn conversation_menu(
        &self,
        button: Button,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let key = (self.host, self.session);
        let active = self.conversations[&key]
            .turns
            .last()
            .is_some_and(|turn| turn.status.active());
        let owner = cx.entity().downgrade();
        button.dropdown_menu(move |menu, _, _| {
            let approval_owner = owner.clone();
            let mut menu = menu.item(
                PopupMenuItem::new(tr("approval_preview_menu"))
                    .icon(IconName::CircleUser)
                    .disabled(active)
                    .on_click(move |_, _, cx| {
                        _ = approval_owner
                            .update(cx, |shell, cx| shell.show_approval_preview(key, cx));
                    }),
            );
            for scene in InteractionScene::ALL {
                let owner = owner.clone();
                menu = menu.item(
                    PopupMenuItem::new(tr(scene.label()))
                        .icon(IconName::Bot)
                        .disabled(active)
                        .on_click(move |_, window, cx| {
                            _ = owner.update(cx, |shell, cx| {
                                shell.show_interaction_preview(key, scene, window, cx)
                            });
                        }),
                );
            }
            let owner = owner.clone();
            menu = menu.item(
                PopupMenuItem::new(tr("subagent_preview_menu"))
                    .icon(IconName::Bot)
                    .disabled(active)
                    .on_click(move |_, _, cx| {
                        _ = owner.update(cx, |shell, cx| shell.show_subagent_preview(key, cx));
                    }),
            );
            menu
        })
    }

    pub(crate) fn show_subagent_preview(&mut self, key: (usize, usize), cx: &mut Context<Self>) {
        let Some(thread) = self.conversations.get_mut(&key) else {
            return;
        };
        if thread.turns.last().is_some_and(|turn| turn.status.active()) {
            return;
        }
        let mut turn = Turn::new(tr("subagent_preview_prompt"));
        turn.status = Status::Running(Phase::Subagents);
        turn.elapsed = Duration::from_secs(8);
        let agents = [
            "subagent_sample_review",
            "subagent_sample_test",
            "subagent_sample_waiting",
            "subagent_sample_failed",
        ]
        .into_iter()
        .enumerate()
        .map(|(id, name)| {
            let mut child = match id {
                2 => sample(2),
                3 => sample(1),
                _ => sample(0),
            };
            if id == 1 {
                advance(&mut child, Duration::from_secs(3));
                child.finished_at = None;
            }
            child.prompt = tr(name);
            super::subagent::Agent {
                key: super::subagent::Key { id, generation: 0 },
                parent: (id > 1).then_some(super::subagent::Key {
                    id: 1,
                    generation: 0,
                }),
                name: tr(name),
                role: tr(if id == 0 {
                    "subagent_role_review"
                } else {
                    "subagent_role_test"
                }),
                order: 4 - id as u64,
                turn: child,
            }
        })
        .collect();
        turn.blocks.push(Block::Subagents(agents));
        thread.preview_task = None;
        thread.turns.push(turn);
        thread.scroller.update(cx, |state, cx| {
            state.append(1, cx);
            state.scroll_to_end(cx);
        });
        cx.notify();
    }

    pub(crate) fn show_interaction_preview(
        &mut self,
        key: (usize, usize),
        scene: InteractionScene,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = self.conversations.get(&key) else {
            return;
        };
        if thread.turns.last().is_some_and(|turn| turn.status.active()) {
            return;
        }
        let target = Target {
            session: key,
            turn: thread.turns.len(),
            request: 0,
            generation: 0,
        };
        let request = scene.request();
        let draft = Draft::new(&request.kind, window, cx);
        let thread = self.conversations.get_mut(&key).unwrap();
        let mut turn = Turn::new(tr("interaction_preview_prompt"));
        turn.status = Status::Waiting;
        turn.elapsed = Duration::from_secs(3);
        turn.blocks.push(Block::Interaction(request));
        thread.turns.push(turn);
        thread.preview_task = None;
        thread.interaction_drafts.insert(target, draft);
        thread.scroller.update(cx, |state, cx| {
            state.append(1, cx);
            state.scroll_to_end(cx);
        });
        cx.notify();
    }

    pub(crate) fn show_approval_preview(&mut self, key: (usize, usize), cx: &mut Context<Self>) {
        let Some(thread) = self.conversations.get_mut(&key) else {
            return;
        };
        if thread.turns.last().is_some_and(|turn| turn.status.active()) {
            return;
        }
        thread.preview_task = None;
        thread.turns.push(sample(3));
        thread.scroller.update(cx, |state, cx| {
            state.append(1, cx);
            state.scroll_to_end(cx);
        });
        cx.notify();
    }

    pub(super) fn animate_preview(
        &mut self,
        key: (usize, usize),
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let executor = cx.background_executor().clone();
        let mut started = executor.now();
        let mut index = self.conversations[&key].turns.len() - 1;
        let task = cx.spawn_in(window, async move |owner, cx| {
            loop {
                executor.timer(Duration::from_millis(250)).await;
                let keep_running = owner.update(cx, |shell, cx| {
                    let Some(conversation) = shell.conversations.get_mut(&key) else {
                        return false;
                    };
                    let Some(turn) = conversation.turns.get_mut(index) else {
                        return false;
                    };
                    if !turn.status.active() {
                        return false;
                    }
                    advance(turn, executor.now().duration_since(started));
                    let active = turn.status.active();
                    conversation.scroller.update(cx, |state, cx| {
                        state.remeasure_items(index..index + 1, cx);
                    });
                    cx.notify();
                    if !active && !conversation.queue.paused && shell.begin_queued_preview(key, cx)
                    {
                        index += 1;
                        started = executor.now();
                        true
                    } else {
                        active
                    }
                });
                if !matches!(keep_running, Ok(true)) {
                    break;
                }
            }
        });
        self.conversations.get_mut(&key).unwrap().preview_task = Some(task);
    }
}

#[derive(Clone, Copy)]
pub(crate) enum InteractionScene {
    Single,
    Multiple,
    Text,
    Secret,
    Plan,
}

impl InteractionScene {
    const ALL: [Self; 5] = [
        Self::Single,
        Self::Multiple,
        Self::Text,
        Self::Secret,
        Self::Plan,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Single => "interaction_menu_single",
            Self::Multiple => "interaction_menu_multiple",
            Self::Text => "interaction_menu_text",
            Self::Secret => "interaction_menu_secret",
            Self::Plan => "interaction_menu_plan",
        }
    }

    fn request(self) -> Request {
        let (prompt, kind) = match self {
            Self::Single | Self::Multiple => (
                "interaction_question_scope",
                Kind::Choice {
                    options: ["interaction_option_files", "interaction_option_tests"]
                        .into_iter()
                        .map(tr)
                        .collect(),
                    multiple: matches!(self, Self::Multiple),
                    allow_other: true,
                },
            ),
            Self::Text => (
                "interaction_question_details",
                Kind::Text {
                    multiline: true,
                    max_bytes: 4096,
                },
            ),
            Self::Secret => ("interaction_question_secret", Kind::Secret),
            Self::Plan => ("interaction_plan_details", Kind::Plan),
        };
        Request {
            id: 0,
            generation: 0,
            prompt: tr(prompt),
            kind,
            state: State::Pending,
        }
    }
}
