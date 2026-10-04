//! Bounded, transient evidence for copy-before-Trash. This is neither a durable
//! transfer ledger nor an atomic directory snapshot.
use super::*;
use crate::files::save::content;
use std::collections::BTreeMap;

pub(super) struct Evidence {
    pub source: Object,
    pub target: Object,
    pub contents: Contents,
}

pub(super) enum Contents {
    File(String),
    Directory(BTreeMap<String, Evidence>),
}

#[derive(Clone, Copy)]
pub(super) enum Side {
    Source,
    Target,
}

impl Evidence {
    pub fn verify(&self, parent: &Dir, name: &str, side: Side) -> Result<(), Fault> {
        let expected = match side {
            Side::Source => &self.source,
            Side::Target => &self.target,
        };
        if &object(parent, name)? != expected {
            return Err(changed());
        }
        match &self.contents {
            Contents::File(revision) => {
                let mut file = open_regular(parent, name)?;
                if identity::file(&file)? != expected.id
                    || content::revision(&mut file, expected.size, None)? != *revision
                {
                    return Err(changed());
                }
            }
            Contents::Directory(children) => {
                let dir = parent.open_dir_nofollow(name).map_err(io_error)?;
                if identity::directory(&dir)? != expected.id {
                    return Err(changed());
                }
                // Never allocate a new listing for a potentially growing tree.
                let mut count = 0;
                for child in dir.entries().map_err(io_error)? {
                    count += 1;
                    let name = child
                        .map_err(io_error)?
                        .file_name()
                        .into_string()
                        .map_err(|_| changed())?;
                    if count > children.len() || !children.contains_key(&name) {
                        return Err(changed());
                    }
                }
                if count != children.len() {
                    return Err(changed());
                }
                for (name, evidence) in children {
                    evidence.verify(&dir, name, side)?;
                }
            }
        }
        if &object(parent, name)? != expected {
            return Err(changed());
        }
        Ok(())
    }
}
