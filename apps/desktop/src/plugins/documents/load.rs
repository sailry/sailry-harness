use super::*;
use gpui_kit::component::input::EditorState;

pub(super) struct Target {
    pub path: String,
    pub line: Option<usize>,
}

impl Controller {
    pub(super) fn open(
        &mut self,
        target: Target,
        context: plugin::Context,
        completion: Completion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.load(target, context, completion, true, window, cx);
    }

    pub(super) fn load(
        &mut self,
        target: Target,
        context: plugin::Context,
        completion: Completion,
        reveal: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Target { path, line } = target;
        let Completion { stop, reply } = completion;
        let intent = reveal.then(|| {
            self.intent += 1;
            self.intent
        });
        if let Some(id) = self
            .documents
            .iter()
            .find_map(|(id, document)| (document.path == path).then_some(*id))
        {
            if let Some(intent) = intent {
                self.documents.get_mut(&id).unwrap().dismissed = false;
                self.reveal(id, intent, line, window, cx);
            }
            if self.documents[&id].unsaved(cx) {
                let _ = reply.send(Ok(self.documents[&id].snapshot(id, cx)));
                return;
            }
        }
        self.read_sequence += 1;
        let sequence = self.read_sequence;
        self.reads.insert(path.clone(), sequence);
        let client = self.binding.client.clone();
        let cancellation = stop.clone();
        let task_path = path.clone();
        let job = self.binding.runtime.spawn(async move {
            client
                .read_document_scoped(&context, &task_path, cancellation)
                .await
        });
        cx.spawn_in(window, async move |controller, cx| {
            let result = job.await.unwrap_or_else(|_| Err(unavailable()));
            let mut reply = Some(reply);
            let _ = controller.update_in(cx, |controller, window, cx| {
                let reply = reply.take().unwrap();
                if stop.is_cancelled() || controller.reads.get(&path) != Some(&sequence) {
                    let _ = reply.send(Err(unavailable()));
                    return;
                }
                controller.reads.remove(&path);
                let content = match result {
                    Ok(content) => content,
                    Err(fault) => {
                        let _ = reply.send(Err(fault));
                        return;
                    }
                };
                let id = if let Some(id) = controller
                    .documents
                    .iter()
                    .find_map(|(id, document)| (document.path == path).then_some(*id))
                {
                    let document = controller.documents.get_mut(&id).unwrap();
                    if !document.unsaved(cx) {
                        document.input.update(cx, |input, cx| {
                            if input.value().as_ref() != content.text {
                                let selected = input.selected_range();
                                let scroll = input.scroll_offset();
                                input.set_value(content.text.clone(), window, cx);
                                input.set_selected_range(selected, cx);
                                input.set_scroll_offset(scroll, cx);
                            }
                        });
                        document.baseline = content.text;
                        document.revision = content.revision;
                        document.truncated = content.truncated;
                        document.error = None;
                    }
                    id
                } else {
                    if intent.is_none() {
                        let _ = reply.send(Err(unavailable()));
                        return;
                    }
                    let id = RequestId::new();
                    let input = cx.new(|cx| {
                        EditorState::new(window, cx)
                            .language(
                                std::path::Path::new(&path)
                                    .extension()
                                    .and_then(|value| value.to_str())
                                    .unwrap_or("text"),
                            )
                            .line_number(true)
                            .default_value(content.text.clone())
                    });
                    let markdown = path
                        .rsplit('.')
                        .next()
                        .is_some_and(|extension| {
                            ["md", "markdown", "mdown"]
                                .iter()
                                .any(|value| extension.eq_ignore_ascii_case(value))
                        })
                        .then(|| {
                            cx.new(|cx| crate::content::editor::State::from_input(&input, cx))
                        });
                    let mut observers =
                        vec![cx.observe(&input, |controller, _, cx| controller.changed(cx))];
                    if let Some(markdown) = &markdown {
                        observers
                            .push(cx.observe(markdown, |controller, _, cx| controller.changed(cx)));
                    }
                    controller.documents.insert(
                        id,
                        Document {
                            path,
                            input,
                            markdown,
                            baseline: content.text,
                            revision: content.revision,
                            truncated: content.truncated,
                            request: None,
                            submitted_text: None,
                            saving: false,
                            staging: None,
                            dismissed: false,
                            uncertain: false,
                            error: None,
                            stop: controller.stop.child_token(),
                            _observers: observers,
                        },
                    );
                    id
                };
                if let Some(intent) = intent {
                    controller.reveal(id, intent, line, window, cx);
                } else {
                    controller.changed(cx);
                }
                let _ = reply.send(Ok(controller.documents[&id].snapshot(id, cx)));
            });
        })
        .detach();
    }
}
