use super::*;
use sailry_protocol::{Event, Update};
use std::time::Duration;

impl Panel {
    pub(super) fn watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.stop.is_some() {
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        let stop = CancellationToken::new();
        self.stop = Some(stop.clone());
        let binding = self.binding.clone();
        let node = binding.client.target();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        self.binding.runtime.spawn(async move {
            loop {
                let stream = tokio::select! {
                    _ = stop.cancelled() => return,
                    stream = binding.client.subscribe() => stream,
                };
                if let Ok(mut stream) = stream {
                    loop {
                        let update = tokio::select! {
                            _ = stop.cancelled() => return,
                            update = stream.next() => update,
                        };
                        let update = match update {
                            Ok(Update::Event(envelope)) if envelope.node == node => {
                                match envelope.event {
                                    Event::KnowledgeModelsChanged { models } => Some(Some(models)),
                                    Event::KnowledgeSettingsChanged { .. } => Some(None),
                                    _ => None,
                                }
                            }
                            Ok(Update::Snapshot(snapshot)) if snapshot.node == node => Some(None),
                            Ok(Update::ResetRequired) => Some(None),
                            Ok(_) => None,
                            Err(_) => break,
                        };
                        if let Some(update) = update {
                            tokio::select! {
                                _ = stop.cancelled() => return,
                                result = sender.send(update) => if result.is_err() { return; },
                            }
                        }
                    }
                }
                tokio::select! {
                    _ = stop.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {},
                }
            }
        });
        cx.spawn_in(window, async move |owner, cx| {
            while let Some(update) = receiver.recv().await {
                if owner
                    .update_in(cx, |panel, window, cx| {
                        if !panel.active || panel.generation != generation {
                            return;
                        }
                        if let Some(models) = update {
                            if panel.loading || panel.locked() {
                                panel.models = models;
                                panel.refresh = true;
                            } else {
                                let selected = if panel.dirty(cx) {
                                    panel.model.read(cx).selected_value().copied().flatten()
                                } else {
                                    panel.saved.as_ref().and_then(|settings| settings.model)
                                };
                                let chosen = panel.update_choices(models, selected, window, cx);
                                panel.fallback(selected, chosen, window, cx);
                            }
                        } else {
                            panel.load(window, cx);
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();
    }
}
