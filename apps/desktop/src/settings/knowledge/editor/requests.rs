use super::*;
use sailry_protocol::{Command, ErrorCode, Output};

impl Editor {
    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed {
            return;
        }
        if !self.owner.read(cx).active || self.owner.read(cx).pending {
            self.error = Some("knowledge_target_changed");
            cx.notify();
            return;
        }
        let binding = self.owner.read(cx).binding.clone();
        if self.request.is_none() {
            let definition = match self.definition(cx) {
                Ok(definition) => definition,
                Err(key) => {
                    self.error = Some(key);
                    cx.notify();
                    return;
                }
            };
            let command = match &self.collection {
                Some(collection) => Command::UpdateKnowledgeCollection {
                    id: collection.id,
                    expected_revision: collection.revision,
                    definition,
                },
                None => Command::CreateKnowledgeCollection {
                    definition,
                    plugin: None,
                },
            };
            self.request = Some(binding.client.prepare(command));
        }
        let request = self.request.clone().expect("prepared collection request");
        self.pending = true;
        self.error = None;
        let runtime = binding.runtime.clone();
        let job =
            runtime.spawn(async move { super::super::requests::execute(&binding, request).await });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |editor, window, cx| {
                editor.pending = false;
                match result {
                    Ok((Ok(Output::KnowledgeCollection(collection)), _)) => {
                        editor.request = None;
                        editor.owner.update(cx, |panel, cx| {
                            panel
                                .collections
                                .retain(|current| current.id != collection.id);
                            panel.collections.push(collection);
                            panel.load(window, cx);
                        });
                        crate::feedback::info("", &tr("knowledge_saved"), window, cx);
                        editor.closed = true;
                        window.close_dialog(cx);
                    }
                    Ok((Err(error), confirmed)) => {
                        let uncertain = !confirmed || error.code == ErrorCode::OutcomeUnknown;
                        if !uncertain {
                            editor.request = None;
                        }
                        editor.error = Some(if uncertain {
                            "knowledge_unknown"
                        } else {
                            super::super::requests::error_key(error.code)
                        });
                    }
                    _ => editor.error = Some("knowledge_unknown"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.request.is_some() || self.closed || !self.owner.read(cx).active {
            return;
        }
        let Some(collection) = &self.collection else {
            self.error = None;
            cx.notify();
            return;
        };
        let id = collection.id;
        let binding = self.owner.read(cx).binding.clone();
        self.pending = true;
        self.error = None;
        let job = binding.runtime.spawn(async move {
            binding
                .client
                .execute(binding.client.prepare(Command::ListKnowledgeCollections))
                .await
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |editor, _, cx| {
                editor.pending = false;
                match result {
                    Ok(Ok(Output::KnowledgeCollections(collections))) => {
                        if let Some(current) =
                            collections.into_iter().find(|current| current.id == id)
                        {
                            editor.collection = Some(current);
                        } else {
                            editor.error = Some("knowledge_collection_missing");
                        }
                    }
                    _ => editor.error = Some("knowledge_failed"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
