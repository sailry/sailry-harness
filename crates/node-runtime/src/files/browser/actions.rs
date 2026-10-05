use super::*;
use crate::files;

pub(crate) fn targets(action: &Action) -> Result<Vec<(PathBuf, String)>, Fault> {
    let paths: Vec<PathBuf> = match action {
        Action::CreateDirectory { path } => vec![path.into()],
        Action::Rename { from, to } => vec![from.into(), to.into()],
        Action::Transfer {
            paths, destination, ..
        } => {
            if paths.is_empty() || paths.len() > 100 {
                return Err(invalid("select between one and one hundred entries"));
            }
            let destination = resolve(Path::new(destination))?;
            let mut targets = Vec::new();
            for path in paths {
                let (_, name) = split(Path::new(path))?;
                targets.push(PathBuf::from(path));
                targets.push(destination.join(name));
            }
            targets
        }
        Action::Trash { paths } => {
            if paths.is_empty() || paths.len() > 100 {
                return Err(invalid("select between one and one hundred entries"));
            }
            paths.iter().map(PathBuf::from).collect()
        }
    };
    let home = dirs::home_dir().and_then(|home| home.canonicalize().ok());
    let mut entries = Vec::new();
    for path in paths {
        let (parent, name) = split(&path)?;
        let target = parent.join(&name);
        if home.as_ref().is_some_and(|home| home.starts_with(&target)) {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "a home directory or its ancestors cannot be modified",
            ));
        }
        entries.push((parent, name));
    }
    Ok(entries)
}

pub(crate) fn execute(action: &Action) -> Result<Vec<Outcome>, Fault> {
    let paths: Vec<&str> = match action {
        Action::CreateDirectory { path } => vec![path],
        Action::Rename { from, .. } => vec![from],
        Action::Transfer { paths, .. } | Action::Trash { paths } => {
            paths.iter().map(String::as_str).collect()
        }
    };
    let mut results = Vec::new();
    for path in paths {
        let result = apply(action, path);
        let failed = result.is_err();
        let (destination, error) = match result {
            Ok(destination) => (destination, None),
            Err(error) => (None, Some(error)),
        };
        results.push(Outcome {
            path: path.into(),
            destination,
            error,
        });
        if failed {
            break;
        }
    }
    Ok(results)
}

fn apply(action: &Action, path: &str) -> Result<Option<String>, Fault> {
    let (source, name) = split(Path::new(path))?;
    match action {
        Action::CreateDirectory { .. } => {
            files::directory(&source, &name)?;
            Ok(Some(text(&source.join(name))?))
        }
        Action::Rename { to, .. } => {
            let (target, renamed) = split(Path::new(to))?;
            files::rename::between(&source, &target, &name, &renamed)?;
            Ok(Some(text(&target.join(renamed))?))
        }
        Action::Trash { .. } => {
            files::trash(&source, &name)?;
            Ok(None)
        }
        Action::Transfer {
            destination,
            cut,
            conflict,
            ..
        } => {
            let target = resolve(Path::new(destination))?;
            let original = source.join(&name);
            let metadata = original.symlink_metadata().map_err(io_error)?;
            if metadata.is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
                return Err(invalid(
                    "only regular files and directories can be transferred",
                ));
            }
            if metadata.is_dir() && target.starts_with(&original) {
                return Err(invalid("a directory cannot be transferred into itself"));
            }
            let mut renamed = name.clone();
            let exists = target.join(&renamed).try_exists().map_err(io_error)?;
            let mut replaced = false;
            if exists {
                match conflict {
                    Conflict::Stop => {
                        return Err(Fault::new(
                            ErrorCode::Conflict,
                            "an entry already exists at the destination",
                        ));
                    }
                    Conflict::Replace => {
                        if original.starts_with(target.join(&renamed)) {
                            return Err(invalid("an entry cannot replace itself or its ancestors"));
                        }
                        files::trash(&target, &renamed)?;
                        replaced = true;
                    }
                    Conflict::KeepBoth => {
                        let path = Path::new(&name);
                        let stem = if metadata.is_dir() {
                            name.as_str()
                        } else {
                            path.file_stem()
                                .and_then(|part| part.to_str())
                                .unwrap_or(&name)
                        };
                        let suffix = if metadata.is_dir() {
                            String::new()
                        } else {
                            path.extension()
                                .and_then(|part| part.to_str())
                                .map(|part| format!(".{part}"))
                                .unwrap_or_default()
                        };
                        let mut found = false;
                        for index in 2..=10000 {
                            renamed = format!("{stem} ({index}){suffix}");
                            if !target.join(&renamed).try_exists().map_err(io_error)? {
                                found = true;
                                break;
                            }
                        }
                        if !found {
                            return Err(Fault::new(
                                ErrorCode::Conflict,
                                "no available destination name",
                            ));
                        }
                    }
                }
            }
            let result = if *cut {
                files::rename::between(&source, &target, &name, &renamed)
            } else {
                files::copy::between(&source, &target, &name, &renamed)
            };
            result.map_err(|error| if replaced {
                Fault::new(ErrorCode::OutcomeUnknown, "destination was moved to Trash but transfer failed; inspect both locations")
            } else { error })?;
            Ok(Some(text(&target.join(renamed))?))
        }
    }
}

fn split(path: &Path) -> Result<(PathBuf, String), Fault> {
    if !path.is_absolute() {
        return Err(invalid("an absolute entry path is required"));
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("a filesystem root cannot be modified"))?;
    files::path::entry_components(name)?;
    let parent = resolve(
        path.parent()
            .ok_or_else(|| invalid("entry parent is missing"))?,
    )?;
    Ok((parent, name.into()))
}
