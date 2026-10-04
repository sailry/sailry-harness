use super::*;
use crate::files::save::content;
use cap_std::fs::OpenOptions;
use evidence::{Contents, Evidence};
use std::{collections::BTreeMap, io::Seek};

const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 64;

pub(super) struct Copier {
    pub count: usize,
    pub moving: bool,
}

impl Copier {
    pub fn copy(
        &mut self,
        source: &Dir,
        name: &str,
        target: &Dir,
        destination: &str,
        depth: usize,
    ) -> Result<Option<Evidence>, Fault> {
        self.count += 1;
        if self.count > MAX_ENTRIES || depth > MAX_DEPTH {
            return Err(invalid("copy exceeds the entry or nesting limit"));
        }
        let before = object(source, name)?;
        let contents = if before.directory {
            let source = source.open_dir_nofollow(name).map_err(io_error)?;
            if identity::directory(&source)? != before.id {
                return Err(changed());
            }
            target.create_dir(destination).map_err(io_error)?;
            let target = target.open_dir_nofollow(destination).map_err(io_error)?;
            let mut children = BTreeMap::new();
            for child in source.entries().map_err(io_error)? {
                let name = child
                    .map_err(io_error)?
                    .file_name()
                    .into_string()
                    .map_err(|_| {
                        invalid("copy contains a name not representable in the file protocol")
                    })?;
                if !self.moving && name.eq_ignore_ascii_case(".git") {
                    continue;
                }
                // Whole-tree moves must preserve metadata that copies skip.
                // Direct file requests to Git metadata remain forbidden.
                path::components(&name, false)?;
                if let Some(evidence) = self.copy(&source, &name, &target, &name, depth + 1)?
                    && children.insert(name, evidence).is_some()
                {
                    return Err(changed());
                }
            }
            if self.moving {
                target
                    .set_times(".", None, before.modified.map(Into::into))
                    .map_err(io_error)?;
            }
            target
                .set_permissions(".", before.permissions.clone())
                .map_err(io_error)?;
            #[cfg(unix)]
            target.into_std_file().sync_all().map_err(io_error)?;
            Contents::Directory(children)
        } else {
            let mut file = open_regular(source, name)?;
            if identity::file(&file)? != before.id {
                return Err(changed());
            }
            let revision = content::revision(&mut file, u64::MAX, None)?;
            file.rewind().map_err(io_error)?;
            let mut output = target
                .open_with(destination, OpenOptions::new().write(true).create_new(true))
                .map_err(io_error)?;
            content::copy(&mut file, &mut output, before.size, &revision)?;
            if self.moving {
                target
                    .set_symlink_times(destination, None, before.modified.map(Into::into))
                    .map_err(io_error)?;
            }
            output
                .set_permissions(before.permissions.clone())
                .map_err(io_error)?;
            output.sync_all().map_err(io_error)?;
            Contents::File(revision)
        };
        if object(source, name)? != before {
            return Err(changed());
        }
        if self.moving {
            Ok(Some(Evidence {
                source: before,
                target: object(target, destination)?,
                contents,
            }))
        } else {
            Ok(None)
        }
    }
}
