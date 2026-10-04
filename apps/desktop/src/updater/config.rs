//! Release-owned trust configuration. Runtime environment variables cannot replace its keys.
use super::{Failure, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};

pub(super) const SOURCE: &str =
    "https://raw.githubusercontent.com/sailry/sailry-harness/main/update-v1.json";
pub(super) const MAX_MANIFEST: usize = 1024 * 1024;
pub(super) const MAX_ARCHIVE: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct Config {
    pub source: String,
    pub keys: Vec<self_update::VerifyingKey>,
    pub target: String,
    pub version: String,
    pub system: String,
}

impl Config {
    pub fn release() -> Result<Self> {
        let source = option_env!("SAILRY_UPDATE_SOURCE")
            .unwrap_or(SOURCE)
            .to_owned();
        let encoded = option_env!("SAILRY_UPDATE_PUBLIC_KEYS").unwrap_or_default();
        let mut keys = Vec::new();
        for value in encoded.split(',').filter(|value| !value.trim().is_empty()) {
            let bytes = STANDARD.decode(value.trim()).map_err(|_| {
                Failure::new(
                    "updates_source_unready",
                    "invalid configured update public key",
                )
            })?;
            let bytes: [u8; 32] = bytes.try_into().map_err(|_| {
                Failure::new(
                    "updates_source_unready",
                    "update public keys must have 32 bytes",
                )
            })?;
            let key = ed25519_dalek::VerifyingKey::from_bytes(&bytes).map_err(|_| {
                Failure::new(
                    "updates_source_unready",
                    "invalid configured update public key",
                )
            })?;
            if key.is_weak() {
                return Err(Failure::new(
                    "updates_source_unready",
                    "weak update public key",
                ));
            }
            keys.push(bytes);
        }
        if keys.is_empty() {
            return Err(Failure::new(
                "updates_source_unready",
                "no trusted update public key is configured",
            ));
        }
        validate_url(&source)?;
        Ok(Self {
            source,
            keys,
            target: self_update::get_target().into(),
            version: env!("CARGO_PKG_VERSION").into(),
            system: system_version()?,
        })
    }
}

pub(super) fn validate_url(value: &str) -> Result<()> {
    let url = url::Url::parse(value)
        .map_err(|_| Failure::new("updates_manifest_invalid", "invalid update URL"))?;
    let local_fixture = cfg!(test)
        && url.scheme() == "http"
        && url
            .host_str()
            .is_some_and(|host| matches!(host, "127.0.0.1" | "[::1]" | "localhost"));
    if (url.scheme() != "https" && !local_fixture)
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Failure::new(
            "updates_manifest_invalid",
            "update URLs require HTTPS without credentials or fragments",
        ));
    }
    Ok(())
}

fn system_version() -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/bin/sw_vers")
            .arg("-productVersion")
            .output()
            .map_err(|_| {
                Failure::new(
                    "updates_system_unavailable",
                    "macOS version could not be read",
                )
            })?;
        if !output.status.success() {
            return Err(Failure::new(
                "updates_system_unavailable",
                "macOS version could not be read",
            ));
        }
        String::from_utf8(output.stdout)
            .map(|value| value.trim().to_owned())
            .map_err(|_| Failure::new("updates_system_unavailable", "macOS version is not UTF-8"))
    }
    #[cfg(target_os = "windows")]
    {
        super::windows::system_version()
            .map_err(|detail| Failure::new("updates_system_unavailable", detail))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err(Failure::new(
            "updates_platform",
            "desktop updates are supported on macOS and Windows",
        ))
    }
}
