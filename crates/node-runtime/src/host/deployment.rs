//! Official release bootstrap shared by SSH and installed WSL distributions.
use sailry_link::LinkHandle;
use sailry_protocol::{ErrorCode, Fault, NodeId, RequestId, network::Selection};

pub(crate) fn script(
    version: &str,
    update: bool,
    network: Option<&Selection>,
) -> Result<String, Fault> {
    let mut arguments = Vec::new();
    match network {
        Some(Selection::Iroh) => arguments.push("'--internet'".into()),
        Some(Selection::Custom { relays }) => {
            for relay in relays {
                if relay.contains('\0') {
                    return Err(Fault::invalid("relay URL contains a null byte"));
                }
                arguments.push("'--relay'".into());
                arguments.push(format!("'{}'", relay.replace('\'', "'\"'\"'")));
            }
        }
        None => {}
    }
    Ok(format!(
        "install_version='{version}'\ninstall_update={}\nset -- {}\n{}",
        if update { "1" } else { "0" },
        arguments.join(" "),
        normalize(include_str!("deployment/release.sh"))
    ))
}

pub(crate) fn normalize(source: &str) -> String {
    source.replace("\r\n", "\n")
}

pub(crate) async fn configure_network(
    link: &LinkHandle,
    address: sailry_link::EndpointAddr,
    id: RequestId,
    selected: &Selection,
) -> Result<(), Fault> {
    use sailry_protocol::{Command, Output, Request};
    let target = NodeId(*address.id.as_bytes());
    let remote = link.remote(address);
    let read = || Request::new(target, Command::ReadNetworkSettings);
    let admission = remote.dispatch(read()).await?;
    let Output::NetworkSettings(settings) = admission
        .completion
        .await
        .map_err(|_| unknown("network settings response was lost"))??
    else {
        return Err(Fault::invalid("network settings response expected"));
    };
    if settings.revision > 0 {
        if settings.selection.as_ref() == Some(selected)
            && settings.effective.as_ref() == Some(selected)
        {
            return Ok(());
        }
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "installed Host network changed",
        ));
    }
    let mut request = Request::new(
        target,
        Command::SaveNetworkSettings {
            expected_revision: 0,
            selection: selected.clone(),
        },
    );
    request.id = id;
    let admission = remote.dispatch(request).await?;
    let Output::NetworkSettings(saved) = admission
        .completion
        .await
        .map_err(|_| unknown("network change response was lost"))??
    else {
        return Err(Fault::invalid("network settings response expected"));
    };
    let actual: sailry_link::EndpointAddr = serde_json::from_str(&saved.address)
        .map_err(|_| Fault::invalid("invalid Host network address"))?;
    if actual.id.as_bytes() != &target.0 {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "network settings belong to another Host",
        ));
    }
    link.update_peer(actual).await?;
    let admission = remote.dispatch(read()).await?;
    let Output::NetworkSettings(verified) = admission
        .completion
        .await
        .map_err(|_| unknown("network verification response was lost"))??
    else {
        return Err(Fault::invalid("network settings response expected"));
    };
    if verified.revision == 0
        || verified.selection.as_ref() != Some(selected)
        || verified.effective.as_ref() != Some(selected)
    {
        return Err(unknown("Host network change could not be confirmed"));
    }
    Ok(())
}

fn unknown(message: &str) -> Fault {
    Fault::new(ErrorCode::OutcomeUnknown, message)
}

#[cfg(test)]
mod tests;
