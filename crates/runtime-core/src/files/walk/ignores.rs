use super::*;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

const MAX_IGNORE_BYTES: usize = 1024 * 1024;

pub fn load(
    directory: &Dir,
    prefix: &str,
    control: &Control,
    report: &mut Report,
) -> Option<Gitignore> {
    load_path(directory, Path::new(prefix), control, report)
}

pub fn load_path(
    directory: &Dir,
    prefix: &Path,
    control: &Control,
    report: &mut Report,
) -> Option<Gitignore> {
    load_file(directory, prefix, ".gitignore", control, report)
}

pub(super) fn override_rules(
    directory: &Dir,
    prefix: &str,
    control: &Control,
    report: &mut Report,
) -> Option<Gitignore> {
    load_file(directory, Path::new(prefix), ".ignore", control, report)
}

pub(super) fn base(
    directory: &Dir,
    root: &Path,
    control: &Control,
    report: &mut Report,
) -> Vec<Gitignore> {
    let mut layers = Vec::new();
    let repository = match repository(directory, root, control) {
        Ok(repository) => repository,
        Err(error) => {
            report.notice(".git", &error.message);
            None
        }
    };
    let Some(repository) = repository else {
        return layers;
    };
    let configured = repository
        .config()
        .and_then(|config| config.get_path("core.excludesFile"));
    match configured {
        Ok(configured) if configured.as_os_str().is_empty() => {}
        Ok(configured) => {
            let configured = root.join(configured);
            let loaded: Result<Option<Gitignore>, Fault> = (|| {
                let name = configured
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| Fault::invalid("invalid core.excludesFile path"))?;
                let parent = match path::root(configured.parent().unwrap()) {
                    Err(error) if error.code == ErrorCode::NotFound => return Ok(None),
                    parent => parent?,
                };
                Ok(load_file(&parent, Path::new(""), name, control, report))
            })();
            match loaded {
                Ok(Some(rules)) => layers.push(rules),
                Ok(None) => {}
                Err(error) => report.notice("", &error.message),
            }
        }
        Err(error) if error.code() != git2::ErrorCode::NotFound => {
            report.notice(
                "",
                &format!("could not load Git ignore configuration: {error}"),
            );
        }
        _ => {
            let (global, error) = GitignoreBuilder::new(".").build_global();
            if let Some(error) = error {
                report.notice("", &format!("could not load global ignore rules: {error}"));
            }
            layers.push(global);
        }
    }
    if let Ok(directory) = path::root(repository.commondir())
        && let Ok(info) = path::descend(directory, &["info"])
        && let Some(exclude) = load_file(&info, Path::new(""), "exclude", control, report)
    {
        layers.push(exclude);
    }
    layers
}

fn repository(
    directory: &Dir,
    root: &Path,
    control: &Control,
) -> Result<Option<git2::Repository>, Fault> {
    control.check()?;
    let metadata = match directory.symlink_metadata(".git") {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
    };
    if metadata.is_symlink() {
        return Err(Fault::invalid("Git metadata cannot be a symbolic link"));
    }
    git2::Repository::open_ext(
        root,
        git2::RepositoryOpenFlags::NO_SEARCH,
        std::iter::empty::<&Path>(),
    )
    .map(Some)
    .map_err(|error| Fault::invalid(format!("could not open Git ignore configuration: {error}")))
}

fn load_file(
    directory: &Dir,
    prefix: &Path,
    name: &str,
    control: &Control,
    report: &mut Report,
) -> Option<Gitignore> {
    let label = prefix.to_string_lossy();
    let result = (|| {
        let Some(text) = read(directory, name, control)? else {
            return Ok(None);
        };
        let mut builder = GitignoreBuilder::new(if prefix.as_os_str().is_empty() {
            Path::new(".")
        } else {
            prefix
        });
        for line in text.lines() {
            control.check()?;
            builder
                .add_line(None, line)
                .map_err(|_| Fault::invalid(format!("invalid {name} pattern")))?;
        }
        builder
            .build()
            .map(Some)
            .map_err(|_| Fault::invalid(format!("invalid {name} patterns")))
    })();
    match result {
        Ok(matcher) => matcher,
        Err(error) => {
            report.notice(&label, &error.message);
            None
        }
    }
}

fn read(directory: &Dir, name: &str, control: &Control) -> Result<Option<String>, Fault> {
    let metadata = match directory.symlink_metadata(name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
    };
    if metadata.is_symlink() || !metadata.is_file() {
        return Err(Fault::invalid(format!(
            "{name} is not a regular file; its rules were not loaded"
        )));
    }
    if metadata.len() > MAX_IGNORE_BYTES as u64 {
        return Err(Fault::invalid(format!("{name} exceeds the 1 MiB limit")));
    }
    let mut file = super::super::open_regular(directory, name)?;
    let bytes = super::super::read_bytes(&mut file, MAX_IGNORE_BYTES + 1, control)?;
    if bytes.len() > MAX_IGNORE_BYTES {
        return Err(Fault::invalid(format!("{name} exceeds the 1 MiB limit")));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| Fault::invalid(format!("{name} is not UTF-8 text")))
}

pub(super) fn excludes<'a>(
    layers: impl Iterator<Item = &'a Gitignore>,
    relative: &str,
    directory: bool,
) -> bool {
    for layer in layers {
        let found = layer.matched(relative, directory);
        if !found.is_none() {
            return found.is_ignore();
        }
    }
    false
}
