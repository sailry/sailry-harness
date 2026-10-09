use super::*;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::io::Read;

const LIMIT: usize = 1024 * 1024;

pub(super) fn protected(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str().to_str().is_some_and(excluded))
}

pub(super) fn excluded(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        ".git"
            | ".ssh"
            | ".gnupg"
            | ".aws"
            | ".azure"
            | ".kube"
            | ".docker"
            | ".config"
            | ".sailry"
            | ".codex"
            | ".claude"
            | ".gemini"
            | ".npmrc"
            | ".pypirc"
            | ".netrc"
            | ".gitconfig"
            | ".git-credentials"
            | ".ds_store"
            | "credentials"
            | "credentials.json"
            | "credentials.toml"
    ) || name == ".env"
        || name.starts_with(".env.")
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || [".pem", ".key", ".p12", ".pfx"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
}

pub(super) fn load(
    directory: &Dir,
    relative: &str,
    cancel: &CancellationToken,
) -> Result<Option<Gitignore>, Fault> {
    cancelled(cancel)?;
    let metadata = match directory.symlink_metadata(".gitignore") {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(files::io_error(error)),
    };
    if !metadata.is_file() || metadata.is_symlink() || metadata.len() > LIMIT as u64 {
        return Err(Fault::invalid(
            ".gitignore must be a regular file within 1 MiB",
        ));
    }
    let mut bytes = Vec::new();
    files::open_regular(directory, ".gitignore")?
        .take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(files::io_error)?;
    cancelled(cancel)?;
    if bytes.len() > LIMIT {
        return Err(Fault::invalid(".gitignore exceeds 1 MiB"));
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| Fault::invalid(".gitignore is not UTF-8 text"))?;
    let mut builder = GitignoreBuilder::new(if relative.is_empty() { "." } else { relative });
    // Parse retained text only; ambient ignore discovery could follow external links.
    for line in text.lines() {
        cancelled(cancel)?;
        builder
            .add_line(None, line)
            .map_err(|_| Fault::invalid("invalid .gitignore pattern"))?;
    }
    builder
        .build()
        .map(Some)
        .map_err(|_| Fault::invalid("invalid .gitignore patterns"))
}

pub(super) fn excludes<'a>(
    layers: impl Iterator<Item = &'a Gitignore>,
    relative: &str,
    directory: bool,
) -> bool {
    for layer in layers {
        let matched = layer.matched(relative, directory);
        if !matched.is_none() {
            return matched.is_ignore();
        }
    }
    false
}
