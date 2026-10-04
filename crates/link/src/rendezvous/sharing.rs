use std::time::Duration;

use sailry_protocol::{ErrorCode, Fault};
use tokio::sync::watch;

use super::{Relay, RequestId, now_ms};
use crate::{CancellationToken, LinkHandle};

/// Presentation data only; no ticket or cancellation credential leaves the owner.
#[derive(Clone)]
pub enum ShareState {
    Preparing,
    Ready { code: String, expires_at_ms: u64 },
    Retrying(Fault),
    Paired,
    Closed,
}

impl Relay {
    /// Drive while the pairing view is alive. Cancelling or dropping this future
    /// drops the Rust invitation immediately; established trust is not revoked.
    pub async fn share(
        &self,
        link: &LinkHandle,
        state: watch::Sender<ShareState>,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        loop {
            if stop.is_cancelled() || state.is_closed() {
                state.send_replace(ShareState::Closed);
                return Ok(());
            }
            state.send_replace(ShareState::Preparing);
            let mut invitation = link.invite()?;
            let request = RequestId::default();
            let deadline = tokio::time::Instant::now()
                + Duration::from_millis(invitation.expires_at_ms().saturating_sub(now_ms()?));
            let publication = loop {
                let result = tokio::select! {
                    biased;
                    _ = stop.cancelled() => break None,
                    _ = state.closed() => break None,
                    _ = tokio::time::sleep_until(deadline) => break None,
                    result = self.publish(&invitation, &request) => result,
                };
                match result {
                    Ok(code) => break Some(code),
                    Err(error) => {
                        let retry = matches!(error.code, ErrorCode::Unavailable | ErrorCode::Busy);
                        state.send_replace(ShareState::Retrying(error.clone()));
                        if !retry {
                            return Err(error);
                        }
                        tokio::select! {
                            _ = stop.cancelled() => break None,
                            _ = state.closed() => break None,
                            _ = tokio::time::sleep_until(deadline) => break None,
                            _ = tokio::time::sleep(Duration::from_secs(15)) => {},
                        }
                    }
                }
            };
            let Some(code) = publication else {
                drop(invitation);
                continue;
            };
            state.send_replace(ShareState::Ready {
                code: code.code.clone(),
                expires_at_ms: code.expires_at_ms,
            });
            let paired = tokio::select! {
                biased;
                _ = stop.cancelled() => false,
                _ = state.closed() => false,
                result = invitation.paired() => result.is_ok(),
                _ = tokio::time::sleep_until(deadline) => false,
            };
            // Invalidate locally before doing any best-effort HTTP cleanup.
            drop(invitation);
            let ended = paired
                || stop.is_cancelled()
                || state.is_closed()
                || tokio::time::Instant::now() < deadline;
            state.send_replace(if paired {
                ShareState::Paired
            } else if ended {
                ShareState::Closed
            } else {
                ShareState::Preparing
            });
            let _ = tokio::time::timeout(Duration::from_secs(2), self.cancel(&code)).await;
            if ended {
                return Ok(());
            }
        }
    }
}
