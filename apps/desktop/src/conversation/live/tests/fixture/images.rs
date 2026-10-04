//! Integration access for the shared image component without exposing production view state.
use super::*;
use crate::content::images::Images;
use sailry_protocol::attachment::Attachment;
use std::path::{Path, PathBuf};

pub(crate) trait ImageView {
    fn set_reply(
        &mut self,
        parts: Vec<sailry_protocol::conversation::Part>,
        cx: &mut Context<Self>,
    ) where
        Self: Sized;
    fn image_cache(&self) -> Entity<Images>;
    fn image_history(&self) -> &History;
    fn image_history_mut(&mut self) -> &mut History;
    fn image_count(&self) -> usize;
    fn image_ready(&self) -> bool;
    fn image_staged(&self) -> bool;
    fn image_attachments(&self) -> Vec<Attachment>;
    fn add_images(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>)
    where
        Self: Sized;
}

impl ImageView for View {
    fn set_reply(
        &mut self,
        parts: Vec<sailry_protocol::conversation::Part>,
        cx: &mut Context<Self>,
    ) {
        use sailry_protocol::conversation::Part;
        let page = Arc::make_mut(&mut Arc::make_mut(self.history.snapshot.as_mut().unwrap()).page);
        let entry = page
            .entries
            .iter_mut()
            .rev()
            .find(|entry| {
                entry.author != "user"
                    && entry.parts.iter().any(|part| matches!(part, Part::Text(_)))
            })
            .unwrap();
        entry.parts = parts;
        self.scroller.update(cx, |scroller, cx| {
            scroller.remeasure_items(0..self.rows.len(), cx)
        });
        cx.notify();
    }
    fn image_cache(&self) -> Entity<Images> {
        self.images.clone()
    }
    fn image_history(&self) -> &History {
        &self.history
    }
    fn image_history_mut(&mut self) -> &mut History {
        &mut self.history
    }
    fn image_count(&self) -> usize {
        self.attachments.items.len()
    }
    fn image_ready(&self) -> bool {
        self.attachments.ready()
    }
    fn image_staged(&self) -> bool {
        self.session.is_none()
            && matches!(
                self.attachments.items.first().map(|item| &item.status),
                Some(transfer::Status::Staged)
            )
    }
    fn image_attachments(&self) -> Vec<Attachment> {
        self.attachments
            .items
            .iter()
            .filter_map(|item| match &item.status {
                transfer::Status::Ready(attachment) => Some(attachment.clone()),
                _ => None,
            })
            .collect()
    }
    fn add_images(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        self.attach_paths(paths, window, cx);
    }
}

pub(crate) fn finish_download(visual: &mut VisualTestContext, destination: &Path) {
    tests::finish_download(visual, destination);
}
