//! Package acquisition and dependency preparation stay in their plugin domain.
use crate::{
    installation::Reporter,
    store::{
        Ingress, Job,
        installations::{Execution, Worker},
        mutations::Mutation,
        unavailable,
    },
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, ErrorCode, Fault, NodeId, Output, Request,
    installation::{Phase, Resource},
    plugin::Info,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::oneshot;

pub(in crate::store) fn handles(command: &Command) -> bool {
    name(command).is_some()
}

fn name(command: &Command) -> Option<&str> {
    match command {
        Command::InstallPluginUpload { name, .. }
        | Command::InstallPlugin { name, .. }
        | Command::InstallPluginSource { name, .. }
        | Command::InstallBundledPlugin { name, .. }
        | Command::InstallSkill { name, .. }
        | Command::InstallMcp { name, .. }
        | Command::SetPluginEnabled {
            name,
            enabled: true,
            ..
        } => Some(name),
        _ => None,
    }
}

pub(in crate::store) fn submit(
    worker: &mut Worker,
    ingress: Arc<Ingress>,
    mutation: Mutation,
) -> Result<(), Box<Execution>> {
    let resource = Resource::Plugin {
        name: name(&mutation.request.command)
            .expect("plugin installation command expected")
            .into(),
    };
    let request = mutation.request.clone();
    let root = mutation.roots.target.clone();
    let caller = mutation.caller;
    let execution = Execution {
        caller,
        request: mutation.request,
        reply: mutation.reply,
        operation: false,
    };
    worker.submit(execution, resource, move |reporter, stop| async move {
        let info = acquire(&ingress, caller, &request, root, &stop, &reporter).await?;
        check(&stop)?;
        if let Command::InstallPlugin {
            expected_digest: Some(expected),
            ..
        } = &request.command
        {
            reporter(Phase::Validating, None);
            if info.summary.digest != *expected {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "plugin source changed; check updates again",
                ));
            }
        }
        ingress
            .plugins
            .prepare_python(&info, &ingress.environment, &stop, Some(&reporter))
            .await?;
        check(&stop)?;
        Ok(Output::Plugin(info))
    })
}

async fn acquire(
    ingress: &Ingress,
    caller: NodeId,
    request: &Request,
    root: PathBuf,
    stop: &CancellationToken,
    reporter: &Reporter,
) -> Result<Info, Fault> {
    match &request.command {
        Command::InstallPluginSource {
            source, path, name, ..
        } => {
            ingress
                .plugins
                .install_source(source, path, name, stop.clone(), Some(reporter.clone()))
                .await
        }
        Command::InstallSkill {
            source, path, name, ..
        } => {
            ingress
                .plugins
                .install_skill(source, path, name, stop.clone(), Some(reporter.clone()))
                .await
        }
        Command::SetPluginEnabled {
            name,
            expected_revision,
            enabled: true,
        } => {
            reporter(Phase::Inspecting, None);
            let (reply, response) = oneshot::channel();
            ingress
                .sender
                .send(Job::PluginPreparation {
                    name: name.clone(),
                    revision: *expected_revision,
                    reply,
                })
                .await
                .map_err(|_| unavailable())?;
            tokio::select! {
                biased;
                _ = stop.cancelled() => Err(cancelled()),
                result = response => result.map_err(|_| unavailable())?,
            }
        }
        _ => {
            reporter(Phase::Inspecting, None);
            let host = ingress.plugins.clone();
            let transfers = ingress.transfers.clone();
            let command = request.command.clone();
            tokio::task::spawn_blocking(move || match command {
                Command::InstallPluginUpload { stream, name, .. } => {
                    transfers.install_plugin(caller, stream, &name, &host)
                }
                Command::InstallPlugin { path, name, .. } => host.install(&root, &path, &name),
                Command::InstallBundledPlugin { name, .. } => host.install_bundled(&name),
                Command::InstallMcp {
                    name, definition, ..
                } => host.install_mcp(&name, &definition),
                _ => unreachable!("plugin acquisition command expected"),
            })
            .await
            .map_err(|_| unavailable())?
        }
    }
}

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "plugin preparation cancelled")
}

fn check(stop: &CancellationToken) -> Result<(), Fault> {
    if stop.is_cancelled() {
        Err(cancelled())
    } else {
        Ok(())
    }
}
