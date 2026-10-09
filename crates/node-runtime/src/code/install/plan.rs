use crate::code::rules::{Package, Rules};
use sailry_protocol::{Fault, code};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Serialize)]
pub(super) struct Plan {
    pub consent: code::Install,
    pub kind: Kind,
    pub binaries: BTreeMap<String, String>,
}

#[derive(Clone, Serialize)]
pub(super) enum Kind {
    Archives {
        assets: Vec<Asset>,
        release: Option<String>,
    },
    Toolchain {
        manager: String,
        executable: PathBuf,
        name: String,
        extra: Vec<String>,
    },
}

#[derive(Clone, Serialize)]
pub(super) struct Asset {
    pub url: String,
    pub name: String,
    pub directory: String,
}

pub(in crate::code) fn package(
    package: &Package,
    environment: &crate::process::Environment,
) -> code::Package {
    let version = purl(package)
        .map(|(_, _, version)| version)
        .unwrap_or_default();
    let result = build(package, environment);
    code::Package {
        name: package.name.clone(),
        version,
        homepage: package.homepage.clone().filter(|value| value.len() <= 4096),
        licenses: package
            .licenses
            .iter()
            .filter(|value| value.len() <= 256)
            .take(16)
            .cloned()
            .collect(),
        install: result.as_ref().ok().map(|plan| plan.consent.clone()),
        unavailable: result.err().map(|fault| fault.message),
    }
}

pub(in crate::code) fn version(package: &Package) -> Result<String, Fault> {
    purl(package).map(|(_, _, version)| version)
}

pub(super) fn prepare(
    rules: &Rules,
    consent: &code::Install,
    environment: &crate::process::Environment,
) -> Result<Plan, Fault> {
    let package = rules
        .packages
        .iter()
        .find(|package| package.name == consent.package)
        .ok_or_else(|| Fault::invalid("language package is not in the current registry"))?;
    let plan = build(package, environment)?;
    if plan.consent != *consent {
        return Err(Fault::new(
            sailry_protocol::ErrorCode::RevisionConflict,
            "language installation differs from the consent description",
        ));
    }
    Ok(plan)
}

