//! A view may consume only files explicitly dropped into its own native tree.
//! JavaScript receives a one-use handle; controller-local paths stay in Rust.
use super::*;
use std::path::PathBuf;

#[derive(Clone)]
pub(in crate::plugins) struct Drops {
    paths: Rc<RefCell<BTreeMap<String, Vec<PathBuf>>>>,
    stop: CancellationToken,
}
impl Drops {
    pub fn new(stop: CancellationToken) -> Self {
        Self {
            paths: Default::default(),
            stop,
        }
    }
    pub fn take(&self, id: &str) -> Result<Vec<PathBuf>, HostError> {
        if self.stop.is_cancelled() {
            return Err(HostError::new("plugin view is closed"));
        }
        self.paths
            .borrow_mut()
            .remove(id)
            .ok_or_else(|| HostError::new("dropped files are unavailable"))
    }
    pub(super) fn publish(&self, paths: &ExternalPaths, events: &Events, tree: &str, target: &str) {
        if self.stop.is_cancelled() || paths.0.is_empty() {
            return;
        }
        let id = sailry_protocol::RequestId::new().to_string();
        self.paths.borrow_mut().insert(id.clone(), paths.0.to_vec());
        if events
            .sender
            .try_send(json!({"tree":tree,"id":target,"kind":"external_drop","files":id}))
            .is_err()
        {
            self.paths.borrow_mut().remove(&id);
        }
    }
}
