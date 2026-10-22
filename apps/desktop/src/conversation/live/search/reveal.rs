use super::*;
use sailry_client::conversation::HistoryRequest;

#[derive(Clone)]
pub(crate) struct Target {
    turn: sailry_protocol::TurnId,
    turn_sequence: std::num::NonZeroU64,
    entry: String,
}

impl Target {
    pub(crate) fn decode(value: serde_json::Value) -> Result<Self, &'static str> {
        let turn = serde_json::from_value(value["turn"].clone())
            .map_err(|_| "invalid conversation turn")?;
        let turn_sequence = value["turn_sequence"]
            .as_str()
            .and_then(|value| value.parse().ok())
            .filter(|value: &u64| *value <= i64::MAX as u64)
            .and_then(std::num::NonZeroU64::new)
            .ok_or("invalid conversation turn sequence")?;
        let entry = value["entry"]
            .as_str()
            .filter(|value| !value.is_empty() && value.len() <= 256)
            .ok_or("invalid conversation entry")?
            .to_owned();
        Ok(Self {
            turn,
            turn_sequence,
            entry,
        })
    }
}

impl View {
    pub(in crate::conversation::live) fn reveal_match(
        &mut self,
        target: Match,
        cx: &mut Context<Self>,
    ) {
        self.reveal_entry(
            Target {
                turn: target.turn,
                turn_sequence: target
                    .turn_sequence
                    .try_into()
                    .expect("validated search turn sequence"),
                entry: target.entry,
            },
            cx,
        );
    }

    pub(crate) fn reveal_entry(&mut self, target: Target, cx: &mut Context<Self>) {
        self.reveal = Some(target);
        if self.reveal_loaded(cx) {
            return;
        }
        self.request_target(cx);
    }

    pub(in crate::conversation::live) fn request_target(&mut self, cx: &mut Context<Self>) {
        let Some(target) = &self.reveal else {
            return;
        };
        if let Some(requests) = &self.older {
            let sequence = target.turn_sequence;
            if requests
                .try_send(HistoryRequest::Through(sequence))
                .is_err()
            {
                self.error = Some("chat_history_failed");
            }
        }
        cx.notify();
    }

    pub(in crate::conversation::live) fn reveal_loaded(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(target) = &self.reveal else {
            return false;
        };
        let Some(index) = self.rows.iter().position(|turn| *turn == target.turn) else {
            return false;
        };
        if !self.history.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot
                .page
                .entries
                .iter()
                .any(|entry| entry.id == target.entry && entry.turn == target.turn)
        }) {
            return false;
        }
        self.scroller.update(cx, |scroller, cx| {
            scroller.scroll_to_item(index, cx);
        });
        self.reveal = None;
        cx.notify();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use serde_json::json;

    #[test]
    fn target_requires_bounded_exact_identity() {
        let mut value = json!({"turn":sailry_protocol::TurnId::new(),
            "turn_sequence":"9007199254740993", "entry":"canonical-event"});
        assert_eq!(
            Target::decode(value.clone()).unwrap().turn_sequence.get(),
            9007199254740993
        );
        for invalid in ["0", "-1", "9223372036854775808", "invalid"] {
            value["turn_sequence"] = invalid.into();
            assert!(Target::decode(value.clone()).is_err());
        }
        value["turn_sequence"] = "1".into();
        value["entry"] = "".into();
        assert!(Target::decode(value).is_err());
    }
}
