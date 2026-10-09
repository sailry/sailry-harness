//! Original Helix/Mason snapshots retain their licenses in upstream/.
use globset::{Glob, GlobMatcher};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::Read,
    path::Path,
    sync::{Arc, OnceLock},
};
mod registry;
pub(super) use registry::Registry;
use sailry_protocol::Fault;

pub(super) struct Rules {
    pub languages: Vec<Language>,
    pub servers: BTreeMap<String, Server>,
    pub packages: Vec<Package>,
}

pub(super) struct Language {
    pub name: String,
    pub id: String,
    pub extensions: Vec<String>,
    pub patterns: Vec<GlobMatcher>,
    pub shebangs: Vec<String>,
    pub roots: Vec<String>,
    pub servers: Vec<String>,
}

#[derive(Clone)]
pub(super) struct Server {
    pub command: String,
    pub args: Vec<String>,
    pub config: Value,
    pub environment: BTreeMap<String, String>,
    pub timeout: u64,
    pub roots: Vec<String>,
}

#[derive(Clone)]
pub(super) struct Package {
    pub name: String,
    pub source: Value,
    pub bin: BTreeMap<String, String>,
    pub homepage: Option<String>,
    pub licenses: Vec<String>,
}

impl Rules {
    pub fn load() -> Result<Arc<Self>, Fault> {
        static RULES: OnceLock<Result<Arc<Rules>, String>> = OnceLock::new();
        RULES
            .get_or_init(|| {
                Self::parse(
                    include_str!("upstream/languages.toml"),
                    include_bytes!("upstream/registry.json.zip"),
                )
                .map(Arc::new)
                .map_err(|error| error.to_string())
            })
            .as_ref()
            .cloned()
            .map_err(|error| Fault::invalid(format!("language rules are invalid: {error}")))
    }

    fn parse(helix: &str, mason: &[u8]) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let data: toml::Value = toml::from_str(helix)?;
        let mut servers = BTreeMap::new();
        if let Some(entries) = data.get("language-server").and_then(toml::Value::as_table) {
            for (name, item) in entries {
                let Some(command) = item.get("command").and_then(toml::Value::as_str) else {
                    continue;
                };
                let config = item
                    .get("config")
                    .map(serde_json::to_value)
                    .transpose()?
                    .unwrap_or(Value::Null);
                let environment = item
                    .get("environment")
                    .and_then(toml::Value::as_table)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.into())))
                            .collect()
                    })
                    .unwrap_or_default();
                servers.insert(
                    name.clone(),
                    Server {
                        command: command.into(),
                        args: strings(item.get("args")),
                        config,
                        environment,
                        timeout: item
                            .get("timeout")
                            .and_then(toml::Value::as_integer)
                            .and_then(|value| u64::try_from(value).ok())
                            .unwrap_or(20)
                            .clamp(1, 120),
                        roots: strings(item.get("required-root-patterns")),
                    },
                );
            }
        }
        let mut languages = Vec::new();
        for item in data
            .get("language")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(name) = item.get("name").and_then(toml::Value::as_str) else {
                continue;
            };
            let mut extensions = Vec::new();
            let mut patterns = Vec::new();
            for kind in item
                .get("file-types")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(extension) = kind.as_str() {
                    extensions.push(extension.into());
                } else if let Some(pattern) = kind.get("glob").and_then(toml::Value::as_str) {
                    match Glob::new(pattern) {
                        Ok(glob) => patterns.push(glob.compile_matcher()),
                        Err(error) => eprintln!("skipping invalid language glob {name}: {error}"),
                    }
                }
            }
            let candidates = item
                .get("language-servers")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|value| {
                    value
                        .as_str()
                        .or_else(|| value.get("name").and_then(toml::Value::as_str))
                })
                .filter(|name| servers.contains_key(*name))
                .map(str::to_owned)
                .collect();
            languages.push(Language {
                name: name.into(),
                id: item
                    .get("language-id")
                    .and_then(toml::Value::as_str)
                    .unwrap_or(name)
                    .into(),
                extensions,
                patterns,
                shebangs: strings(item.get("shebangs")),
                roots: strings(item.get("roots")),
                servers: candidates,
            });
        }
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(mason))?;
        let mut text = String::new();
        let file = archive.by_name("registry.json")?;
        if file.size() > 16 * 1024 * 1024 {
            return Err("Mason registry exceeds the expanded limit".into());
        }
        file.take(16 * 1024 * 1024).read_to_string(&mut text)?;
        let entries: Vec<Value> = serde_json::from_str(&text)?;
        let packages = entries
            .into_iter()
            .filter_map(|entry| {
                let name = entry.get("name")?.as_str()?.to_owned();
                if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
                    return None;
                }
                let source = entry.get("source")?.clone();
                let bin = entry
                    .get("bin")
                    .and_then(Value::as_object)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|(name, value)| {
                                Some((name.clone(), value.as_str()?.into()))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let homepage = entry
                    .get("homepage")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let licenses = entry
                    .get("licenses")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                Some(Package {
                    name,
                    source,
                    bin,
                    homepage,
                    licenses,
                })
            })
            .collect();
        Ok(Self {
            languages,
            servers,
            packages,
        })
    }

    pub fn language(&self, path: &str, text: &str) -> Option<&Language> {
        let file = Path::new(path);
        let name = file.file_name()?.to_str()?;
        let extension = file.extension().and_then(|value| value.to_str());
        self.languages
            .iter()
            .find(|language| {
                language
                    .patterns
                    .iter()
                    .any(|pattern| pattern.is_match(path) || pattern.is_match(name))
            })
            .or_else(|| {
                self.languages.iter().find(|language| {
                    language
                        .extensions
                        .iter()
                        .any(|value| Some(value.as_str()) == extension || value == name)
                })
            })
            .or_else(|| {
                let first = text.lines().next()?.strip_prefix("#!")?;
                let mut words = first.split_whitespace();
                let mut program = words.next()?.rsplit('/').next()?;
                if program == "env" {
                    loop {
                        let word = words.next()?;
                        if matches!(word, "-u" | "--unset") {
                            words.next()?;
                            continue;
                        }
                        if word.starts_with('-') || word.contains('=') {
                            continue;
                        }
                        program = word.trim_matches(['\'', '"']).rsplit('/').next()?;
                        break;
                    }
                }

                self.languages
                    .iter()
                    .find(|language| language.shebangs.iter().any(|value| value == program))
            })
    }

    pub fn package(&self, command: &str) -> Option<&Package> {
        let mut matches = self
            .packages
            .iter()
            .filter(|package| package.bin.contains_key(command));
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    }
}

fn strings(value: Option<&toml::Value>) -> Vec<String> {
    value
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests;
