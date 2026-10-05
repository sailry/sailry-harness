//! Explicit checkpoint recovery uses the ordinary save and native Trash kernels.
use super::{io_error, open_regular, path, remove_verified, save, version};
use sailry_protocol::{ErrorCode, Fault, conversation::checkpoint};
use std::path::Path;

pub(crate) fn restore(
    root: &Path,
    content: &checkpoint::Content,
) -> Result<checkpoint::Restored, Fault> {
    let file = &content.file;
    let revision = if let Some(before) = &content.before {
        Some(save(root, &file.path, before, Some(&file.after.revision))?.revision)
    } else {
        // Bound verification before passing its ordinary identity/metadata stamp to Trash.
        let parts = path::entry_components(&file.path)?;
        let (name, parents) = parts.split_last().unwrap();
        let retained = path::root(root)?;
        let parent = path::descend(retained.try_clone().map_err(io_error)?, parents)?;
        let current = open_regular(&parent, name)?;
        if current.metadata().map_err(io_error)?.len() != file.after.size {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "file changed after checkpoint write",
            ));
        }
        let stamp = version::retained(&retained, &parent, &current, &file.path)?;
        remove_verified(root, &file.path, &file.after.revision, &stamp)?;
        None
    };
    Ok(checkpoint::Restored {
        session: content.session,
        checkpoint: file.id,
        path: file.path.clone(),
        revision,
    })
}

#[cfg(test)]
mod tests;
