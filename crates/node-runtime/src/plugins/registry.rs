//! Skillhub coordinates resolve to verified immutable standalone skill packages.
use super::{Host, io_error};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    plugin::{
        Info,
        skills::{Candidate, Provenance, RegistryDiscovery, RegistryResolved, RegistrySource},
    },
};
use std::fs::File;
mod client;
#[cfg(test)]
mod tests;
mod verification;

pub(crate) use client::Registry;

fn invalid() -> Fault {
    Fault::invalid("invalid Skillhub package identity or signature")
}

fn segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && value != "."
        && value != ".."
}

/// Recognizes only the official directory URL, keeping downloads Node-owned.
pub(crate) fn source_url(input: &str) -> Result<Option<RegistrySource>, Fault> {
    let Ok(url) = url::Url::parse(input) else {
        return Ok(None);
    };
    if !matches!(url.host_str(), Some("skillhub.cn" | "www.skillhub.cn")) {
        return Ok(None);
    }
    let parts = url.path().trim_matches('/').split('/').collect::<Vec<_>>();
    if input.len() > 2048
        || input.contains('\\')
        || input.chars().any(char::is_control)
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
        || !(parts.len() == 2 || parts.len() == 3)
        || parts[0] != "skills"
        || parts[1..].iter().any(|part| !segment(part))
        || input
            .split('?')
            .next()
            .unwrap_or("")
            .split('/')
            .any(|part| matches!(part, "." | ".."))
    {
        return Err(invalid());
    }
    let mut version = None;
    for (key, value) in url.query_pairs() {
        if key != "version" || version.is_some() {
            return Err(invalid());
        }
        version = Some(value.into_owned());
    }
    let source = RegistrySource {
        namespace: (parts.len() == 3).then(|| parts[1].to_owned()),
        slug: parts.last().unwrap().to_string(),
        version,
    };
    validate(&source)?;
    Ok(Some(source))
}

pub(in crate::plugins) fn validate(source: &RegistrySource) -> Result<(), Fault> {
    if !segment(&source.slug)
        || source
            .namespace
            .as_deref()
            .is_some_and(|value| !segment(value))
    {
        return Err(invalid());
    }
    if let Some(version) = &source.version {
        semver::Version::parse(version).map_err(|_| invalid())?;
    }
    Ok(())
}

fn name(source: &RegistryResolved) -> String {
    use sha2::{Digest, Sha256};
    let digest =
        Sha256::digest(format!("skillhub.cn/{}/{}", source.namespace, source.slug).as_bytes());
    format!("skillhub-{}", &format!("{digest:x}")[..24])
}

impl Host {
    pub(in crate::plugins) async fn registry_document(
        &self,
        source: &RegistryResolved,
        stop: CancellationToken,
    ) -> Result<sailry_protocol::plugin::skills::Document, Fault> {
        let (file, _) = self
            .registry
            .acquire(
                &RegistrySource {
                    namespace: Some(source.namespace.clone()),
                    slug: source.slug.clone(),
                    version: Some(source.version.clone()),
                },
                Some(source),
                &stop,
            )
            .await?;
        tokio::task::spawn_blocking(move || {
            super::skill_sources::document::archive(file, "", &stop)
        })
        .await
        .map_err(|_| client::unavailable())?
    }

    pub(crate) async fn discover_registry_skill(
        &self,
        source: &RegistrySource,
        stop: CancellationToken,
    ) -> Result<RegistryDiscovery, Fault> {
        let _permit = self
            .reads
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fault::new(ErrorCode::Busy, "skill discovery is busy"))?;
        let (file, source) = self.registry.acquire(source, None, &stop).await?;
        let resolved = source.clone();
        let candidate = tokio::task::spawn_blocking(move || {
            let (temporary, root, links) = super::archive::unpack_source(file, &stop, 10_000)?;
            if !links.is_empty() {
                return Err(invalid());
            }
            let root = crate::files::path::root(&root)?;
            let bytes = super::package::read(&root, "SKILL.md", super::skills::MAX_BYTES)?
                .ok_or_else(invalid)?;
            let skill =
                super::skills::parse_document(std::str::from_utf8(&bytes).map_err(|_| invalid())?)?;
            let candidate = Candidate {
                name: name(&resolved),
                path: String::new(),
                skill,
            };
            temporary.close().map_err(io_error)?;
            Ok(candidate)
        })
        .await
        .map_err(|_| client::unavailable())??;
        Ok(RegistryDiscovery {
            source,
            skills: vec![candidate],
        })
    }

    pub(crate) async fn install_registry_skill(
        &self,
        source: &RegistryResolved,
        selected_name: &str,
        stop: CancellationToken,
        reporter: Option<crate::installation::Reporter>,
    ) -> Result<Info, Fault> {
        if name(source) != selected_name {
            return Err(invalid());
        }
        let (file, resolved) = self
            .registry
            .acquire(
                &RegistrySource {
                    namespace: Some(source.namespace.clone()),
                    slug: source.slug.clone(),
                    version: Some(source.version.clone()),
                },
                Some(source),
                &stop,
            )
            .await?;
        let host = self.clone();
        let selected_name = selected_name.to_owned();
        tokio::task::spawn_blocking(move || {
            if let Some(reporter) = reporter {
                reporter(sailry_protocol::installation::Phase::Inspecting, None);
            }
            normalize(&host, file, &selected_name, resolved, &stop, true)
        })
        .await
        .map_err(|_| client::unavailable())?
    }

    pub(super) async fn inspect_registry_skill(
        &self,
        source: &RegistryResolved,
        selected_name: &str,
        stop: CancellationToken,
    ) -> Result<Option<Info>, Fault> {
        let _permit = self
            .reads
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fault::new(ErrorCode::Busy, "skill inspection is busy"))?;
        let (file, resolved) = self
            .registry
            .acquire(
                &RegistrySource {
                    namespace: Some(source.namespace.clone()),
                    slug: source.slug.clone(),
                    version: None,
                },
                None,
                &stop,
            )
            .await?;
        let current = semver::Version::parse(&source.version).map_err(|_| invalid())?;
        let available = semver::Version::parse(&resolved.version).map_err(|_| invalid())?;
        if !available.cmp_precedence(&current).is_gt() {
            if available.cmp_precedence(&current).is_eq()
                && (resolved.content_hash != source.content_hash
                    || resolved.archive_sha256 != source.archive_sha256
                    || resolved.version_id != source.version_id)
            {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "published skill version changed",
                ));
            }
            return Ok(None);
        }
        if name(&resolved) != selected_name {
            return Err(invalid());
        }
        let host = self.clone();
        let selected_name = selected_name.to_owned();
        tokio::task::spawn_blocking(move || {
            normalize(&host, file, &selected_name, resolved, &stop, false).map(Some)
        })
        .await
        .map_err(|_| client::unavailable())?
    }
}

fn normalize(
    host: &Host,
    file: File,
    name: &str,
    source: RegistryResolved,
    stop: &CancellationToken,
    install: bool,
) -> Result<Info, Fault> {
    super::skill_sources::package::with_package(
        file,
        name,
        Provenance::Registry { registry: source },
        stop,
        |root| {
            if install {
                host.install(root, "", name)
            } else {
                host.inspect(root, "", name, stop.clone())
            }
        },
    )
}
