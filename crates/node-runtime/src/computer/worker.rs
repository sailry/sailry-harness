//! The application hosts Cua's native run loop in a supervised SDK worker.
use super::{Result, error};
use cua_driver_sdk::{
    ConfiguredDriverOptions, CuaDriver, PrivateWorkerOptions, RuntimeAuthorizationOptions,
    SessionPermissionMode,
};
use std::{path::PathBuf, sync::Arc};

/// An application entry point implementing the official private-worker channel.
#[derive(Clone, Debug)]
pub struct Worker {
    pub executable: PathBuf,
    pub bundle_id: String,
}

impl Worker {
    pub(super) async fn start(&self) -> Result<Arc<CuaDriver>> {
        let options = self.options()?;
        tokio::task::spawn_blocking(move || CuaDriver::create_private_worker(options))
            .await
            .map_err(error)?
            .map_err(error)
    }

    fn options(&self) -> Result<PrivateWorkerOptions> {
        Ok(PrivateWorkerOptions {
            binary_path: self
                .executable
                .to_str()
                .ok_or("computer worker path is not UTF-8")?
                .into(),
            host_bundle_id: self.bundle_id.clone(),
            startup_timeout_ms: Some(10_000),
            shutdown_timeout_ms: Some(2_000),
            configured_driver: configuration(),
            environment: vec![],
            inherit_stderr: true,
        })
    }
}

pub(super) fn configuration() -> ConfiguredDriverOptions {
    ConfiguredDriverOptions {
        claude_code_compatibility: false,
        authorization: RuntimeAuthorizationOptions {
            allowed_modes: vec![SessionPermissionMode::Standard],
            compatibility_mode: SessionPermissionMode::Standard,
            compatibility_capability_manifest_path: None,
            compatibility_bounded_manifest_path: None,
            unrestricted_acknowledged: false,
            max_session_ttl_seconds: super::driver::TTL_SECONDS,
            max_idle_ttl_seconds: super::driver::IDLE_TTL_SECONDS,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_standard_authorization() {
        let options = Worker {
            executable: "/fixture/sailry-host".into(),
            bundle_id: "ai.sailry.host".into(),
        }
        .options()
        .unwrap();
        assert_eq!(
            options.configured_driver.authorization.allowed_modes,
            [SessionPermissionMode::Standard]
        );
        assert!(
            !options
                .configured_driver
                .authorization
                .unrestricted_acknowledged
        );
        assert!(options.environment.is_empty());
    }
}
