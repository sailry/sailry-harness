use super::*;
use sailry_protocol::conversation::recovery::{Retry, Stop, Transport};

impl View {
    pub(super) fn recovery(
        &self,
        turn: TurnId,
        key: String,
        retry: Retry,
        active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = format!("live-model-retry-{turn}-{key}");
        let label = if retry.fallback {
            tr("chat_model_http_fallback").to_string()
        } else {
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
        };
        let label = if active {
            match retry.delay_ms {
                Some(delay) => format!("{label} · {}", wait(delay)),
                None => label,
            }
        } else {
            label
        };
        let detail = crate::conversation::disclosure::Detail {
            running: active,
            ..Default::default()
        };
        let error = retry
            .error
            .as_deref()
            .filter(|text| !text.trim().is_empty());
        let code = retry.code.as_deref().filter(|text| !text.trim().is_empty());
        let diagnostics = fields(&retry);
        if error.is_none() && code.is_none() && diagnostics.is_empty() {
            return crate::conversation::disclosure::summary(
                id,
                IconName::Network,
                label.into(),
                detail,
                cx,
            )
            .into_any_element();
        }
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
                v_flex()
                    .gap_2()
                    .text_sm()
                    .min_w_0()
                    .text_color(cx.theme().muted_foreground)
                    .children(
                        diagnostics
                            .into_iter()
                            .map(|(label, value)| div().child(format!("{}: {value}", tr(label)))),
                    )
                    .when_some(code, |body, code| {
                        body.child(format!("{}: {code}", tr("chat_model_error_code")))
                    })
                    .when_some(error, |body, error| {
                        body.child(
                            div()
                                .id(format!("{id}-scroll"))
                                .min_w_0()
                                .max_h_40()
                                .overflow_y_scroll()
                                .child(gpui_kit::base::SelectableText::new(
                                    format!("{id}-text"),
                                    diagnostic::display(error),
                                )),
                        )
                    }),
                cx,
            ))
            .into_any_element()
    }
}

fn wait(milliseconds: u64) -> String {
    rust_i18n::t!("chat_model_retry_wait", delay = duration(milliseconds)).to_string()
}

fn duration(milliseconds: u64) -> String {
    if milliseconds % 1000 == 0 {
        format!("{} s", milliseconds / 1000)
    } else {
        format!("{milliseconds} ms")
    }
}

fn fields(retry: &Retry) -> Vec<(&'static str, String)> {
    let mut fields = Vec::new();
    if let Some(status) = retry.status {
        fields.push(("chat_model_http_status", status.to_string()));
    }
    if let Some(transport) = retry.transport {
        fields.push((
            "chat_model_transport",
            match transport {
                Transport::Http => "HTTP",
                Transport::WebSocket => "WebSocket",
            }
            .into(),
        ));
    }
    if let Some(delay) = retry.delay_ms {
        fields.push(("chat_model_retry_delay", duration(delay)));
    }
    fields
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
    use sailry_protocol::{ErrorCode, Fault, FaultDetails};

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
                assert_eq!(
                    diagnostic::display(&fault.message),
                    "error.message: Final provider failure"
                );
                assert_eq!(fault, original);
            }
        }
    }
}
