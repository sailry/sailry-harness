use super::*;

impl Controller {
    pub(super) fn request_close(
        &mut self,
        id: RequestId,
        discard: bool,
        confirm: bool,
        completion: Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Completion { stop, reply } = completion;
        let Some(document) = self.documents.get(&id) else {
            let _ = reply.send(Err(unavailable()));
            return;
        };
        if discard || !confirm || !document.unsaved(cx) {
            let _ = reply.send(self.close(id, discard, cx).map(Value::Bool));
            return;
        }
        let path = document.path.clone();
        let response = crate::prompts::ask(
            PromptLevel::Warning,
            &crate::tr("files_discard_title"),
            &format!("{path}\n\n{}", crate::tr("files_discard_description")),
            &[crate::tr("settings_cancel"), crate::tr("files_discard")],
            window,
            cx,
        );
        cx.spawn_in(window, async move |controller, cx| {
            let selection = response.await;
            let result = controller
                .update_in(cx, |controller, _, cx| {
                    if stop.is_cancelled()
                        || controller
                            .documents
                            .get(&id)
                            .is_none_or(|document| document.path != path)
                    {
                        return Err(unavailable());
                    }
                    if selection != Some(1) {
                        return Ok(Value::Bool(false));
                    }
                    controller.close(id, true, cx).map(Value::Bool)
                })
                .unwrap_or_else(|_| Err(unavailable()));
            let _ = reply.send(result);
        })
        .detach();
    }
}