fn build(package: &Package, environment: &crate::process::Environment) -> Result<Plan, Fault> {
    component(&package.name)?;
    let (manager, name, version) = purl(package)?;
    component(&version)?;
    let source = package
        .source
        .as_object()
        .ok_or_else(|| Fault::invalid("language source is not supported"))?;
    if source.keys().any(|key| {
        !matches!(
            key.as_str(),
            "id" | "asset" | "download" | "extra_packages" | "supported_platforms"
        )
    }) {
        return Err(Fault::invalid(
            "language source requires unsupported build or version rules",
        ));
    }
    if let Some(platforms) = source.get("supported_platforms")
        && !platforms
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item.as_str().is_some_and(target)))
    {
        return Err(Fault::invalid(
            "language package is unavailable on this platform",
        ));
    }
    let (kind, binaries, urls, command) = match manager.as_str() {
        "github" | "generic" => {
            let field = if manager == "github" {
                "asset"
            } else {
                "download"
            };
            let selected =
                select(source.get(field).ok_or_else(|| {
                    Fault::invalid("language package has no downloadable assets")
                })?)?;
            let fields = selected
                .as_object()
                .ok_or_else(|| Fault::invalid("language download has an invalid description"))?;
            if fields
                .keys()
                .any(|key| !matches!(key.as_str(), "target" | "file" | "files" | "bin"))
            {
                return Err(Fault::invalid(
                    "language asset requires unsupported installation rules",
                ));
            }
            let mut substitutions = BTreeMap::from([("version".to_owned(), version.clone())]);
            for (key, value) in fields {
                if let Some(value) = value.as_str() {
                    substitutions.insert(
                        format!("source.{field}.{key}"),
                        render(value, &substitutions)?,
                    );
                }
            }
            let binaries = package
                .bin
                .iter()
                .map(|(name, value)| {
                    component(name)?;
                    let value = render(value, &substitutions)?;
                    let value = value.strip_prefix("exec:").unwrap_or(&value).to_owned();
                    relative(&value)?;
                    Ok((name.clone(), value))
                })
                .collect::<Result<BTreeMap<_, _>, Fault>>()?;
            let mut assets = Vec::new();
            let release = if manager == "github" {
                if name.split('/').count() != 2
                    || name.split('/').any(|part| component(part).is_err())
                {
                    return Err(Fault::invalid("invalid GitHub package source"));
                }
                let file = fields
                    .get("file")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Fault::invalid("GitHub package has no literal release asset"))?;
                let file = render(file, &substitutions)?;
                let (file, directory) = destination(&file)?;
                let base = format!("https://github.com/{name}/releases/download/{version}");
                assets.push(Asset {
                    url: format!("{base}/{file}"),
                    name: file,
                    directory,
                });
                Some(format!(
                    "https://api.github.com/repos/{name}/releases/tags/{version}"
                ))
            } else {
                let files = fields
                    .get("files")
                    .and_then(Value::as_object)
                    .ok_or_else(|| Fault::invalid("generic package has no literal downloads"))?;
                if files.is_empty() || files.len() > 8 {
                    return Err(Fault::invalid("language download count exceeds the limit"));
                }
                for (file, url) in files {
                    let file = render(file, &substitutions)?;
                    let (file, directory) = destination(&file)?;
                    let url = render(
                        url.as_str()
                            .ok_or_else(|| Fault::invalid("language download URL is not text"))?,
                        &substitutions,
                    )?;
                    https(&url)?;
                    assets.push(Asset {
                        url,
                        name: file,
                        directory,
                    });
                }
                None
            };
            for asset in &assets {
                archive(&asset.name)?;
            }
            let urls = assets.iter().map(|asset| asset.url.clone()).collect();
            (Kind::Archives { assets, release }, binaries, urls, None)
        }
        "npm" | "pypi" | "cargo" | "golang" => {
            if source.contains_key("asset") || source.contains_key("download") {
                return Err(Fault::invalid(
                    "language toolchain source requires unsupported assets",
                ));
            }
            let command = match manager.as_str() {
                "npm" => "npm",
                "pypi" => "python3",
                "cargo" => "cargo",
                _ => "go",
            };
            let executable = which::which_in(
                command,
                environment.path(),
                std::env::current_dir().map_err(|error| Fault::invalid(error.to_string()))?,
            )
            .map_err(|_| {
                Fault::invalid(format!(
                    "{command} is not available in the Node login environment"
                ))
            })?;
            let binaries = package
                .bin
                .iter()
                .map(|(bin, value)| {
                    component(bin)?;
                    let expected = format!("{manager}:");
                    let name = value.strip_prefix(&expected).ok_or_else(|| {
                        Fault::invalid("language binary requires an unsupported launcher")
                    })?;
                    component(name)?;
                    Ok((bin.clone(), name.to_owned()))
                })
                .collect::<Result<BTreeMap<_, _>, Fault>>()?;
            let extra = source
                .get("extra_packages")
                .map(|value| {
                    value
                        .as_array()
                        .ok_or_else(|| Fault::invalid("extra language packages are invalid"))
                })
                .transpose()?
                .into_iter()
                .flatten()
                .map(|value| {
                    value
                        .as_str()
                        .filter(|value| {
                            !value.is_empty()
                                && !value.starts_with('-')
                                && value.len() <= 512
                                && !value.chars().any(char::is_control)
                        })
                        .map(str::to_owned)
                        .ok_or_else(|| Fault::invalid("extra language package is invalid"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            if manager != "npm" && !extra.is_empty() {
                return Err(Fault::invalid(
                    "extra packages are supported only for npm sources",
                ));
            }
            let display = match manager.as_str() {
                "npm" => {
                    let mut args = vec![
                        "npm".into(),
                        "install".into(),
                        "--prefix".into(),
                        "<install directory>".into(),
                        format!("{name}@{version}"),
                    ];
                    args.extend(extra.clone());
                    args
                }
                "pypi" => vec![
                    "python3".into(),
                    "-m".into(),
                    "venv".into(),
                    "<install directory>/.venv".into(),
                    "then".into(),
                    "pip".into(),
                    "install".into(),
                    format!("{name}=={version}"),
                ],
                "cargo" => vec![
                    "cargo".into(),
                    "install".into(),
                    "--root".into(),
                    "<install directory>".into(),
                    name.clone(),
                    "--version".into(),
                    version.clone(),
                ],
                _ => vec!["go".into(), "install".into(), format!("{name}@{version}")],
            };
            (
                Kind::Toolchain {
                    manager,
                    executable,
                    name,
                    extra,
                },
                binaries,
                Vec::new(),
                Some(display),
            )
        }
        _ => {
            return Err(Fault::invalid(
                "language package source is not supported by this installer",
            ));
        }
    };
    if binaries.is_empty() || binaries.len() > 32 {
        return Err(Fault::invalid(
            "language package has no bounded binary mapping",
        ));
    }
    let mut plan = Plan {
        consent: code::Install {
            package: package.name.clone(),
            version,
            purl: package
                .source
                .get("id")
                .and_then(Value::as_str)
                .unwrap()
                .into(),
            urls,
            licenses: package.licenses.clone(),
            command,
            revision: String::new(),
        },
        kind,
        binaries,
    };
    let bytes = serde_json::to_vec(&plan).map_err(|error| Fault::invalid(error.to_string()))?;
    if bytes.len() > 16 * 1024 {
        return Err(Fault::invalid(
            "language installation description exceeds the limit",
        ));
    }
    let mut digest = Sha256::new();
    digest.update(bytes);
    digest.update(environment.identity());
    plan.consent.revision = format!("{:x}", digest.finalize());
    Ok(plan)
}

fn purl(package: &Package) -> Result<(String, String, String), Fault> {
    let id = package
        .source
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| id.strip_prefix("pkg:"))
        .ok_or_else(|| Fault::invalid("language package has no supported package URL"))?;
    if id.len() > 1024 {
        return Err(Fault::invalid("language package URL exceeds the limit"));
    }
    if id.contains(['?', '#']) {
        return Err(Fault::invalid(
            "language package URL needs unsupported qualifiers",
        ));
    }
    let (manager, path) = id
        .split_once('/')
        .ok_or_else(|| Fault::invalid("language package URL has no namespace"))?;
    let (name, version) = path
        .rsplit_once('@')
        .ok_or_else(|| Fault::invalid("language package has no pinned version"))?;
    let name = url::form_urlencoded::parse(format!("name={name}").as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default();
    if name.is_empty()
        || name.starts_with('-')
        || name.len() > 512
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"@_./-".contains(&byte))
    {
        return Err(Fault::invalid("language package name is invalid"));
    }
    Ok((manager.into(), name, version.into()))
}
fn select(value: &Value) -> Result<&Value, Fault> {
    if let Some(items) = value.as_array() {
        items
            .iter()
            .find(|item| item.get("target").is_none_or(matches_target))
            .ok_or_else(|| Fault::invalid("language package has no asset for this platform"))
    } else if value.get("target").is_none_or(matches_target) {
        Ok(value)
    } else {
        Err(Fault::invalid(
            "language package asset is unavailable on this platform",
        ))
    }
}
fn matches_target(value: &Value) -> bool {
    value.as_str().is_some_and(target)
        || value
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item.as_str().is_some_and(target)))
}
fn target(value: &str) -> bool {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(windows) {
        "win"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        std::env::consts::OS
    };
    let architecture = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    let musl = cfg!(target_env = "musl")
        || (cfg!(target_os = "linux")
            && std::fs::read_dir("/lib")
                .ok()
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().starts_with("ld-musl-")));
    let libc = if musl { "musl" } else { "gnu" };
    let exact = format!("{os}_{architecture}_{libc}");
    value == os
        || value == format!("{os}_{architecture}")
        || (os == "linux" && value == exact)
        || (cfg!(unix) && value == "unix")
}
fn render(value: &str, variables: &BTreeMap<String, String>) -> Result<String, Fault> {
    let mut result = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("{{") {
        result.push_str(&rest[..start]);
        let token = &rest[start + 2..];
        let end = token
            .find("}}")
            .ok_or_else(|| Fault::invalid("unterminated language asset template"))?;
        let variable = token[..end].trim();
        result.push_str(
            variables
                .get(variable)
                .ok_or_else(|| Fault::invalid("language asset uses an unsupported template"))?,
        );
        rest = &token[end + 2..];
    }
    result.push_str(rest);
    if result.len() > 4096 || result.contains("}}") || result.chars().any(char::is_control) {
        return Err(Fault::invalid("language asset template exceeds the limit"));
    }
    Ok(result)
}
fn destination(value: &str) -> Result<(String, String), Fault> {
    if let Some((file, directory)) = value.split_once(':') {
        component(file)?;
        if !directory.ends_with('/') {
            return Err(Fault::invalid("language asset rename is not supported"));
        }
        relative(directory)?;
        Ok((file.into(), directory.into()))
    } else {
        component(value)?;
        Ok((value.into(), String::new()))
    }
}
fn archive(name: &str) -> Result<(), Fault> {
    if [".zip", ".tar.gz", ".tgz", ".tar.xz", ".gz"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
    {
        return Ok(());
    }
    if [
        ".tar", ".bz2", ".tbz", ".tbz2", ".xz", ".txz", ".zst", ".7z", ".rar", ".lz", ".lzma",
        ".lz4", ".tlz", ".cab", ".deb", ".rpm", ".dmg", ".pkg", ".msi", ".vsix",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
    {
        return Err(Fault::invalid("language archive format is not supported"));
    }
    Ok(())
}
pub(super) fn component(value: &str) -> Result<(), Fault> {
    relative(value)?;
    if value.contains('/')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._+@-".contains(&byte))
    {
        return Err(Fault::invalid(
            "language package component contains a directory",
        ));
    }
    Ok(())
}
pub(super) fn relative(value: &str) -> Result<(), Fault> {
    if value.is_empty()
        || value.len() > 4096
        || value.contains(['\\', ':', '\0'])
        || Path::new(value)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(Fault::invalid("language package path is not confined"));
    }
    Ok(())
}
fn https(value: &str) -> Result<(), Fault> {
    let url = url::Url::parse(value).map_err(|_| Fault::invalid("invalid language asset URL"))?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err(Fault::invalid("language assets require HTTPS"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub(in crate::code) fn fixture() -> crate::code::fixtures::CodeServerFixture {
    let rules = Rules::load().expect("embedded fixture rules");
    let language = rules
        .language("fixture.rs", "")
        .expect("Rust fixture language");
    let command = &rules.servers[&language.servers[0]].command;
    let package = rules.package(command).expect("Rust fixture package");
    let environment = crate::process::Environment::ambient();
    let plan = build(package, &environment).expect("Rust fixture archive source");
    let Kind::Archives {
        release: Some(release),
        ..
    } = plan.kind
    else {
        panic!("Rust fixture release archive expected")
    };
    crate::code::fixtures::CodeServerFixture {
        asset_url: plan.consent.urls[0].clone(),
        release_url: release,
        version: plan.consent.version,
    }
}
