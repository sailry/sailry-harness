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
        self.reads.invalidate(path);
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
        let path = path.to_owned();
        let client = self.binding.client.clone();
        let worktree = self.binding.worktree.expect("document worktree");
        let stop = Arc::new(self.stop.child_token());
        let sequence = self.reads.start(path.clone(), stop.clone());
        let cancellation = stop.as_ref().clone();
        let task_path = path.clone();
        let job = self.binding.runtime.spawn(async move {
            super::notebook::load(client, worktree, task_path, None, cancellation).await
        });
        cx.spawn_in(window, async move |controller, cx| {
            let result = job.await;
            let _ = controller.update_in(cx, |controller, window, cx| {
                if stop.is_cancelled() || !controller.reads.current(&path, sequence) {
                    return;
                }
                let Some(document) = controller
                    .documents
                    .values_mut()
                    .find(|document| document.path == path && !document.unsaved(cx))
                else {
                    return;
                };
                match result {
                    Ok(Ok((content, notebook_page))) => {
                        document.input.update(cx, |input, cx| {
                            if input.value().as_ref() != content.text {
                                let selection = input.selected_range();
                                let scroll = input.scroll_offset();
                                input.set_value(content.text.clone(), window, cx);
                                input.set_selected_range(selection, cx);
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
                    Ok(Err(fault)) => document.error = Some(fault),
                    Err(_) => document.error = Some(unavailable()),
                }
                controller.changed(cx);
            });
        })
        .detach();
    }
}
