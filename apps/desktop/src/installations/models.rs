use super::*;
use sailry_protocol::{RequestOutcome, models::Kind};

pub(super) fn resource(command: &Command) -> Option<Resource> {
    match command {
        Command::InstallOcrModel { id } => Some(Resource::Model {
            kind: Kind::Ocr,
            id: id.clone(),
        }),
        Command::InstallKnowledgeModel { model } => Some(Resource::Model {
            kind: Kind::Embedding,
            id: model.key().into_owned(),
        }),
        _ => None,
    }
}

pub(crate) fn model(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    request: &Request,
    window: &mut Window,
    cx: &mut App,
) -> Option<Ticket> {
    resource(&request.command)?;
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

pub(crate) fn completion(
    result: &Result<(Result<Output, Fault>, bool), tokio::task::JoinError>,
) -> Result<(), Fault> {
    match result {
        Ok((Ok(Output::OcrModel { .. } | Output::KnowledgeModel { .. }), _)) => Ok(()),
        Ok((Err(error), true)) => Err(error.clone()),
        _ => Err(Fault::new(
            ErrorCode::OutcomeUnknown,
            "installation result is unconfirmed",
        )),
    }
}

fn dispatch(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    request: Request,
    window: &mut Window,
    cx: &mut App,
) {
    let ticket = model(client.clone(), runtime.clone(), &request, window, cx);
    let job = runtime.spawn(async move {
        let result = client.execute(request.clone()).await;
        if result.is_err() {
            match client.outcome(&request).await {
                Ok(RequestOutcome::Completed(result)) => (*result, true),
                _ => (result, false),
            }
        } else {
            (result, true)
        }
    });
    let origin = window.window_handle();
    cx.spawn(async move |cx| {
        let result = job.await;
        let outcome = completion(&result);
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
                crate::feedback::info(
                    "",
                    tr(if outcome.is_ok() {
                        "knowledge_installed"
                    } else if outcome
                        .as_ref()
                        .is_err_and(|error| error.code == ErrorCode::OutcomeUnknown)
                    {
                        "installation_uncertain"
                    } else {
                        "installation_failed"
                    })
                    .as_ref(),
                    window,
                    cx,
                );
            });
        }
    })
    .detach();
}

pub(crate) fn retry_model(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    request: Request,
    window: &mut Window,
    cx: &mut App,
) {
    dispatch(client, runtime, request, window, cx);
}

pub(crate) fn external(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    request: &Request,
    window: &mut Window,
    cx: &mut App,
) -> Option<tokio::sync::oneshot::Sender<Result<(), Fault>>> {
    let ticket = model(client, runtime, request, window, cx)?;
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
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn normalizes_only_unconfirmed_results() {
        let error = Fault::new(ErrorCode::Unavailable, "model service is unavailable");
        assert_eq!(completion(&Ok((Err(error.clone()), true))), Err(error));
        assert_eq!(
            completion(&Ok((
                Err(Fault::new(
                    ErrorCode::Unavailable,
                    "transport is unavailable"
                )),
                false
            )))
            .unwrap_err()
            .code,
            ErrorCode::OutcomeUnknown
        );
    }
}
