use super::*;
use sailry_protocol::Output;

impl Controller {
    pub(super) fn save(
        &mut self,
        id: RequestId,
        context: plugin::Context,
        stop: CancellationToken,
        reply: Reply,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .documents
            .get(&id)
            .is_some_and(|document| self.locked(&document.path, cx))
        {
            let _ = reply.send(Err(Fault::new(
                ErrorCode::Busy,
                "file operation is in progress",
            )));
            return;
        }
        let Some(document) = self.documents.get_mut(&id) else {
            let _ = reply.send(Err(unavailable()));
            return;
        };
        let Some(revision) = document.revision.clone() else {
            let _ = reply.send(Err(Fault::new(
                ErrorCode::PermissionDenied,
                "document is read-only",
            )));
            return;
        };
        if document.saving || (!document.dirty(cx) && !document.uncertain) {
            let _ = reply.send(Ok(document.snapshot(id, cx)));
            return;
        }
        let worktree = self.binding.worktree.expect("document worktree");
        let path = document.path.clone();
        let draft = document.input.read(cx).value().to_string();
        let text = if document.uncertain {
            document
                .submitted_text
                .clone()
                .unwrap_or_else(|| draft.clone())
        } else {
            draft
        };
        let pending = document
            .request
            .as_ref()
            .filter(|request| {
                document.uncertain || request.matches(worktree, &path, &text, &revision)
            })
            .cloned();
        if document.uncertain && pending.is_none() {
            let _ = reply.send(Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "the previous save outcome is unknown",
            )));
            return;
        }
        document.saving = true;
        document.error = None;
        let stop = stop.child_token();
        document.staging = Some(stop.clone());
        let client = self.binding.client.clone();
        let runtime = self.binding.runtime.clone();
        let staging = runtime.spawn({
            let client = client.clone();
            let text = text.clone();
            async move {
                match pending {
                    Some(pending) => Ok(pending),
                    None => {
                        client
                            .stage_document_scoped(&context, &path, &text, &revision, stop)
                            .await
                    }
                }
            }
        });
        self.changed(cx);
        cx.spawn_in(window, async move |controller, cx| {
            let staged = staging.await.unwrap_or_else(|_| Err(unavailable()));
            let mut publication = None;
            let mut reply = Some(reply);
            let _ = controller.update_in(cx, |controller, _, cx| {
                let Some(document) = controller.documents.get_mut(&id) else {
                    return;
                };
                document.staging = None;
                match staged {
                    Ok(pending) => {
                        let request = pending.request.clone();
                        let request_id = request.id;
                        // Recovery identity is retained before durable admission can happen.
                        document.request = Some(pending);
                        document.submitted_text = Some(text.clone());
                        publication = Some((
                            request_id,
                            runtime.spawn(async move { client.execute(request).await }),
                        ));
                    }
                    Err(fault) => {
                        document.saving = false;
                        document.error = Some(fault.clone());
                        if let Some(reply) = reply.take() {
                            let _ = reply.send(Err(fault));
                        }
                    }
                }
                controller.changed(cx);
            });
            let Some((request_id, publication)) = publication else {
                return;
            };
            // Closing a renderer must not abandon an admitted publication's outcome.
            let result = publication.await.unwrap_or_else(|_| {
                Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "document save outcome is unknown",
                ))
            });
            let _ = controller.update_in(cx, |controller, _, cx| {
                let Some(document) = controller.documents.get_mut(&id).filter(|document| {
                    document
                        .request
                        .as_ref()
                        .is_some_and(|request| request.request.id == request_id)
                }) else {
                    return;
                };
                document.saving = false;
                let result = match result {
                    Ok(Output::FileWritten(written)) => {
                        document.baseline = text;
                        document.revision = Some(written.revision);
                        document.request = None;
                        document.submitted_text = None;
                        document.uncertain = false;
                        document.error = None;
                        Ok(document.snapshot(id, cx))
                    }
                    result => {
                        let fault = match result {
                            Err(fault) => fault,
                            _ => Fault::new(
                                ErrorCode::OutcomeUnknown,
                                "document save response is unavailable",
                            ),
                        };
                        document.uncertain = fault.code == ErrorCode::OutcomeUnknown;
                        document.error = Some(fault.clone());
                        Err(fault)
                    }
                };
                if document.dismissed && !document.uncertain {
                    controller.documents.remove(&id);
                }
                controller.changed(cx);
                if let Some(reply) = reply.take() {
                    let _ = reply.send(result);
                }
            });
        })
        .detach();
    }
}
