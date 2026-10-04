//! Controller application updates never replace or shut down a remote execution Host.
mod cached;
mod config;
mod install;
mod manifest;
mod package;
mod panel;
mod receipt;
pub(crate) mod recovery;
mod service;
#[cfg(test)]
mod tests;
mod transfer;
#[cfg(any(target_os = "windows", test))]
mod windows;

pub(crate) use panel::Panel;
pub(crate) use service::init;
use std::fmt;

type Result<T> = std::result::Result<T, Failure>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Failure {
    key: &'static str,
    detail: String,
}

impl Failure {
    fn new(key: &'static str, detail: impl Into<String>) -> Self {
        Self {
            key,
            detail: detail.into(),
        }
    }

    fn io(error: std::io::Error) -> Self {
        Self::new("updates_io_failed", error.to_string())
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for Failure {}

pub(crate) fn run_helper() -> bool {
    #[cfg(target_os = "windows")]
    {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if let Some(result) = windows::run_if_requested(&args, |archive, proof, staging| {
            let selection: manifest::Selection = serde_json::from_value(proof.clone())
                .map_err(|_| "invalid staged update proof".to_owned())?;
            let config = config::Config::release().map_err(|error| error.to_string())?;
            manifest::revalidate(&selection, &config).map_err(|error| error.to_string())?;
            package::verify_archive(archive, &selection.release, &config.keys)
                .map_err(|error| error.to_string())?;
            let root = package::extract(archive, &selection.release, staging)
                .map_err(|error| error.to_string())?;
            Ok(windows::VerifiedBundle {
                root,
                executable: selection.release.executable.into(),
            })
        }) {
            if let Err(error) = result {
                eprintln!("desktop update helper failed: {error}");
                std::process::exit(1);
            }
            return true;
        }
    }
    false
}
