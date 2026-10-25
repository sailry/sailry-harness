//! Check rustup proxies without starting a process or installing a component.
use super::*;

pub(super) fn available(
    path: &Path,
    directory: Option<&Path>,
    environment: &crate::process::Environment,
    args: &[String],
    variables: &BTreeMap<String, String>,
) -> bool {
    if !proxy(path) {
        return true;
    }
    // Unknown configuration remains a normal startup error. A known missing
    // component must not make its rustup launcher look like an installed server.
    component(path, directory, environment, args, variables).unwrap_or(true)
}

fn proxy(path: &Path) -> bool {
    if path
        .file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.eq_ignore_ascii_case("rustup") || name.eq_ignore_ascii_case("rustup-init")
        })
    {
        return false;
    }
    if path.canonicalize().ok().is_some_and(|resolved| {
        matches!(
            resolved.file_stem().and_then(|name| name.to_str()),
            Some("rustup" | "rustup-init")
        )
    }) {
        return true;
    }
    let Some(directory) = path.parent() else {
        return false;
    };
    let identity = |path: &Path| {
        let file = std::fs::File::open(path).ok()?;
        crate::files::path::identity::file(&cap_std::fs::File::from_std(file)).ok()
    };
    let Some(expected) = identity(path) else {
        return false;
    };
    ["rustup", "rustup.exe"]
        .into_iter()
        .any(|name| identity(&directory.join(name)).is_some_and(|actual| actual == expected))
}

fn component(
    path: &Path,
    directory: Option<&Path>,
    environment: &crate::process::Environment,
    args: &[String],
    variables: &BTreeMap<String, String>,
) -> Result<bool, ()> {
    let home = variables
        .get("RUSTUP_HOME")
        .map(PathBuf::from)
        .or_else(|| environment.home().map(|home| home.join(".rustup")))
        .ok_or(())?;
    let settings = read(&home.join("settings.toml"))?
        .map(|contents| toml::from_str::<toml::Value>(&contents).map_err(|_| ()))
        .transpose()?
        .unwrap_or_else(|| toml::Value::Table(Default::default()));
    let selected = args
        .first()
        .and_then(|argument| argument.strip_prefix('+'))
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .or_else(|| variables.get("RUSTUP_TOOLCHAIN").cloned());
    let selected = match selected {
        Some(selected) => Some(selected),
        None => override_(directory, &settings)?,
    }
    .or_else(|| {
        settings
            .get("default_toolchain")
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
    });
    let Some(selected) = selected else {
        return Ok(false);
    };
    let toolchain = toolchain(&home, &selected, &settings)?;
    Ok(toolchain.is_some_and(|toolchain| {
        path.file_name()
            .and_then(|command| super::installed(toolchain.join("bin").join(command)))
            .is_some()
    }))
}

fn override_(directory: Option<&Path>, settings: &toml::Value) -> Result<Option<String>, ()> {
    for directory in directory.into_iter().flat_map(Path::ancestors) {
        if let Some(name) = directory
            .to_str()
            .and_then(|path| settings.get("overrides").and_then(|value| value.get(path)))
            .and_then(toml::Value::as_str)
        {
            return Ok(Some(name.into()));
        }
        for (file, legacy) in [("rust-toolchain", true), ("rust-toolchain.toml", false)] {
            let Some(contents) = read(&directory.join(file))? else {
                continue;
            };
            if legacy && contents.lines().count() == 1 {
                return Ok(Some(contents.trim().into()));
            }
            let config = toml::from_str::<toml::Value>(&contents).map_err(|_| ())?;
            let toolchain = config.get("toolchain").ok_or(())?;
            return Ok(toolchain
                .get("channel")
                .or_else(|| toolchain.get("path"))
                .and_then(toml::Value::as_str)
                .map(str::to_owned));
        }
    }
    Ok(None)
}

fn toolchain(home: &Path, name: &str, settings: &toml::Value) -> Result<Option<PathBuf>, ()> {
    if Path::new(name).is_absolute() {
        return Ok(Some(name.into()));
    }
    if name.is_empty() || Path::new(name).components().count() != 1 {
        return Err(());
    }
    let root = home.join("toolchains");
    let exact = root.join(name);
    if exact.is_dir() {
        return Ok(Some(exact));
    }
    if let Some(host) = settings
        .get("default_host_triple")
        .and_then(toml::Value::as_str)
    {
        return Ok(Some(root.join(format!("{name}-{host}"))));
    }
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    let prefix = format!("{name}-");
    let mut matching = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.strip_prefix(&prefix).is_some_and(|host| {
                    host.starts_with(|character: char| character.is_ascii_alphabetic())
                })
            })
        })
        .map(|entry| entry.path());
    let selected = matching.next();
    if matching.next().is_some() {
        return Err(());
    }
    Ok(selected)
}

fn read(path: &Path) -> Result<Option<String>, ()> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(()),
    }
}

#[cfg(test)]
mod tests;
