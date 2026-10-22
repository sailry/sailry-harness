//! Independent metadata reads preserve inventory provenance and isolate failures.
use futures::{FutureExt as _, StreamExt as _, stream::FuturesUnordered};
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, ErrorCode, Fault, Output,
    plugin::{Info, Summary},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;

pub(super) struct Batch {
    pub results: Vec<(String, Result<Info, &'static str>)>,
    pub retrying: bool,
    pub diagnostics: std::collections::BTreeMap<String, Fault>,
}

async fn read(
    client: Arc<Client>,
    expected: Summary,
    attempt: u8,
) -> (Summary, u8, Result<Info, Fault>) {
    if attempt > 0 {
        tokio::time::sleep(Duration::from_millis(250 * (1 << (attempt - 1)))).await;
    }
    let result = match tokio::time::timeout(
        Duration::from_secs(10),
        client.execute(client.prepare(Command::ReadPlugin {
            name: expected.name.clone(),
        })),
    )
    .await
    {
        Ok(Ok(Output::Plugin(info))) if info.summary == expected => Ok(info),
        Ok(Ok(Output::Plugin(_))) => {
            Err(Fault::new(ErrorCode::Conflict, "plugin metadata changed"))
        }
        Ok(Err(error)) => Err(error),
        Ok(Ok(_)) => Err(Fault::invalid("unexpected plugin metadata output")),
        Err(_) => Err(Fault::new(
            ErrorCode::Unavailable,
            "plugin metadata read timed out",
        )),
    };
    (expected, attempt, result)
}

pub(super) async fn load(
    client: Arc<Client>,
    missing: Vec<Summary>,
    stop: CancellationToken,
    sender: mpsc::Sender<Batch>,
) {
    let mut pending = FuturesUnordered::new();
    for expected in missing {
        pending.push(read(client.clone(), expected, 0).boxed());
    }
    loop {
        let Some((expected, attempt, result)) = (tokio::select! {
            biased;
            _ = stop.cancelled() => return,
            result = pending.next() => result,
        }) else {
            return;
        };
        if result
            .as_ref()
            .is_err_and(|error| matches!(error.code, ErrorCode::Busy | ErrorCode::Unavailable))
            && attempt < 2
        {
            pending.push(read(client.clone(), expected.clone(), attempt + 1).boxed());
            continue;
        }
        if let Err(error) = &result {
            use sailry_protocol::notification::{DiagnosticPlugin, SystemDiagnostic, SystemSource};
            let mut report = SystemDiagnostic::new(
                sailry_protocol::RequestId::new(),
                SystemSource::PluginMetadata,
                error.code,
            );
            if let Some(version) = &expected.version {
                report.plugin = Some(DiagnosticPlugin {
                    name: expected.name.clone(),
                    version: version.clone(),
                });
            }
            report.operation = Some("metadata_read".into());
            let mut entry = sailry_node_runtime::logging::Entry::new(
                sailry_node_runtime::logging::Area::Plugin,
                sailry_node_runtime::logging::Phase::Failed,
            );
            entry.context.node = Some(client.target());
            entry.context.package = Some(expected.reference());
            entry.context.request = Some(report.incident);
            entry.reason = Some("metadata_read");
            entry.code = Some(error.code);
            entry.emit();
            let reporting = client.clone();
            let reporting_stop = stop.clone();
            // Diagnostic delivery cannot hold successful, unrelated metadata reads.
            tokio::spawn(async move {
                tokio::select! { biased; _ = reporting_stop.cancelled() => {}, _ = reporting.report_diagnostic(report) => {} }
            });
        }
        let mut diagnostics = std::collections::BTreeMap::new();
        let result = result.map_err(|error| {
            let key = if error.code == ErrorCode::Conflict {
                "plugins_conflict"
            } else {
                "plugins_read_failed"
            };
            diagnostics.insert(expected.name.clone(), error);
            key
        });
        let batch = Batch {
            results: vec![(expected.name, result)],
            retrying: !pending.is_empty(),
            diagnostics,
        };
        tokio::select! {
            biased;
            _ = stop.cancelled() => return,
            result = sender.send(batch) => if result.is_err() { return; },
        }
    }
}
