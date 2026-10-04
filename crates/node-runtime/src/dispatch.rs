//! A headless callback worker; no view, connection or plugin panel owns its lifetime.
use crate::{Error, store::Ingress, tasks::Tasks};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault};
use std::{sync::Arc, time::Duration};
use tokio::{sync::Notify, task::JoinSet};

mod completion;
pub(crate) use completion::turn_result;

#[derive(Default)]
pub(crate) struct Controls {
    pub wake: Notify,
    pub stop: CancellationToken,
}

pub(crate) fn start(ingress: Arc<Ingress>, tasks: &Tasks) -> Result<(), Error> {
    tasks.spawn(async move {
        serve(ingress).await;
    })?;
    Ok(())
}

async fn serve(ingress: Arc<Ingress>) {
    let mut running = JoinSet::new();
    let mut recover = true;
    loop {
        let notified = ingress.dispatch.wake.notified();
        let batch = match ingress.dispatch_tick(recover).await {
            Ok(batch) => batch,
            Err(_) if ingress.dispatch.stop.is_cancelled() => break,
            Err(error) => {
                eprintln!("dispatch service failed: {error}");
                tokio::select! {
                    _ = ingress.dispatch.stop.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
                }
            }
        };
        recover = false;
        for work in batch.work {
            let ingress = ingress.clone();
            running.spawn(async move {
                let result = match ingress.dispatch_internal(work.request).await {
                    Ok(admission) => admission.completion.await.unwrap_or_else(|_| {
                        Err(Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "callback result was not received",
                        ))
                    }),
                    Err(error) => Err(error),
                };
                let result = tokio::select! {
                    biased;
                    _ = ingress.dispatch.stop.cancelled() => return,
                    result = completion::wait(&ingress, work.completion, result) => result,
                };
                if let Err(error) = ingress.dispatch_finished(work.job, result).await {
                    eprintln!("callback completion was not recorded: {error}");
                }
            });
        }
        if batch.pending {
            if ingress.dispatch.stop.is_cancelled() {
                break;
            }
            continue;
        }
        let delay = batch.next_ms.map(|due| {
            Duration::from_millis((due - chrono::Utc::now().timestamp_millis()).max(1) as u64)
        });
        tokio::select! {
            _ = ingress.dispatch.stop.cancelled() => break,
            _ = notified => {},
            _ = running.join_next(), if !running.is_empty() => {},
            _ = async { match delay { Some(delay) => tokio::time::sleep(delay).await, None => std::future::pending().await } } => {},
        }
    }
    // Ordinary shutdown drains admitted callback completions. The shared
    // supervisor bounds shutdown; interrupted jobs are resolved from receipts.
    while running.join_next().await.is_some() {}
}
