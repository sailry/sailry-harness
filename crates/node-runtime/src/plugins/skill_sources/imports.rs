//! Uploaded or scoped local bytes become ordinary immutable standalone packages.
use super::*;
use sailry_protocol::plugin::{
    UploadSource,
    skills::{ArchiveFormat, ImportDiscovery, Imported},
};
use std::{
    io::{Read, Seek},
    path::Path,
};
mod archive;
mod snapshot;
#[cfg(test)]
mod tests;

fn identity(path: &str, skill: &str) -> String {
    let hash = blake3::hash(format!("{path}\0{skill}").as_bytes());
    format!("skill-import-{}", &hash.to_hex()[..24])
}

impl Host {
    pub(crate) fn skill_import_digest(
        file: &mut File,
        stop: &CancellationToken,
    ) -> Result<String, Fault> {
        digest(file, stop)
    }
    pub(crate) fn snapshot_skills(
        root: &Path,
        path: &str,
        file: File,
        stop: &CancellationToken,
    ) -> Result<(File, ArchiveFormat, UploadSource), Fault> {
        snapshot::capture(root, path, file, stop)
    }

    pub(crate) fn discover_skill_import(
        &self,
        file: File,
        stream: sailry_protocol::StreamId,
        digest: String,
        format: ArchiveFormat,
        kind: UploadSource,
        stop: &CancellationToken,
    ) -> Result<ImportDiscovery, Fault> {
        let file = archive::zip(file, format, stop)?;
        let (temporary, root, links) =
            super::super::archive::unpack_source_with(file, stop, 10_000, false)?;
        if !links.is_empty() {
            return Err(Fault::invalid(
                "skill import contains unsupported symbolic links",
            ));
        }
        let root = crate::files::path::root(&root)?;
        let mut skills = Vec::new();
        super::package::scan(&root, "", "", &mut skills, stop, 0)?;
        for candidate in &mut skills {
            candidate.name = identity(&candidate.path, &candidate.skill.name);
        }
        if skills.is_empty() {
            return Err(Fault::invalid("skill import contains no SKILL.md"));
        }
        temporary.close().map_err(io_error)?;
        Ok(ImportDiscovery {
            stream,
            digest,
            kind,
            skills,
        })
    }

    pub(crate) fn install_skill_import(
        &self,
        file: File,
        format: ArchiveFormat,
        source: Imported,
        name: &str,
        stop: &CancellationToken,
    ) -> Result<Info, Fault> {
        let file = archive::zip(file, format, stop)?;
        super::package::with_package(
            file,
            name,
            Provenance::Imported {
                imported: source.clone(),
            },
            stop,
            |root| {
                // The normalizer parses SKILL.md; its declared name and actual path
                // determine the same inventory key that discovery returned.
                let info = self.inspect(root, "", name, stop.clone())?;
                if info.skills.len() != 1 || identity(&source.path, &info.skills[0].name) != name {
                    return Err(Fault::invalid(
                        "skill identity does not match the imported directory",
                    ));
                }
                self.install(root, "", name)
            },
        )
    }
}

fn digest(file: &mut File, stop: &CancellationToken) -> Result<String, Fault> {
    file.rewind().map_err(io_error)?;
    let mut hash = blake3::Hasher::new();
    let mut bytes = [0; 64 * 1024];
    loop {
        if stop.is_cancelled() {
            return Err(cancelled());
        }
        let count = file.read(&mut bytes).map_err(io_error)?;
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
    }
    file.rewind().map_err(io_error)?;
    Ok(hash.finalize().to_hex().to_string())
}
