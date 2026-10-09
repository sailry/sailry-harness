use super::*;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

const MAX_IGNORE_BYTES: usize = 1024 * 1024;

pub(super) fn load(
    directory: &Dir,
    prefix: &str,
    control: &Control,
    report: &mut Report,
) -> Option<Gitignore> {
    let metadata = match directory.symlink_metadata(".gitignore") {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => {
            report.diagnostic(prefix, "could not inspect .gitignore");
            return None;
        }
    };
    if metadata.is_symlink() || !metadata.is_file() {
        report.diagnostic(
            prefix,
            ".gitignore is not a regular file; its rules were not loaded",
        );
        return None;
    }
    let result = (|| {
        if metadata.len() > MAX_IGNORE_BYTES as u64 {
            return Err(Fault::invalid(".gitignore exceeds the 1 MiB limit"));
        }
        let mut file = super::super::open_regular(directory, ".gitignore")?;
        let bytes = super::super::read_bytes(&mut file, MAX_IGNORE_BYTES + 1, control)?;
        if bytes.len() > MAX_IGNORE_BYTES {
            return Err(Fault::invalid(".gitignore exceeds the 1 MiB limit"));
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| Fault::invalid(".gitignore is not UTF-8 text"))?;
        // Only already-confined text is parsed. add(path) and global ignore discovery
        // would read ambient paths, including symlinked files outside the worktree.
        let mut builder = GitignoreBuilder::new(if prefix.is_empty() { "." } else { prefix });
        for line in text.lines() {
            control.check()?;
            builder
                .add_line(None, line)
                .map_err(|_| Fault::invalid("invalid .gitignore pattern"))?;
        }
        builder
            .build()
            .map_err(|_| Fault::invalid("invalid .gitignore patterns"))
    })();
    match result {
        Ok(matcher) => Some(matcher),
        Err(error) => {
            report.diagnostic(prefix, &error.message);
            None
        }
    }
}

pub(super) fn excludes(layers: &[Gitignore], relative: &str, directory: bool) -> bool {
    for layer in layers.iter().rev() {
        let found = layer.matched(relative, directory);
        if !found.is_none() {
            return found.is_ignore();
        }
    }
    false
}
