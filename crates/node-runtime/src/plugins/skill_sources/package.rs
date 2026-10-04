use super::super::repository::LICENSES;
use super::super::{archive, manifest, skills};
use super::*;
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;

pub(super) fn discover(
    file: File,
    selected: &source::Selection,
    stop: &CancellationToken,
) -> Result<Vec<Candidate>, Fault> {
    let (temporary, root, links) = archive::unpack_source(file, stop, 10_000)?;
    reject_ancestors(&links, &selected.path)?;
    let source = crate::files::path::root(&root)?;
    let selected_root = crate::files::path::descend(
        source,
        &crate::files::path::components(&selected.path, true)?,
    )?;
    let mut candidates = Vec::new();
    scan(
        &selected_root,
        &selected.repository,
        &selected.path,
        &mut candidates,
        stop,
        0,
    )?;
    temporary.close().map_err(io_error)?;
    Ok(candidates)
}

fn scan(
    directory: &Dir,
    repository: &str,
    path: &str,
    candidates: &mut Vec<Candidate>,
    stop: &CancellationToken,
    depth: usize,
) -> Result<(), Fault> {
    if stop.is_cancelled() {
        return Err(cancelled());
    }
    if depth > 32 {
        return Err(invalid("skill repository nesting is too deep"));
    }
    if let Some(bytes) = super::super::package::read(directory, "SKILL.md", skills::MAX_BYTES)? {
        let text =
            std::str::from_utf8(&bytes).map_err(|_| invalid("SKILL.md must be UTF-8 text"))?;
        let skill = skills::parse_document(text)?;
        if candidates.len() >= 64 {
            return Err(invalid("skill repository has too many skills"));
        }
        candidates.push(Candidate {
            name: source::name(repository, path),
            path: path.into(),
            skill,
        });
        // References may contain example SKILL.md files, which are not separate installs.
        return Ok(());
    }
    let mut entries = directory
        .entries()
        .map_err(io_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(io_error)?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| invalid("skill filenames must be UTF-8"))?;
        if matches!(name.as_str(), ".git" | "node_modules" | "target" | ".venv") {
            continue;
        }
        if !entry.file_type().map_err(io_error)?.is_dir() {
            continue;
        }
        let child = directory.open_dir_nofollow(&name).map_err(io_error)?;
        let path = if path.is_empty() {
            name
        } else {
            format!("{path}/{name}")
        };
        scan(&child, repository, &path, candidates, stop, depth + 1)?;
    }
    Ok(())
}

pub(super) fn install(
    host: &Host,
    file: File,
    name: &str,
    provenance: Provenance,
    stop: &CancellationToken,
) -> Result<Info, Fault> {
    with_package(file, name, provenance, stop, |root| {
        host.install(root, "", name)
    })
}

pub(super) fn inspect(
    host: &Host,
    file: File,
    name: &str,
    provenance: Provenance,
    stop: &CancellationToken,
) -> Result<Info, Fault> {
    with_package(file, name, provenance, stop, |root| {
        host.inspect(root, "", name, stop.clone())
    })
}

fn with_package(
    file: File,
    name: &str,
    provenance: Provenance,
    stop: &CancellationToken,
    read: impl FnOnce(&std::path::Path) -> Result<Info, Fault>,
) -> Result<Info, Fault> {
    let (repository, root, links) = archive::unpack_source(file, stop, 10_000)?;
    reject_ancestors(&links, &provenance.path)?;
    if links.iter().any(|link| {
        provenance.path.is_empty()
            || link.starts_with(&format!("{}/", provenance.path))
            || LICENSES.contains(&link.as_str())
    }) {
        return Err(invalid(
            "selected skill contains unsupported symbolic links",
        ));
    }
    let root = crate::files::path::root(&root)?;
    let selected = crate::files::path::descend(
        root.try_clone().map_err(io_error)?,
        &crate::files::path::components(&provenance.path, true)?,
    )?;
    let bytes = super::super::package::read(&selected, "SKILL.md", skills::MAX_BYTES)?
        .ok_or_else(|| invalid("selected directory has no SKILL.md"))?;
    let skill = skills::parse_document(
        std::str::from_utf8(&bytes).map_err(|_| invalid("SKILL.md must be UTF-8 text"))?,
    )?;
    let normalized = tempfile::tempdir().map_err(io_error)?;
    let normalized_root = normalized.path().canonicalize().map_err(io_error)?;
    let destination = crate::files::path::root(&normalized_root)?;
    let skill_root = super::super::directory(
        &super::super::directory(&destination, "skills")?,
        &skill.name,
    )?;
    let mut tree = super::super::package::Tree::default();
    tree.walk(&selected, Some(&skill_root), "", 0)?;
    if !tree.issues.is_empty() {
        return Err(invalid("skill contains unsupported files"));
    }
    let manifest = serde_json::json!({
        "$schema": manifest::SCHEMA,
        "name": name,
        "version": provenance.source.commit,
        "description": skill.description,
        "repository": provenance.source.repository,
    });
    destination
        .write(
            "plugin.json",
            serde_json::to_vec(&manifest)
                .map_err(|_| invalid("skill manifest could not be encoded"))?,
        )
        .map_err(io_error)?;
    // A repository license outside the skill directory still travels with the import.
    for license in LICENSES {
        if let Some(bytes) = super::super::package::read(&root, license, 1024 * 1024)? {
            destination
                .write(format!("SOURCE-{license}"), bytes)
                .map_err(io_error)?;
        }
    }
    let mut info = read(&normalized_root)?;
    info.skill = Some(provenance);
    normalized.close().map_err(io_error)?;
    repository.close().map_err(io_error)?;
    Ok(info)
}

fn reject_ancestors(links: &[String], path: &str) -> Result<(), Fault> {
    if links
        .iter()
        .any(|link| path == link || path.starts_with(&format!("{link}/")))
    {
        return Err(invalid("selected skill directory is a symbolic link"));
    }
    Ok(())
}
