use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn normalize(name: &str) -> String {
    let name = name.trim().to_lowercase();
    let name = name
        .split_once('/')
        .map_or(name.as_str(), |(_, model)| model);
    let mut normalized = name.replace(['.', '_'], "-");
    if let Some(base) = normalized.strip_suffix("-latest") {
        normalized = base.to_owned();
    }
    let segments: Vec<_> = normalized.split('-').collect();
    let date = segments.len() >= 4
        && segments[segments.len() - 3..]
            .iter()
            .zip([4, 2, 2])
            .all(|(segment, length)| {
                segment.len() == length && segment.bytes().all(|byte| byte.is_ascii_digit())
            });
    if date {
        normalized.truncate(normalized.len() - 11);
    } else if let Some((base, suffix)) = normalized.rsplit_once('-')
        && (4..=8).contains(&suffix.len())
        && suffix.bytes().all(|byte| byte.is_ascii_digit())
    {
        normalized = base.to_owned();
    }
    normalized
}

pub(crate) fn resolve(
    candidates: &[String],
    models: &BTreeSet<String>,
    aliases: &BTreeMap<String, String>,
) -> Option<String> {
    let mut matches = BTreeSet::new();
    for candidate in candidates {
        let normalized = normalize(candidate);
        let direct: Vec<_> = models
            .iter()
            .filter(|model| normalize(model) == normalized)
            .collect();
        if !direct.is_empty() {
            matches.extend(direct.into_iter().cloned());
            continue;
        }
        for (alias, model) in aliases {
            if normalize(alias) == normalized {
                matches.insert(model.clone());
            }
        }
        if let Some(alias) = super::aliases::resolve(&normalized) {
            for model in models {
                if normalize(model) == alias {
                    matches.insert(model.clone());
                }
            }
        }
    }
    (matches.len() == 1).then(|| matches.into_iter().next().unwrap())
}
