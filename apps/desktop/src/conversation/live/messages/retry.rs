use super::*;
use sailry_protocol::conversation::recovery::{Retry, Stop};
use std::collections::BTreeSet;

impl View {
    pub(super) fn recovery(
        &self,
        turn: TurnId,
        key: String,
        retry: Retry,
        recovered: bool,
        active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = format!("live-model-retry-{turn}-{key}");
        let label = label(&retry, active);
        let detail = crate::conversation::disclosure::Detail {
            running: active,
            ..Default::default()
        };
        let Some(error) = body(&retry) else {
            return crate::conversation::disclosure::summary(
                id,
                IconName::Network,
                label.into(),
                detail,
                cx,
            )
            .into_any_element();
        };
        let key = expansion(key, recovered);
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
                    IconName::Network,
                    label.into(),
                    detail,
                    open,
                    cx,
                )
                .on_click(
                    cx.listener(move |view, _, _, cx| view.expand(turn, key.clone(), !open, cx)),
                ),
            )
            .content(crate::conversation::disclosure::body(
                div()
                    .id(format!("{id}-scroll"))
                    .text_sm()
                    .min_w_0()
                    .max_h_40()
                    .overflow_y_scroll()
                    .text_color(cx.theme().muted_foreground)
                    .child(gpui_kit::base::SelectableText::new(
                        format!("{id}-text"),
                        error.to_owned(),
                    )),
                cx,
            ))
            .into_any_element()
    }
}

fn label(retry: &Retry, active: bool) -> String {
    match retry.limit {
        Some(limit) => rust_i18n::t!(
            if active {
                "chat_model_reconnecting"
            } else {
                "chat_model_retried"
            },
            attempt = retry.attempt,
            limit = limit
        )
        .to_string(),
        None => tr(if active {
            "chat_model_reconnecting_continuously"
        } else {
            "chat_model_connection_retried"
        })
        .to_string(),
    }
}

fn body(retry: &Retry) -> Option<&str> {
    if retry.fallback {
        return None;
    }
    retry
        .error
        .as_deref()
        .filter(|text| !text.trim().is_empty())
}

fn expansion(key: String, recovered: bool) -> String {
    // Recovery closes the old detail once; manual expansion uses the settled key.
    if recovered {
        format!("{key}-recovered")
    } else {
        key
    }
}

