use super::*;
use sailry_protocol::RequestOutcome;

pub(crate) fn model(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    request: &Request,
    resource: Resource,
    window: &mut Window,
    cx: &mut App,
) -> Option<Ticket> {
    let captured = request.clone();
    let retry_client = client.clone();
    let retry_runtime = runtime.clone();
    track_resource(
        client,
        runtime,
        Capture {
            request,
            resource: Some(resource),
            waiting: false,
        },
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
        Ok((Ok(Output::LocalModel { .. }), _)) => Ok(()),
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
    let feedback = external(client.clone(), runtime.clone(), &request, window, cx);
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
        if let Some(feedback) = feedback {
            let _ = feedback.send(outcome.clone());
        }
        if let Some(target) = cx.update(|cx| target(origin, cx)) {
            let _ = target.update(cx, |_, window, cx| {
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
    let Command::InstallLocalModel { id } = &request.command else {
        return None;
    };
    let id = id.clone();
    let request = request.clone();
    let read = client.clone();
    let job = runtime.spawn(async move {
        match read
            .execute(read.prepare(Command::ReadLocalModels { kind: None }))
            .await
        {
            Ok(Output::LocalModels(sailry_protocol::models::Catalog { models, .. })) => models
                .into_iter()
                .find(|model| model.id == id)
                .map(|model| Resource::Model {
                    kind: model.definition.format.kind(),
                    id,
                }),
            _ => None,
        }
    });
    let origin = window.window_handle();
    let (sender, receive) = tokio::sync::oneshot::channel();
    cx.spawn(async move |cx| {
        let resource = job.await.ok().flatten();
        let ticket = if let Some(resource) = resource {
            cx.update(|cx| target(origin, cx)).and_then(|target| {
                target
                    .update(cx, |_, window, cx| {
                        model(client, runtime, &request, resource, window, cx)
                    })
                    .ok()
                    .flatten()
            })
        } else {
            None
        };
        let outcome = receive.await.unwrap_or_else(|_| {
            Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "installation result is unconfirmed",
            ))
        });
        cx.update(|cx| {
            if let Some(ticket) = &ticket {
                ticket.record_outcome(outcome, cx);
            }
        });
        if let Some(target) = cx.update(|cx| target(origin, cx)) {
            let _ = target.update(cx, |_, window, cx| {
                if let Some(ticket) = &ticket {
                    ticket.close_empty(window, cx);
                }
            });
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
