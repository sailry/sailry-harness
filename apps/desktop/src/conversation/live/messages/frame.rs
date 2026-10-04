//! Turn-frame behavior follows Code Pi d9b56405 ai_response_block.dart.
//! Kit owns the header/body/footer layout; Node timestamps own execution timing.
use super::{Block, Status};
use sailry_protocol::conversation::{Page, Run, RunKind};

pub(super) fn active(status: Status) -> bool {
    matches!(status, Status::Queued | Status::Running | Status::Stopping)
}

pub(super) fn status(run: &Run, connected: bool) -> &'static str {
    if active(run.status) && !connected {
        "chat_status_unsynced"
    } else {
        super::super::subagents::status_key(run.status)
    }
}

pub(super) fn elapsed(run: &Run, now_ms: i64) -> Option<String> {
    let started = run.started_ms?;
    let end = if active(run.status) {
        now_ms
    } else {
        run.finished_ms?
    };
    let seconds = end.saturating_sub(started).max(0) as u64 / 1000;
    Some(if seconds < 60 {
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
    })
}

pub(super) fn phase(run: &Run, page: &Page, blocks: &[Block<'_>]) -> Option<&'static str> {
    if !active(run.status) {
        return None;
    }
    if run.kind == RunKind::Compaction {
        return Some(super::super::compaction::status(run, page));
    }
    if run.status != Status::Running {
        return Some(super::super::subagents::status_key(run.status));
    }
    if page.approvals.iter().any(|item| {
        item.turn == run.turn && item.state == sailry_protocol::conversation::ApprovalState::Pending
    }) || page.questions.iter().any(|item| {
        item.turn == run.turn
            && item.state == sailry_protocol::conversation::question::State::Pending
    }) {
        return Some("turn_waiting");
    }
    if continuing_tools(run, page, blocks)
        || blocks.iter().any(|block| match block {
            Block::Tools(calls) => calls
                .iter()
                .any(|call| call.state == sailry_client::conversation::tools::State::Running),
            Block::Children(children) => page
                .children
                .iter()
                .any(|child| children.contains(&child.run.session) && active(child.run.status)),
            _ => false,
        })
    {
        return None;
    }
    if matches!(blocks.last(), Some(Block::Retry(..))) {
        return None;
    }
    Some(match blocks.last() {
        Some(Block::Text(..) | Block::Search(..) | Block::Image(..)) => "turn_generating",
        Some(Block::Thinking(..)) => "turn_thinking",
        _ => "turn_awaiting_response",
    })
}

/// Keep the presentation on the last activity until new content takes over.
/// This does not change the completed tool's execution state.
pub(super) fn continuing_tools(run: &Run, page: &Page, blocks: &[Block<'_>]) -> bool {
    run.status == Status::Running
        && run.kind == RunKind::Task
        && matches!(blocks.last(), Some(Block::Tools(_) | Block::Children(_)))
        && !page.approvals.iter().any(|item| {
            item.turn == run.turn
                && item.state == sailry_protocol::conversation::ApprovalState::Pending
        })
        && !page.questions.iter().any(|item| {
            item.turn == run.turn
                && item.state == sailry_protocol::conversation::question::State::Pending
        })
}

pub(super) fn finished(run: &Run) -> Option<String> {
    if active(run.status) {
        return None;
    }
    at(run.finished_ms?)
}

pub(super) fn at(timestamp_ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|time| timestamp(time.with_timezone(&chrono::Local), chrono::Local::now()))
}

fn timestamp(
    time: chrono::DateTime<chrono::Local>,
    now: chrono::DateTime<chrono::Local>,
) -> String {
    use chrono::Datelike;
    time.format(if time.date_naive() == now.date_naive() {
        "%H:%M"
    } else if time.year() == now.year() {
        "%m-%d %H:%M"
    } else {
        "%Y-%m-%d %H:%M"
    })
    .to_string()
}

pub(super) fn full_time(run: &Run) -> Option<String> {
    full_at(run.finished_ms?)
}

pub(super) fn full_at(timestamp_ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(timestamp_ms).map(|time| {
        time.with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string()
    })
}

#[cfg(test)]
mod tests;
