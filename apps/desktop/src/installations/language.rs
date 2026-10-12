//! Captured language requests reuse the common installation presenter and Client.
use super::*;
use sailry_protocol::{RequestOutcome, code::InstallStatus};

pub(crate) fn completion(request: &Request, result: &Result<Output, Fault>) -> Result<(), Fault> {
    match (&request.command, result) {
        (Command::InstallCodeServer { install }, Ok(Output::CodeInstallation(state)))
            if state.package == install.package
                && state.status == InstallStatus::Ready
                && state.receipt.as_ref().is_some_and(|receipt| {
                    receipt.package == install.package
                        && receipt.version == install.version
                        && receipt.revision == install.revision
                        && receipt.purl == install.purl
                }) =>
        {
            Ok(())
        }
        (_, Err(error)) => Err(error.clone()),
        _ => Err(Fault::new(
            ErrorCode::OutcomeUnknown,
            "installation result is unconfirmed",
        )),
    }
}

fn track_request(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    request: &Request,
    window: &mut Window,
    cx: &mut App,
) -> Option<Ticket> {
    if !matches!(request.command, Command::InstallCodeServer { .. }) {
        return None;
    }
    let captured = request.clone();
    let retry_client = client.clone();
    let retry_runtime = runtime.clone();
    track(
        client,
        runtime,
        request,
        false,
        move |attempt, window, cx| {
            let request = attempt.request(&retry_client, &captured);
            dispatch(
                retry_client.clone(),
                retry_runtime.clone(),
                request,
                window,
                cx,
            );
        },
        window,
        cx,
    )
}

fn dispatch(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    request: Request,
    window: &mut Window,
    cx: &mut App,
) {
    let ticket = track_request(client.clone(), runtime.clone(), &request, window, cx);
    let captured = request.clone();
    let job = runtime.spawn(async move {
        let mut result = client.execute(request.clone()).await;
        if result.is_err() {
            loop {
                match client.outcome(&request).await {
                    Ok(RequestOutcome::Completed(confirmed)) => {
                        result = *confirmed;
                        break;
                    }
                    Ok(RequestOutcome::Admitted) => {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await
                    }
                    _ => {
                        return Err(Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "installation result is unconfirmed",
                        ));
                    }
                }
            }
        }
        result
    });
    let origin = window.window_handle();
    cx.spawn(async move |cx| {
        let outcome = match job.await {
            Ok(result) => completion(&captured, &result),
            Err(_) => Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "installation result is unconfirmed",
            )),
        };
        cx.update(|cx| {
            if let Some(ticket) = &ticket {
                ticket.record_outcome(outcome.clone(), cx);
            }
        });
        if let Some(target) = cx.update(|cx| target(origin, cx)) {
            let _ = target.update(cx, |_, window, cx| {
                if let Some(ticket) = &ticket {
                    ticket.close_empty(window, cx);
                }
                if let Err(error) = outcome {
                    crate::feedback::error(
                        "",
                        tr(if error.code == ErrorCode::OutcomeUnknown {
                            "installation_uncertain"
                        } else {
                            "installation_failed"
                        })
                        .as_ref(),
                        window,
                        cx,
                    );
                } else {
                    crate::feedback::info("", tr("code_installed").as_ref(), window, cx);
                }
            });
        }
    })
    .detach();
}

pub(crate) fn external(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    request: &Request,
    window: &mut Window,
    cx: &mut App,
) -> Option<tokio::sync::oneshot::Sender<Result<(), Fault>>> {
    let ticket = track_request(client, runtime, request, window, cx)?;
    let origin = window.window_handle();
    let (sender, receive) = tokio::sync::oneshot::channel();
    cx.spawn(async move |cx| {
        let outcome = receive.await.unwrap_or_else(|_| {
            Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "installation result is unconfirmed",
            ))
        });
        cx.update(|cx| ticket.record_outcome(outcome, cx));
        if let Some(target) = cx.update(|cx| target(origin, cx)) {
            let _ = target.update(cx, |_, window, cx| ticket.close_empty(window, cx));
        }
    })
    .detach();
    Some(sender)
}

#[cfg(test)]
mod tests;
