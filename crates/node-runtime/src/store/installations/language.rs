//! Language packages keep their existing physical installer and managed receipts.
use super::{Execution, Worker};
use crate::store::{Ingress, resources};
use sailry_protocol::{
    Command,
    installation::{Phase, Resource},
};
use std::sync::Arc;

pub(in crate::store) fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::InstallCodeServer { .. } | Command::UninstallCodeServer { .. }
    )
}

pub(in crate::store) fn submit(
    worker: &mut Worker,
    ingress: Arc<Ingress>,
    execution: resources::Execution,
) -> Result<(), Box<Execution>> {
    let (package, version, observe) = match &execution.request.command {
        Command::InstallCodeServer { install } => (&install.package, &install.version, true),
        Command::UninstallCodeServer { package, version } => (package, version, false),
        _ => unreachable!("language installation or removal expected"),
    };
    let resource = Resource::Language {
        package: package.clone(),
        version: version.clone(),
    };
    let request = execution.request.clone();
    worker.submit(
        Execution {
            caller: execution.caller,
            request: execution.request,
            reply: execution.reply,
            operation: true,
            observe,
        },
        resource,
        move |reporter, stop| async move {
            reporter(Phase::Inspecting, None);
            ingress.change_code(&request, &stop, Some(&reporter)).await
        },
    )
}
