use super::*;
use crate::host::deployment;
use sailry_link::LinkHandle;
use std::{path::Path, time::Duration};

const PREFLIGHT: &str = include_str!("preflight.sh");
const TICKET: &str = "cat \"$HOME/.sailry-host/bootstrap.ticket\" 2>/dev/null || test ! -e \"$HOME/.sailry-host/bootstrap.ticket\"\n";

impl Installation {
    pub(crate) async fn execute(
        self,
        environment: &Environment,
        link: &LinkHandle,
        network: Option<&Selection>,
    ) -> Result<Outcome, Fault> {
        self.progress.send_replace(InstallProgress::Detecting);
        let source = self.service.source(environment, &self.stop).await?;
        let installed = source
            .distributions
            .iter()
            .find(|entry| entry.name == self.distribution)
            .ok_or_else(|| {
                Fault::new(
                    ErrorCode::NotFound,
                    "WSL distribution is no longer installed",
                )
            })?;
        if installed.version != 2 {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                "Host requires a WSL2 distribution",
            ));
        }
        self.run(&source, environment, PREFLIGHT, Duration::from_secs(30))
            .await?;
        if self.stop.is_cancelled() {
            return Err(Fault::new(
                ErrorCode::Cancelled,
                "WSL Host installation was cancelled",
            ));
        }
        let version = env!("SAILRY_HOST_VERSION");
        let script = format!(
            "{PREFLIGHT}\n{}",
            deployment::script(version, false, network)?
        );
        self.progress.send_replace(InstallProgress::Installing);
        self.run(&source, environment, &script, Duration::from_secs(480)).await.map_err(|error| {
            if error.code == ErrorCode::Cancelled {
                unknown("Host installation was cancelled after launch; inspect the distribution before retrying")
            } else { error }
        })?;
        self.progress.send_replace(InstallProgress::Pairing);
        let pairing = async {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
            loop {
                let ticket = self
                    .run(&source, environment, TICKET, Duration::from_secs(5))
                    .await
                    .map_err(|_| {
                        unknown("Host installed but its bootstrap ticket could not be read")
                    })?;
                if !ticket.trim().is_empty() {
                    return pair(link, ticket.trim(), self.owner.1, network, version).await;
                }
                if tokio::time::Instant::now() >= deadline {
                    return Err(unknown(
                        "Host service started but pairing is unavailable; inspect the distribution service",
                    ));
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        };
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err(unknown("Host installed but pairing was cancelled; inspect the distribution before retrying")),
            result = tokio::time::timeout(Duration::from_secs(120), pairing) => result.unwrap_or_else(|_| Err(unknown("Host installed but pairing completion is unconfirmed"))),
        }
    }

    async fn run(
        &self,
        source: &Source,
        environment: &Environment,
        script: &str,
        deadline: Duration,
    ) -> Result<String, Fault> {
        command::run(
            launch(&source.executable, &self.distribution, environment),
            script,
            &self.stop,
            deadline,
        )
        .await
    }
}

#[cfg(test)]
mod tests;

fn launch(
    executable: &Path,
    distribution: &str,
    environment: &Environment,
) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(executable);
    environment.apply(&mut command);
    command.args([
        "--distribution",
        distribution,
        "--cd",
        "~",
        "--exec",
        "/bin/sh",
        "-s",
    ]);
    if let Some(parent) = executable.parent() {
        command.current_dir(parent);
    }
    command
}

async fn pair(
    link: &LinkHandle,
    ticket: &str,
    request: RequestId,
    network: Option<&Selection>,
    version: &str,
) -> Result<Outcome, Fault> {
    let address = link
        .pair(ticket)
        .await
        .map_err(|_| unknown("Host installed but pairing could not be confirmed"))?;
    let node = NodeId(*address.id.as_bytes());
    if address.id == link.address().id {
        return Err(unknown("Installed Host identity is not independent"));
    }
    let info = link
        .inspect(&address)
        .await
        .map_err(|_| unknown("Host installed but its endpoint could not be confirmed"))?;
    let current = info
        .software_version
        .as_deref()
        .and_then(|value| semver::Version::parse(value).ok())
        .zip(semver::Version::parse(version).ok())
        .is_some_and(|(installed, expected)| installed.cmp_precedence(&expected).is_eq());
    if !info.execution || !current {
        return Err(unknown(
            "Installed endpoint does not provide the expected Host release",
        ));
    }
    if let Some(selection) = network
        && deployment::configure_network(link, address, request, selection)
            .await
            .is_err()
    {
        return Ok(Outcome::NetworkUnconfirmed {
            node,
            selection: selection.clone(),
        });
    }
    Ok(Outcome::Installed { node })
}

fn unknown(message: &str) -> Fault {
    Fault::new(ErrorCode::OutcomeUnknown, message)
}
