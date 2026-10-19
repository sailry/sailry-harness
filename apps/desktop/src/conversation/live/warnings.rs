//! Presentation-only delivery of safe operation warnings from newly observed history.
use super::*;
use gpui_kit::component::notification::Notification;
use sailry_protocol::conversation::Part;

#[derive(Default)]
struct Cursor {
    generation: Option<u64>,
    connected: bool,
    sequence: u64,
}

impl Cursor {
    fn accept(&mut self, history: &History) -> Vec<String> {
        let Some(snapshot) = &history.snapshot else {
            self.connected = false;
            return vec![];
        };
        let latest = snapshot
            .page
            .entries
            .last()
            .map_or(0, |entry| entry.sequence);
        let seed =
            self.generation != Some(history.generation) || !self.connected || !history.connected;
        self.generation = Some(history.generation);
        self.connected = history.connected;
        let previous = if seed && history.connected {
            history
                .live_start
                .map_or(latest, |start| start.saturating_sub(1))
        } else {
            self.sequence
        };
        self.sequence = if seed {
            latest
        } else {
            self.sequence.max(latest)
        };
        if !history.connected || latest <= previous {
            return vec![];
        }
        let Some(start) = history.live_start else {
            return vec![];
        };
        snapshot
            .page
            .entries
            .iter()
            .filter(|entry| {
                entry.sequence > previous
                    && entry.sequence >= start
                    && entry.author == "system"
                    && entry.parts.iter().any(|part| {
                        let Part::Resource(value) = part else {
                            return false;
                        };
                        value["type"] == "operation_warning"
                            && value["code"] == "context_summary_failed"
                            && value["turn"].as_str() == Some(entry.turn.to_string().as_str())
                    })
            })
            .map(|entry| entry.id.clone())
            .collect()
    }
}

pub(super) fn observe(window: &Window, cx: &mut Context<View>) {
    let window = window.window_handle();
    let mut cursor = Cursor::default();
    cx.observe_self(move |view, cx| {
        let notices = cursor.accept(&view.history);
        if notices.is_empty() {
            return;
        }
        _ = window.update(cx, |_, window, cx| {
            for id in notices {
                let message = tr("chat_context_summary_failed");
                crate::feedback::toast(
                    window,
                    message.clone(),
                    Notification::warning(message).id1::<Cursor>(id),
                    cx,
                );
            }
        });
    })
    .detach();
}

#[cfg(test)]
mod tests;
