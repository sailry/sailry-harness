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
                self.reads.invalidate(&path);
                let _ = reply.send(Ok(self.documents[&id].snapshot(id, cx)));
                return;
            }
        }
        let sequence = self.reads.start(path.clone(), stop.clone());
        let client = self.binding.client.clone();
        let cancellation = stop.as_ref().clone();
        let task_path = path.clone();
        let task_context = context.clone();
        let job = self.binding.runtime.spawn(async move {
            let worktree = task_context
                .worktree
                .ok_or_else(|| Fault::invalid("document requires a worktree"))?;
            let markdown = crate::content::files::markdown(&task_path);
            let document = super::notebook::load(
                client.clone(),
                worktree,
                task_path,
                Some(task_context),
                cancellation.clone(),
            )
            .await?;
            let root = if markdown {
                let request =
                    client.prepare(sailry_protocol::Command::ReadWorktreeCatalog { worktree });
                let result = tokio::select! {
                    _ = cancellation.cancelled() => return Err(cancelled()),
                    result = client.execute(request) => result,
                };
                match result {
                    Ok(sailry_protocol::Output::ProjectCatalog(catalog)) => catalog
                        .worktrees
                        .into_iter()
                        .find(|tree| tree.id == worktree)
                        .map(|tree| tree.path),
                    _ => None,
                }
            } else {
                None
            };
            Ok((document, root))
        });
        cx.spawn_in(window, async move |controller, cx| {
            let result = job.await.unwrap_or_else(|_| Err(unavailable()));
            let mut reply = Some(reply);
            let _ = controller.update_in(cx, |controller, window, cx| {
                let reply = reply.take().unwrap();
                if stop.is_cancelled() || !controller.reads.current(&path, sequence) {
                    let _ = reply.send(Err(cancelled()));
                    return;
                }
                let ((content, notebook_page), root) = match result {
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
                        if let Some(page) = notebook_page {
                            document.notebook =
                                Some(super::notebook::Notebook::new(page, window, cx));
                        }
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
                    if let Some(markdown) = &markdown {
                        let owner = cx.weak_entity();
                        let client = controller.binding.client.clone();
                        let (base, context) = (path.clone(), context.clone());
                        let runtime = controller.binding.runtime.clone();
                        let images = cx.new(|cx| {
                            crate::content::images::Images::new(
                                client.clone(),
                                runtime,
                                Arc::new(tokio::sync::Semaphore::new(2)),
                                cx,
                            )
                        });
                        markdown.update(cx, |state, cx| {
                            state.set_link_handler(links::handler(
                                owner,
                                base.clone(),
                                context.clone(),
                            ));
                            state.set_image_chooser(links::chooser(
                                client,
                                base.clone(),
                                root.clone(),
                            ));
                            state.set_image_handler(links::images(images, base, root, context), cx);
                        });
                    }
                    let mut observers = vec![cx.observe(&input, move |controller, _, cx| {
                        controller.document_changed(id, cx)
                    })];
                    if let Some(markdown) = &markdown {
                        observers.push(cx.observe(markdown, move |controller, _, cx| {
                            controller.document_changed(id, cx)
                        }));
                    }
                    controller.documents.insert(
                        id,
                        Document {
                            path,
                            input,
                            markdown,
                            notebook: notebook_page
                                .map(|page| super::notebook::Notebook::new(page, window, cx)),
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
                            wrap: true,
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
