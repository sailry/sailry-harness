//! Launch mode is selected before creating a runtime, profile, or window.
use std::{ffi::OsString, path::PathBuf};

pub(crate) enum Launch {
    Preview,
    Node(Options),
}

pub(crate) struct Options {
    pub data_dir: PathBuf,
    pub relays: Vec<String>,
}

pub(crate) fn parse(args: impl Iterator<Item = OsString>) -> Result<Option<Launch>, String> {
    let mut args = args.peekable();
    if args
        .peek()
        .is_some_and(|flag| flag == "--help" || flag == "-h")
    {
        args.next();
        if args.next().is_some() {
            return Err("--help cannot be combined with other options".into());
        }
        println!(
            "Usage: sailry-desktop [--data-dir <absolute private directory>] [--relay <HTTPS URL> ...]\n       sailry-desktop --preview\nDefaults to ~/.sailry. Preview does not start a Node or open user data."
        );
        return Ok(None);
    }
    let mut data_dir = None;
    let mut relays = Vec::new();
    let mut preview = false;
    while let Some(flag) = args.next() {
        if flag == "--preview" && !preview {
            preview = true;
        } else if flag == "--data-dir" && data_dir.is_none() {
            let path = PathBuf::from(args.next().ok_or("--data-dir requires a path")?);
            if !path.is_absolute() {
                return Err("--data-dir must be an absolute path".into());
            }
            data_dir = Some(path);
        } else if flag == "--relay" {
            relays.push(
                args.next()
                    .ok_or("--relay requires a URL")?
                    .into_string()
                    .map_err(|_| "relay URL must be UTF-8")?,
            );
        } else {
            return Err("unknown or repeated option; use --help for usage".into());
        }
    }
    if preview {
        if data_dir.is_some() || !relays.is_empty() {
            return Err("--preview cannot be combined with Node options".into());
        }
        return Ok(Some(Launch::Preview));
    }
    let data_dir = match data_dir {
        Some(path) => path,
        None => sailry_node_runtime::default_data_dir().map_err(|error| error.to_string())?,
    };
    Ok(Some(Launch::Node(Options { data_dir, relays })))
}

#[cfg(test)]
mod tests;
