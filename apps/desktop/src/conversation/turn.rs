use std::{collections::BTreeMap, time::Duration};

use gpui_kit::SharedString;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Thinking,
    Tools,
    Answer,
    Subagents,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Queued,
    Running(Phase),
    Waiting,
    Completed,
    Cancelled,
    Failed,
}

impl Status {
    pub fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Running(_) | Self::Waiting)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "turn_queued",
            Self::Running(_) => "turn_running",
            Self::Waiting => "turn_waiting",
            Self::Completed => "turn_completed",
            Self::Cancelled => "turn_cancelled",
            Self::Failed => "turn_failed",
        }
    }

    pub fn phase_label(self) -> &'static str {
        match self {
            Self::Running(Phase::Thinking) => "turn_thinking",
            Self::Running(Phase::Tools) => "turn_tools_running",
            Self::Running(Phase::Answer) => "turn_generating",
            Self::Running(Phase::Subagents) => "turn_subagents",
            _ => self.label(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ToolCall {
    pub name: SharedString,
    pub target: SharedString,
    pub result: SharedString,
    pub status: Status,
}

#[derive(Clone, Debug)]
pub(crate) enum Block {
    Text(SharedString),
    Reasoning(SharedString),
    Error(SharedString),
    Tools(Vec<ToolCall>),
    Approval(super::approval::Request),
    Interaction(super::interaction::Request),
    Subagents(Vec<super::subagent::Agent>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Target {
    pub session: (usize, usize),
    pub turn: usize,
    pub request: usize,
    pub generation: usize,
}

/// Presentation snapshot only. The preview driver never executes an agent or tool.
#[derive(Clone, Debug)]
pub(crate) struct Turn {
    pub prompt: SharedString,
    pub options: Option<super::Options>,
    pub status: Status,
    pub elapsed: Duration,
    pub finished_at: Option<SharedString>,
    pub blocks: Vec<Block>,
    pub expanded: BTreeMap<usize, bool>,
}

impl Turn {
    pub fn new(prompt: SharedString) -> Self {
        Self {
            prompt,
            options: None,
            status: Status::Queued,
            elapsed: Duration::ZERO,
            finished_at: None,
            blocks: Vec::new(),
            expanded: BTreeMap::new(),
        }
    }

    pub fn copy_text(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|block| match block {
                Block::Text(text) if !text.trim().is_empty() => Some(text.as_ref()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn duration_label(&self) -> String {
        let seconds = self.elapsed.as_secs();
        if seconds < 60 {
            format!("{seconds}s")
        } else if seconds < 3600 {
            format!("{}m{:02}s", seconds / 60, seconds % 60)
        } else {
            format!(
                "{}h{:02}m{:02}s",
                seconds / 3600,
                seconds / 60 % 60,
                seconds % 60
            )
        }
    }

    pub fn stop(&mut self, status: Status) {
        self.status = status;
        for block in &mut self.blocks {
            if let Block::Approval(request) = block
                && request.state == super::approval::State::Pending
            {
                request.state = super::approval::State::Cancelled;
            }
            if let Block::Interaction(request) = block
                && request.state == super::interaction::State::Pending
            {
                request.state = super::interaction::State::Cancelled;
            }
            if let Block::Tools(calls) = block {
                for call in calls.iter_mut().filter(|call| call.status.active()) {
                    call.status = status;
                }
            }
            if let Block::Subagents(agents) = block {
                for agent in agents.iter_mut().filter(|agent| agent.turn.status.active()) {
                    agent.turn.stop(status);
                    agent.turn.finished_at = Some(crate::tr("turn_preview_time"));
                }
            }
        }
    }

    pub fn has_pending_requests(&self) -> bool {
        self.blocks.iter().any(|block| match block {
            Block::Approval(request) => request.state == super::approval::State::Pending,
            Block::Interaction(request) => request.state == super::interaction::State::Pending,
            _ => false,
        })
    }

    pub fn subagents(&self) -> impl Iterator<Item = &super::subagent::Agent> {
        self.blocks
            .iter()
            .filter_map(|block| match block {
                Block::Subagents(agents) => Some(agents),
                _ => None,
            })
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{conversation::fixture, tr};

    #[test]
    fn duration_boundaries() {
        let mut turn = Turn::new("prompt".into());
        for (seconds, expected) in [
            (0, "0s"),
            (59, "59s"),
            (60, "1m00s"),
            (70, "1m10s"),
            (3601, "1h00m01s"),
        ] {
            turn.elapsed = Duration::from_secs(seconds);
            assert_eq!(turn.duration_label(), expected);
        }
    }

    #[test]
    fn streams_unicode_prefixes() {
        let mut turn = Turn::new("prompt".into());
        let full = tr("preview_notice");
        let mut previous = String::new();
        for ms in (4000..=8000).step_by(125) {
            fixture::advance(&mut turn, Duration::from_millis(ms));
            let current = turn.copy_text();
            assert!(full.starts_with(&current));
            assert!(current.starts_with(&previous));
            previous = current;
        }
        assert_eq!(previous, full.as_ref());
        assert_eq!(turn.blocks.len(), 3);
    }

    #[test]
    fn stops_tools_and_copies_answer() {
        let mut turn = Turn::new("prompt".into());
        fixture::advance(&mut turn, Duration::from_secs(3));
        if let Block::Tools(calls) = &mut turn.blocks[1] {
            calls[0].status = Status::Completed;
        }
        turn.stop(Status::Cancelled);
        assert!(matches!(&turn.blocks[1], Block::Tools(calls)
            if calls[0].status == Status::Completed && calls[1].status == Status::Cancelled));
        turn.blocks.push(Block::Text("First".into()));
        turn.blocks.push(Block::Error("Error".into()));
        turn.blocks.push(Block::Text("Second".into()));
        assert_eq!(turn.copy_text(), "First\n\nSecond");
    }
}
