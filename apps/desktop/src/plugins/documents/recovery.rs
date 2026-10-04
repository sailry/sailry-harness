//! A confirmed core checkpoint refreshes clean buffers without invoking a plugin.
use super::*;

impl Controller {
    pub(crate) fn restored(
        &mut self,
        path: &str,
        removed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reads.remove(path);
        if removed {
            self.documents
                .retain(|_, document| document.path != path || document.unsaved(cx));
            self.changed(cx);
            return;
        }
        if !self
            .documents
            .values()
            .any(|document| document.path == path && !document.unsaved(cx))
        {
            return;
        }
        self.read_sequence += 1;
        let sequence = self.read_sequence;
        let path = path.to_owned();
        self.reads.insert(path.clone(), sequence);
        let client = self.binding.client.clone();
        let worktree = self.binding.worktree.expect("document worktree");
        let stop = self.stop.child_token();
        let task_path = path.clone();
        let job = self
            .binding
            .runtime
            .spawn(async move { client.read_document(worktree, &task_path, stop).await });
        cx.spawn_in(window, async move |controller, cx| {
            let result = job.await;
            let _ = controller.update_in(cx, |controller, window, cx| {
                if controller.reads.get(&path) != Some(&sequence) {
                    return;
                }
                controller.reads.remove(&path);
                let Some(document) = controller
                    .documents
                    .values_mut()
                    .find(|document| document.path == path && !document.unsaved(cx))
                else {
                    return;
                };
                match result {
                    Ok(Ok(content)) => {
                        document.input.update(cx, |input, cx| {
                            if input.value().as_ref() != content.text {
                                let selection = input.selected_range();
                                let scroll = input.scroll_offset();
                                input.set_value(content.text.clone(), window, cx);
                                input.set_selected_range(selection, cx);
                                input.set_scroll_offset(scroll, cx);
                            }
                        });
                        document.baseline = content.text;
                        document.revision = content.revision;
                        document.truncated = content.truncated;
                        document.error = None;
                    }
                    Ok(Err(fault)) => document.error = Some(fault),
                    Err(_) => document.error = Some(unavailable()),
                }
                controller.changed(cx);
            });
        })
        .detach();
    }

    #[cfg(test)]
    pub(crate) fn editor(
        &self,
        path: &str,
    ) -> Option<Entity<gpui_kit::component::input::EditorState>> {
        self.documents
            .values()
            .find(|document| document.path == path && !document.dismissed)
            .map(|document| document.input.clone())
    }
}
