//! Validated release history selects one immutable compatible package.
use super::*;

pub(super) async fn hydrate(
    host: &Host,
    mut entry: Entry,
    commit: &str,
    stop: &CancellationToken,
) -> Result<Entry, Fault> {
    let Some(selected) = entry.commit.clone() else {
        entry.commit = Some(commit.into());
        return Ok(entry);
    };
    let value: serde_json::Value = host
        .catalog_json(manifest_url(host, &selected, &entry.id), stop)
        .await?;
    let manifest = manifest::parse(
        &serde_json::to_vec(&value)
            .map_err(|_| Fault::invalid("invalid official plugin manifest"))?,
    )?;
    if manifest.name != entry.id || manifest.version != entry.version || !manifest.issues.is_empty()
    {
        return Err(Fault::invalid(
            "official package version does not match its catalog",
        ));
    }
    let declared = manifest
        .extension
        .as_ref()
        .and_then(|extension| extension.min_sailry_version.as_deref());
    let record = entry
        .versions
        .iter()
        .find(|version| {
            version.commit == selected && Some(&version.version) == entry.version.as_ref()
        })
        .expect("selected official version");
    if declared != record.min_sailry_version.as_deref() {
        return Err(Fault::invalid(
            "official package requirement does not match its catalog",
        ));
    }
    let mut candidate = super::super::entry(&entry.id, manifest);
    candidate.commit = Some(selected);
    candidate.versions = std::mem::take(&mut entry.versions);
    candidate.release_notes = entry.release_notes.take();
    candidate.icon = entry.icon.take();
    candidate.repository = entry.repository.take();
    Ok(candidate)
}

fn manifest_url(host: &Host, commit: &str, id: &str) -> url::Url {
    fixed_url(host, commit)
        .join(&format!("{id}/plugin.json"))
        .unwrap()
}

pub(super) fn package(value: serde_json::Value, icons: &[Entry]) -> Result<Entry, Fault> {
    let package: Package =
        serde_json::from_value(value).map_err(|_| Fault::invalid("invalid official package"))?;
    validate_name(&package.id)?;
    let bytes = serde_json::to_vec(&package.manifest)
        .map_err(|_| Fault::invalid("invalid official plugin manifest"))?;
    let manifest = manifest::parse(&bytes)?;
    if manifest.name != package.id || !manifest.issues.is_empty() {
        return Err(Fault::invalid(
            "invalid official plugin identity or manifest",
        ));
    }
    if package
        .release_notes
        .as_ref()
        .is_some_and(|notes| !notes.valid())
    {
        return Err(Fault::invalid("invalid official release notes"));
    }
    let mut versions = package.versions;
    let mut seen = std::collections::BTreeSet::new();
    for version in &mut versions {
        let parsed = updates::version(Some(&version.version))?;
        if !github::source::commit(&version.commit)
            || !compatibility::valid(version.min_sailry_version.as_deref())
            || version
                .release_notes
                .as_ref()
                .is_some_and(|notes| !notes.valid())
            || !seen.insert(parsed.to_string())
        {
            return Err(Fault::invalid("invalid official version history"));
        }
        version.compatibility = compatibility::evaluate(version.min_sailry_version.as_deref());
    }
    versions.sort_by(|left, right| {
        semver::Version::parse(&right.version)
            .unwrap()
            .cmp_precedence(&semver::Version::parse(&left.version).unwrap())
    });
    if !versions.is_empty() {
        let latest = updates::version(manifest.version.as_deref())?;
        if versions.iter().any(|record| {
            updates::version(Some(&record.version))
                .unwrap()
                .cmp_precedence(&latest)
                .is_gt()
        }) {
            return Err(Fault::invalid(
                "official history exceeds the latest package version",
            ));
        }
        let minimum = manifest
            .extension
            .as_ref()
            .and_then(|extension| extension.min_sailry_version.as_deref());
        if versions
            .iter()
            .find(|record| Some(record.version.as_str()) == manifest.version.as_deref())
            .is_none_or(|record| record.min_sailry_version.as_deref() != minimum)
        {
            return Err(Fault::invalid(
                "official history does not match the latest package",
            ));
        }
    }
    let mut entry = super::super::entry(&package.id, manifest);
    entry.release_notes = package.release_notes;
    if !entry.compatibility.is_compatible()
        && let Some(selected) = versions
            .iter()
            .find(|version| version.compatibility.is_compatible())
    {
        let latest = updates::version(entry.version.as_deref())?;
        if !updates::version(Some(&selected.version))?
            .cmp_precedence(&latest)
            .is_gt()
        {
            entry.version = Some(selected.version.clone());
            entry.commit = Some(selected.commit.clone());
            entry.compatibility = selected.compatibility.clone();
            entry.release_notes = selected.release_notes.clone();
        }
    }
    entry.versions = versions;
    entry.icon = icons
        .iter()
        .find(|icon| icon.id == entry.id)
        .and_then(|icon| icon.icon.clone());
    entry.repository = Some(REPOSITORY.into());
    Ok(entry)
}
