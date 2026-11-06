//! Installed WSL distributions and independent Linux Host deployment belong to the Node.
use crate::process::Environment;
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault, NodeId, Output, Request, RequestId, host::installation::Outcome,
    network::Selection, plugin, ssh::InstallProgress, wsl::Distribution,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

mod command;
#[cfg(any(windows, test))]
mod discovery;
mod install;
#[cfg(test)]
mod tests;

#[derive(Clone)]
struct Source {
    executable: PathBuf,
    distributions: Vec<Distribution>,
}

struct Active {
    distribution: String,
    context: Option<plugin::Context>,
    stop: CancellationToken,
    progress: tokio::sync::watch::Sender<InstallProgress>,
}

#[derive(Default)]
pub(crate) struct Service {
    active: Mutex<BTreeMap<(NodeId, RequestId), Active>>,
    #[cfg(any(test, feature = "test-support"))]
    source: Mutex<Option<Source>>,
}

pub(crate) struct Installation {
    service: Arc<Service>,
    owner: (NodeId, RequestId),
    distribution: String,
    stop: CancellationToken,
    progress: tokio::sync::watch::Sender<InstallProgress>,
}

impl Service {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn fixture(&self, executable: PathBuf, distributions: Vec<Distribution>) {
        *self.source.lock().expect("WSL fixture poisoned") = Some(Source {
            executable,
            distributions,
        });
    }

    async fn source(
        &self,
        environment: &Environment,
        stop: &CancellationToken,
    ) -> Result<Source, Fault> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(source) = self.source.lock().expect("WSL fixture poisoned").clone() {
            return Ok(source);
        }
        #[cfg(windows)]
        {
            let Some(executable) = executable(environment) else {
                return Ok(Source {
                    executable: PathBuf::new(),
                    distributions: Vec::new(),
                });
            };
            let names = discovery::list(&executable, environment, stop).await?;
            let distributions = tokio::task::spawn_blocking(move || discovery::versions(names))
                .await
                .map_err(|_| {
                    Fault::new(
                        ErrorCode::Unavailable,
                        "WSL registration discovery worker failed",
                    )
                })??;
            Ok(Source {
                executable,
                distributions,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = (environment, stop);
            Err(Fault::new(
                ErrorCode::Unavailable,
                "WSL Host installation requires Windows",
            ))
        }
    }

    pub(crate) async fn list(
        &self,
        environment: &Environment,
        stop: &CancellationToken,
    ) -> Result<Vec<Distribution>, Fault> {
        self.source(environment, stop)
            .await
            .map(|source| source.distributions)
    }

    pub(crate) fn begin(
        self: &Arc<Self>,
        caller: NodeId,
        request: &Request,
        closed: &CancellationToken,
    ) -> Result<Installation, Fault> {
        let sailry_protocol::Command::InstallWslHost { distribution, .. } = &request.command else {
            return Err(Fault::invalid("WSL installation command expected"));
        };
        validate(distribution)?;
        let mut active = self.active.lock().expect("WSL installations poisoned");
        if active
            .values()
            .any(|entry| entry.distribution.eq_ignore_ascii_case(distribution))
        {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "Host installation is already active in this distribution",
            ));
        }
        let stop = closed.child_token();
        let (progress, _) = tokio::sync::watch::channel(InstallProgress::Connecting);
        active.insert(
            (caller, request.id),
            Active {
                distribution: distribution.clone(),
                context: request.plugin.clone(),
                stop: stop.clone(),
                progress: progress.clone(),
            },
        );
        Ok(Installation {
            service: self.clone(),
            owner: (caller, request.id),
            distribution: distribution.clone(),
            stop,
            progress,
        })
    }

    pub(crate) fn progress(
        &self,
        caller: NodeId,
        request: RequestId,
        context: &Option<plugin::Context>,
    ) -> Result<Output, Fault> {
        let active = self.active.lock().expect("WSL installations poisoned");
        let entry = active
            .get(&(caller, request))
            .filter(|entry| context.is_none() || entry.context == *context)
            .ok_or_else(|| {
                Fault::new(ErrorCode::NotFound, "WSL Host installation is not active")
            })?;
        let progress = entry.progress.borrow().clone();
        Ok(Output::HostInstallProgress(progress))
    }

    pub(crate) fn cancel(
        &self,
        caller: NodeId,
        request: RequestId,
        context: &Option<plugin::Context>,
    ) -> Result<Output, Fault> {
        let active = self.active.lock().expect("WSL installations poisoned");
        if let Some(entry) = active.get(&(caller, request)) {
            if context.is_some() && entry.context != *context {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "Host installation belongs to another plugin scope",
                ));
            }
            entry.stop.cancel();
        }
        Ok(Output::WslHostInstallCancelled { request })
    }
}

impl Drop for Installation {
    fn drop(&mut self) {
        self.service
            .active
            .lock()
            .expect("WSL installations poisoned")
            .remove(&self.owner);
    }
}

pub(crate) fn validate(distribution: &str) -> Result<(), Fault> {
    if distribution.is_empty()
        || distribution.len() > 256
        || distribution.starts_with('-')
        || distribution.chars().any(char::is_control)
    {
        return Err(Fault::invalid("invalid WSL distribution name"));
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn executable(environment: &Environment) -> Option<PathBuf> {
    std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32/wsl.exe"))
        .into_iter()
        .chain(which::which_in("wsl.exe", environment.path(), "/").ok())
        .find_map(|path| which::which_in(path, environment.path(), "/").ok())
        .filter(|path| path.is_absolute())
}

#[cfg(windows)]
pub(crate) async fn shells(
    environment: &Environment,
    stop: &CancellationToken,
) -> Result<Vec<sailry_protocol::terminal::Shell>, Fault> {
    let Some(executable) = executable(environment) else {
        return Ok(Vec::new());
    };
    let names = discovery::list(&executable, environment, stop).await?;
    let path = executable
        .to_str()
        .ok_or_else(|| Fault::new(ErrorCode::Unavailable, "WSL executable path is not Unicode"))?;
    Ok(names
        .into_iter()
        .map(|name| sailry_protocol::terminal::Shell {
            path: path.into(),
            arguments: vec!["--distribution".into(), name],
        })
        .collect())
}