pub(super) fn recovered(blocks: &[Block<'_>]) -> BTreeSet<String> {
    let mut output = false;
    let mut recovered = BTreeSet::new();
    for block in blocks.iter().rev() {
        match block {
            Block::Retry(key, _) if output => {
                recovered.insert(key.clone());
            }
            Block::Text(_, text) | Block::Thinking(_, text) if !text.trim().is_empty() => {
                output = true;
            }
            Block::Search(entry)
                if entry
                    .parts
                    .iter()
                    .any(|part| matches!(part, Part::Text(text) if !text.trim().is_empty())) =>
            {
                output = true;
            }
            Block::Images(images) if !images.is_empty() => output = true,
            Block::Tools(_) | Block::Question(_) | Block::Children(_) => output = true,
            _ => {}
        }
    }
    recovered
}

pub(super) fn stop_key(reason: Stop) -> &'static str {
    match reason {
        Stop::AttemptsExhausted => "chat_model_retry_exhausted",
        Stop::RateLimited => "chat_model_rate_limited",
        Stop::ServerOverloaded => "chat_model_server_overloaded",
        Stop::InvalidRequest | Stop::InvalidPrompt => "chat_model_request_rejected",
        Stop::ContextWindowExceeded => "chat_model_context_exceeded",
        Stop::QuotaExceeded | Stop::UsageLimitReached => "chat_model_usage_exhausted",
        Stop::UsageNotIncluded => "chat_model_usage_not_included",
        Stop::ResponseIncomplete => "chat_model_response_incomplete",
        Stop::FlexUnavailable => "chat_model_flex_unavailable",
        Stop::PolicyViolation => "chat_model_policy_rejected",
        Stop::LocalFailure => "chat_model_request_unavailable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use sailry_protocol::conversation::recovery::Transport;
    use sailry_protocol::{ErrorCode, Fault, FaultDetails};
    use std::collections::BTreeMap;

    #[test]
    fn omits_headline_delay() {
        for (attempt, fallback, limit) in
            [(4, false, Some(5)), (2, false, None), (3, true, Some(5))]
        {
            let mut retry = Retry {
                attempt,
                fallback,
                limit,
                delay_ms: None,
                transport: Some(Transport::Http),
                status: Some(503),
                code: Some("connection_lost".into()),
                error: Some("Connection closed".into()),
            };
            let active = label(&retry, true);
            let settled = label(&retry, false);
            for delay in [0, 60_000, 120_000] {
                retry.delay_ms = Some(delay);
                assert_eq!(label(&retry, true), active);
                assert_eq!(label(&retry, false), settled);
            }
        }
    }

    #[test]
    fn preserves_original_error_body() {
        let mut retry = Retry {
            attempt: 4,
            fallback: false,
            limit: Some(5),
            delay_ms: Some(1723),
            transport: Some(Transport::Http),
            status: Some(503),
            code: Some("model.request_unavailable".into()),
            error: None,
        };
        assert_eq!(body(&retry), None);
        for error in [
            "HTTP response ended before completion",
            "Provider rejected the request with HTTP 503",
            r#"{"error":{"message":"Provider explanation"}}"#,
            "  Original detail\n\n[redacted]  ",
        ] {
            retry.error = Some(error.into());
            assert_eq!(body(&retry), Some(error));
        }
        for error in ["", "  "] {
            retry.error = Some(error.into());
            assert_eq!(body(&retry), None);
        }
        retry.error = Some("HTTP response ended before completion".into());
        retry.fallback = true;
        retry.attempt = 3;
        assert_eq!(body(&retry), None);
    }

    #[test]
    fn collapses_after_model_output() {
        let retry = Retry::from_resource(&serde_json::json!({
            "type":"model_retry", "attempt":4, "limit":5,
            "error":"HTTP response ended before completion"
        }))
        .unwrap();
        let mut blocks = vec![
            Block::Text("earlier".into(), "Earlier response".into()),
            Block::Retry("failure".into(), retry.clone()),
            Block::Resource,
            Block::Text("empty".into(), "  ".into()),
        ];
        assert!(recovered(&blocks).is_empty());
        blocks.push(Block::Thinking("output".into(), "Response resumed".into()));
        blocks.push(Block::Retry("later".into(), retry));
        assert_eq!(recovered(&blocks), BTreeSet::from(["failure".into()]));

        let pending = expansion("failure".into(), false);
        let settled = expansion("failure".into(), true);
        let mut expanded = BTreeMap::from([(pending, true)]);
        assert!(!expanded.get(&settled).copied().unwrap_or(false));
        expanded.insert(settled.clone(), true);
        assert_eq!(expansion("failure".into(), true), settled);
        assert_eq!(expanded.get(&settled), Some(&true));
    }

    #[test]
    fn preserves_stop_details_and_original_cause() {
        for reason in [
            Stop::AttemptsExhausted,
            Stop::RateLimited,
            Stop::ServerOverloaded,
            Stop::InvalidRequest,
            Stop::InvalidPrompt,
            Stop::ContextWindowExceeded,
            Stop::QuotaExceeded,
            Stop::UsageNotIncluded,
            Stop::UsageLimitReached,
            Stop::ResponseIncomplete,
            Stop::FlexUnavailable,
            Stop::PolicyViolation,
            Stop::LocalFailure,
        ] {
            for (code, expected) in [
                (ErrorCode::Unavailable, stop_key(reason)),
                (ErrorCode::Cancelled, "turn_cancelled"),
                (ErrorCode::OutcomeUnknown, "chat_execution_unknown"),
            ] {
                let fault = Fault {
                    code,
                    message: r#"{"error":{"message":"Final provider failure"}}"#.into(),
                    details: Some(FaultDetails::ModelRecovery { reason }),
                };
                let original = fault.clone();
                assert_eq!(failure_key(&fault), expected);
                assert_eq!(fault, original);
            }
        }
    }
}
