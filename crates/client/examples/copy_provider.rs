#[path = "copy_provider/controller.rs"]
mod controller;
#[path = "copy_provider/transfer.rs"]
mod transfer;

#[cfg(test)]
#[path = "copy_provider/tests.rs"]
mod tests;

use sailry_client::Client;
use sailry_protocol::NodeId;
use std::{path::PathBuf, process::ExitCode};
use transfer::{NAME, Result};

const HELP: &str = "Temporary, controller-only provider acceptance helper
Required: --controller-dir ABSOLUTE_NEW_DIRECTORY --source-node HEX --target-node HEX
Default: prepare public metadata only, without reading a provider key
Pairing: --pair-source / --pair-target read one app-authorized code or invitation from stdin
Network: --pairing-service URL; --data-relay URL (repeatable; otherwise iroh defaults)
Copy and validate: --copy-to-node HEX --provider-name NAME --provider-revision NUMBER --validate-target
Validate an existing target without copying: --provider-name NAME --validate-target
Only the exact, unique approved provider name is accepted. IDs in output are redacted.
Keep the same controller directory for preparation and copy. A copy.once marker prevents
another copy attempt, including after an uncertain result. Never remove it to retry an unknown save.";

struct Options {
    directory: PathBuf,
    source: NodeId,
    target: NodeId,
    pair_source: bool,
    pair_target: bool,
    service: String,
    relays: Vec<String>,
    mode: Mode,
}

enum Mode {
    Prepare,
    Copy { revision: u64 },
    Validate,
}

impl Options {
    fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut directory = None;
        let mut source = None;
        let mut target = None;
        let mut approved = None;
        let mut provider = None;
        let mut revision = None;
        let mut pair_source = false;
        let mut pair_target = false;
        let mut validate = false;
        let mut service = None;
        let mut relays = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            if argument != "--data-relay" && !seen.insert(argument.clone()) {
                return Err("duplicate_option");
            }
            match argument.as_str() {
                "--pair-source" => pair_source = true,
                "--pair-target" => pair_target = true,
                "--validate-target" => validate = true,
                _ => {
                    let value = arguments.next().ok_or("option_value_required")?;
                    match argument.as_str() {
                        "--controller-dir" => directory = Some(PathBuf::from(value)),
                        "--source-node" => source = Some(node_id(&value)?),
                        "--target-node" => target = Some(node_id(&value)?),
                        "--copy-to-node" => approved = Some(node_id(&value)?),
                        "--provider-name" => provider = Some(value),
                        "--provider-revision" => {
                            revision = Some(value.parse::<u64>().map_err(|_| "invalid_revision")?)
                        }
                        "--pairing-service" => service = Some(value),
                        "--data-relay" => relays.push(value),
                        _ => return Err("unknown_option"),
                    }
                }
            }
        }
        let source = source.ok_or("source_node_required")?;
        let target = target.ok_or("target_node_required")?;
        if source == target {
            return Err("source_and_target_must_differ");
        }
        let mode = match approved {
            Some(approved) => {
                if approved != target {
                    return Err("approved_target_mismatch");
                }
                if provider.as_deref() != Some(NAME) || !validate {
                    return Err("exact_provider_and_validation_approval_required");
                }
                let revision = revision
                    .filter(|revision| *revision > 0)
                    .ok_or("source_provider_revision_required")?;
                Mode::Copy { revision }
            }
            None if validate => {
                if provider.as_deref() != Some(NAME) || revision.is_some() {
                    return Err("exact_provider_required_for_target_validation");
                }
                Mode::Validate
            }
            None => {
                if provider.is_some() || revision.is_some() {
                    return Err("copy_or_validation_approval_required");
                }
                Mode::Prepare
            }
        };
        let directory = directory.ok_or("controller_directory_required")?;
        if !directory.is_absolute() {
            return Err("absolute_controller_directory_required");
        }
        Ok(Self {
            directory,
            source,
            target,
            pair_source,
            pair_target,
            service: service.unwrap_or_else(|| sailry_link::rendezvous::DEFAULT_SERVICE.into()),
            relays,
            mode,
        })
    }
}

fn node_id(value: &str) -> Result<NodeId> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("node_id_requires_64_hex_digits");
    }
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| "invalid_node_id")?;
    }
    Ok(NodeId(bytes))
}

#[tokio::main]
async fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|_| eprintln!("status=panic_redacted")));
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments == ["--help"] {
        println!("{HELP}");
        return ExitCode::SUCCESS;
    }
    let result = match Options::parse(arguments) {
        Ok(options) => run(options).await,
        Err(status) => Err(status),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(status) => {
            eprintln!("status={status}");
            ExitCode::FAILURE
        }
    }
}

async fn run(options: Options) -> Result<()> {
    let fresh = controller::check_directory(&options.directory)?;
    let scope = if options.relays.is_empty() {
        sailry_link::NetworkScope::Internet
    } else {
        sailry_link::NetworkScope::CustomRelays(options.relays.clone())
    };
    let link = sailry_link::Link::controller(&options.directory, scope)
        .await
        .map_err(|_| "controller_link_unavailable")?;
    let handle = link.handle();
    let result = async {
        if fresh {
            controller::mark_directory(&options.directory)?;
        }
        let source = controller::connect(
            &handle,
            options.source,
            options.pair_source,
            &options.service,
            "source",
            "macos",
        )
        .await?;
        let target = controller::connect(
            &handle,
            options.target,
            options.pair_target,
            &options.service,
            "target",
            "windows",
        )
        .await?;
        let source = Client::new(handle.remote(source));
        let target = Client::new(handle.remote(target));
        match options.mode {
            Mode::Prepare => {
                transfer::prepare(&source, &target, None).await?;
                println!("status=prepared_without_key_read");
                Ok(())
            }
            Mode::Copy { revision } => {
                let plan = transfer::prepare(&source, &target, Some(revision)).await?;
                controller::reserve_copy(&options.directory)?;
                transfer::copy(&source, &target, plan).await
            }
            Mode::Validate => transfer::validate_existing(&source, &target).await,
        }
    }
    .await;
    let closed = link.close().await.map_err(|_| "controller_close_failed");
    result.and(closed)
}
