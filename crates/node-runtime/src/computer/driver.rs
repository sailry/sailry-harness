//! One bound Cua SDK handle per Sailry session; no action rewriting.
#[cfg(test)]
mod tests;
use super::{Result, error};
use cua_driver_contract::EndSessionInput;
use cua_driver_sdk::{CuaDriver, CuaDriverSession, SessionPermissionMode, TrustedSessionOptions};
use sailry_protocol::SessionId;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) const TTL_SECONDS: u64 = 86_400;
pub(super) const IDLE_TTL_SECONDS: u64 = 3_600;

struct Session {
    handle: Arc<CuaDriverSession>,
    lease: Lease,
}

struct Lease {
    created: Instant,
    used: Instant,
}

impl Lease {
    fn expired(&self) -> bool {
        self.created.elapsed() >= Duration::from_secs(TTL_SECONDS)
            || self.used.elapsed() >= Duration::from_secs(IDLE_TTL_SECONDS)
    }
}

#[derive(Default)]
pub(super) struct Runtime {
    pub(super) worker: Option<super::Worker>,
    driver: Option<Arc<CuaDriver>>,
    sessions: HashMap<SessionId, Session>,
}

impl Runtime {
    pub(super) async fn call(
        &mut self,
        session: SessionId,
        name: &str,
        arguments: Value,
    ) -> Result<Value> {
        if self.driver.is_none() {
            self.driver = Some(match &self.worker {
                Some(worker) => worker.start().await?,
                None => tokio::task::spawn_blocking(|| {
                    CuaDriver::try_create_configured_for_host(
                        super::worker::configuration(),
                        super::catalog::options(None),
                    )
                })
                .await
                .map_err(error)?
                .map_err(error)?,
            });
        }
        if self
            .sessions
            .get(&session)
            .is_some_and(|session| session.lease.expired())
        {
            self.finish(session).await;
        }
        if !self.sessions.contains_key(&session) {
            let driver = self.driver.as_ref().unwrap().clone();
            let created = Instant::now();
            let bound = tokio::task::spawn_blocking(move || {
                driver.create_trusted_session(TrustedSessionOptions {
                    // Finished native transports retain ownership tombstones.
                    // Each SDK lifecycle therefore has its own public label.
                    public_session: uuid::Uuid::new_v4().to_string(),
                    mode: SessionPermissionMode::Standard,
                    ttl_seconds: TTL_SECONDS,
                    idle_ttl_seconds: IDLE_TTL_SECONDS,
                    capability_manifest_path: None,
                    bounded_manifest_path: None,
                })
            })
            .await
            .map_err(error)?
            .map_err(error)?;
            self.sessions.insert(
                session,
                Session {
                    handle: bound,
                    lease: Lease {
                        created,
                        used: created,
                    },
                },
            );
        }
        let called = Instant::now();
        let result = self.sessions[&session]
            .handle
            .call_tool(name.into(), arguments.to_string())
            .await
            .map_err(error)?;
        let output: Value = serde_json::from_str(&result.raw_json).map_err(error)?;
        if !result.is_error {
            // Start the idle clock before dispatch, conservatively within the
            // SDK's lease. Interrupted or refused calls do not extend it.
            self.sessions.get_mut(&session).unwrap().lease.used = called;
            if name == "end_session" {
                self.close(session).await;
            }
        } else if closed(result.error_code.as_deref(), &output) {
            // A cancelled end-session call can revoke authority after its
            // future is dropped. Return the native refusal without replay,
            // and let only a later explicit call establish fresh authority.
            self.close(session).await;
        }
        Ok(output)
    }

    pub(super) async fn finish(&mut self, session: SessionId) {
        if let Some(bound) = self.sessions.remove(&session) {
            let _ = bound
                .handle
                .end_session(EndSessionInput { session: None })
                .await;
            let _ = tokio::task::spawn_blocking(move || bound.handle.close()).await;
        }
    }

    async fn close(&mut self, session: SessionId) {
        if let Some(bound) = self.sessions.remove(&session) {
            let _ = tokio::task::spawn_blocking(move || bound.handle.close()).await;
        }
    }

    pub(super) async fn shutdown(&mut self) {
        for (_, bound) in self.sessions.drain() {
            let _ = bound
                .handle
                .end_session(EndSessionInput { session: None })
                .await;
            let _ = tokio::task::spawn_blocking(move || bound.handle.close()).await;
        }
        if let Some(driver) = self.driver.take() {
            let _ = driver.shutdown().await;
        }
    }
}

fn closed(code: Option<&str>, output: &Value) -> bool {
    code == Some("authorization_revoked")
        || (code == Some("permission_denied")
            && matches!(
                output["structuredContent"]["refusal"]["message"].as_str(),
                Some(
                    "Permission denied: authorization context expired"
                        | "authorization context expired"
                        | "authorization context idle timeout exceeded"
                )
            ))
}
