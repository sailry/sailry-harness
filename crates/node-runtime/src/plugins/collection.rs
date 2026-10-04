//! Remove unreferenced immutable packages while the Node has no active consumers.
use super::*;
use cap_fs_ext::DirExt;
use std::collections::BTreeSet;

impl Host {
    pub(crate) fn collect(&self, retained: &BTreeSet<String>) -> Result<(), Fault> {
        let Some(profile) = &self.profile else {
            return Ok(());
        };
        let profile = crate::files::path::root(profile)?;
        let packages = match profile
            .open_dir_nofollow("plugins")
            .and_then(|plugins| plugins.open_dir_nofollow("packages"))
        {
            Ok(packages) => packages,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(io_error(error)),
        };
        for entry in packages.entries().map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            // Only canonical content-addressed directories belong to this collector.
            if retained.contains(name)
                || !blake3::Hash::from_hex(name).is_ok_and(|hash| hash.to_hex().as_str() == name)
                || !entry.file_type().map_err(io_error)?.is_dir()
            {
                continue;
            }
            packages.remove_dir_all(name).map_err(io_error)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
