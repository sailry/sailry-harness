use sailry_release::{Channel, Describe, Merge, Result};
use std::{collections::BTreeMap, env, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("release tool failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut arguments = env::args().skip(1);
    let command = arguments
        .next()
        .ok_or("usage: sailry-release <sign-archive|describe|merge-feed> [options]")?;
    let options = options(arguments.collect())?;
    match command.as_str() {
        "sign-archive" => sailry_release::sign_archive(
            &path(&options, "input")?,
            &path(&options, "output")?,
            value(&options, "name")?,
            &path(&options, "keys")?,
        ),
        "describe" => sailry_release::describe(Describe {
            archive: &path(&options, "archive")?,
            keys: &path(&options, "keys")?,
            minimum_system: value(&options, "minimum-system")?,
            output: &path(&options, "output")?,
            repository: value(&options, "repository")?,
            tag: value(&options, "tag")?,
            target: value(&options, "target")?,
            version: value(&options, "version")?,
        }),
        "merge-feed" => {
            let channel = match value(&options, "channel")? {
                "stable" => Channel::Stable,
                "experience" => Channel::Experience,
                _ => return Err("channel must be stable or experience".into()),
            };
            sailry_release::merge(Merge {
                assets: &path(&options, "assets")?,
                channel,
                feed: &path(&options, "feed")?,
                keys: &path(&options, "keys")?,
                output: &path(&options, "output")?,
                repository: value(&options, "repository")?,
                tag: value(&options, "tag")?,
            })
        }
        _ => Err("unknown release-tool command".into()),
    }
}

fn options(arguments: Vec<String>) -> Result<BTreeMap<String, String>> {
    let (pairs, remainder) = arguments.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err("release-tool options require values".into());
    }
    let mut options = BTreeMap::new();
    for [option, value] in pairs {
        let name = option
            .strip_prefix("--")
            .ok_or("release-tool options must start with --")?;
        if name.is_empty() || options.insert(name.into(), value.clone()).is_some() {
            return Err("release-tool options must be unique and named".into());
        }
    }
    Ok(options)
}

fn value<'a>(options: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str> {
    options
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| format!("missing --{name}").into())
}

fn path(options: &BTreeMap<String, String>, name: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(value(options, name)?))
}
