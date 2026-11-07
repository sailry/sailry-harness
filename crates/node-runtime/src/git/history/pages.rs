use super::*;
use crate::files::path;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub(super) struct Capture {
    root: PathBuf,
    identity: path::identity::Identity,
    head: Oid,
    path: Option<String>,
    pub(super) commits: Vec<Oid>,
}

#[derive(Default)]
pub(in crate::git) struct Pages(Mutex<VecDeque<Arc<Capture>>>);

impl Pages {
    pub(super) fn get(
        &self,
        root: &Path,
        head: Oid,
        path: Option<&str>,
    ) -> Result<Option<Arc<Capture>>, Fault> {
        let identity = path::identity::directory(&path::root(root)?)?;
        let mut captures = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let Some(index) = captures.iter().position(|capture| {
            capture.root == root && capture.head == head && capture.path.as_deref() == path
        }) else {
            return Ok(None);
        };
        let capture = captures.remove(index).unwrap();
        if capture.identity != identity {
            return Ok(None);
        }
        captures.push_back(capture.clone());
        Ok(Some(capture))
    }

    pub(super) fn insert(
        &self,
        root: &Path,
        head: Oid,
        path: Option<&str>,
        commits: Vec<Oid>,
    ) -> Result<Arc<Capture>, Fault> {
        let capture = Arc::new(Capture {
            root: root.to_owned(),
            identity: path::identity::directory(&path::root(root)?)?,
            head,
            path: path.map(str::to_owned),
            commits,
        });
        let mut captures = self.0.lock().unwrap_or_else(|error| error.into_inner());
        captures.retain(|capture| {
            capture.root != root || capture.head != head || capture.path.as_deref() != path
        });
        if captures.len() == 8 {
            captures.pop_front();
        }
        captures.push_back(capture.clone());
        Ok(capture)
    }
}
