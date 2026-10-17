use super::*;
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use std::io::Write;

pub(super) fn capture(
    root: &Path,
    selected: &str,
    file: File,
    stop: &CancellationToken,
) -> Result<(File, ArchiveFormat, UploadSource), Fault> {
    let mut parts = crate::files::path::entry_components(selected)?;
    let base = crate::files::path::root(root)?;
    let last = parts
        .pop()
        .ok_or_else(|| Fault::invalid("select a skill directory or archive"))?;
    let parent = crate::files::path::descend(base, &parts)?;
    let metadata = parent.symlink_metadata(last).map_err(io_error)?;
    if metadata.is_symlink() {
        return Err(Fault::invalid("skill sources must not be symbolic links"));
    }
    if metadata.is_dir() || last == "SKILL.md" {
        let directory = if metadata.is_dir() {
            parent.open_dir_nofollow(last).map_err(io_error)?
        } else {
            crate::files::open_regular(&parent, last)?;
            parent
        };
        let mut archive = Capture {
            zip: zip::ZipWriter::new(file),
            entries: 0,
            bytes: 0,
            stop,
        };
        archive.append(&directory, "", 0)?;
        let mut file = archive
            .zip
            .finish()
            .map_err(|_| Fault::invalid("skill directory could not be captured"))?;
        file.rewind().map_err(io_error)?;
        Ok((file, ArchiveFormat::Zip, UploadSource::Directory))
    } else {
        let format = format(last)?;
        let mut original = crate::files::open_regular(&parent, last)?;
        let before = original.metadata().map_err(io_error)?;
        if before.len() == 0 || before.len() > sailry_protocol::plugin::MAX_PACKAGE_BYTES {
            return Err(Fault::invalid("skill archive exceeds its size limit"));
        }
        let mut file = file;
        let copied = copy(&mut original, &mut file, before.len(), stop)?;
        let after = original.metadata().map_err(io_error)?;
        if copied != before.len()
            || after.len() != before.len()
            || before.modified().ok() != after.modified().ok()
        {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "skill archive changed while being captured",
            ));
        }
        file.rewind().map_err(io_error)?;
        Ok((file, format, UploadSource::Archive))
    }
}

fn format(name: &str) -> Result<ArchiveFormat, Fault> {
    let name = name.to_ascii_lowercase();
    if name.ends_with(".zip") || name.ends_with(".skill") {
        Ok(ArchiveFormat::Zip)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Ok(ArchiveFormat::TarGz)
    } else if name.ends_with(".tar") {
        Ok(ArchiveFormat::Tar)
    } else {
        Err(Fault::invalid(
            "select a ZIP, tar, tar.gz or tgz skill archive",
        ))
    }
}

struct Capture<'a> {
    zip: zip::ZipWriter<File>,
    entries: usize,
    bytes: u64,
    stop: &'a CancellationToken,
}
impl Capture<'_> {
    fn append(&mut self, source: &Dir, prefix: &str, depth: usize) -> Result<(), Fault> {
        if depth > 32 {
            return Err(Fault::invalid("skill directory nesting is too deep"));
        }
        let mut names = source
            .entries()
            .map_err(io_error)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(io_error)?;
        names.sort();
        for name in names {
            if self.stop.is_cancelled() {
                return Err(cancelled());
            }
            let name = name
                .to_str()
                .ok_or_else(|| Fault::invalid("skill filenames must be UTF-8"))?;
            let path = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            // These entries are outside the existing Agent file scope. Do not
            // turn a directory snapshot into a read of Git or managed checkouts.
            if let Err(error) = crate::files::path::entry_components(&path) {
                if error.code == ErrorCode::PermissionDenied {
                    continue;
                }
                return Err(error);
            }
            self.entries += 1;
            if self.entries > 10_000 {
                return Err(Fault::invalid("skill directory has too many entries"));
            }
            let metadata = source.symlink_metadata(name).map_err(io_error)?;
            if metadata.is_dir() {
                self.zip
                    .add_directory(format!("{path}/"), zip::write::SimpleFileOptions::default())
                    .map_err(|_| Fault::invalid("skill directory could not be captured"))?;
                self.append(
                    &source.open_dir_nofollow(name).map_err(io_error)?,
                    &path,
                    depth + 1,
                )?;
            } else if metadata.is_file() && !metadata.is_symlink() {
                let mut file = crate::files::open_regular(source, name)?;
                let before = file.metadata().map_err(io_error)?;
                self.bytes = self
                    .bytes
                    .checked_add(before.len())
                    .filter(|size| *size <= sailry_protocol::plugin::MAX_PACKAGE_BYTES)
                    .ok_or_else(|| Fault::invalid("skill directory exceeds its size limit"))?;
                let options = zip::write::SimpleFileOptions::default();
                #[cfg(unix)]
                let options = {
                    use cap_std::fs::PermissionsExt;
                    options.unix_permissions(before.permissions().mode() & 0o777)
                };
                self.zip
                    .start_file(path, options)
                    .map_err(|_| Fault::invalid("skill directory could not be captured"))?;
                let copied = copy(&mut file, &mut self.zip, before.len(), self.stop)?;
                let after = file.metadata().map_err(io_error)?;
                if copied != before.len()
                    || before.len() != after.len()
                    || before.modified().ok() != after.modified().ok()
                {
                    return Err(Fault::new(
                        ErrorCode::RevisionConflict,
                        "skill directory changed while being captured",
                    ));
                }
            } else {
                return Err(Fault::invalid(
                    "skill directory contains an unsupported link or special file",
                ));
            }
        }
        Ok(())
    }
}

fn copy(
    reader: &mut impl Read,
    writer: &mut impl Write,
    limit: u64,
    stop: &CancellationToken,
) -> Result<u64, Fault> {
    let mut bytes = [0; 64 * 1024];
    let mut copied = 0u64;
    loop {
        if stop.is_cancelled() {
            return Err(cancelled());
        }
        let count = reader.read(&mut bytes).map_err(io_error)?;
        if count == 0 {
            return Ok(copied);
        }
        copied += count as u64;
        if copied > limit {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "skill source changed while being captured",
            ));
        }
        writer.write_all(&bytes[..count]).map_err(io_error)?;
    }
}
