//! Shared by the Desktop and Host build scripts. Only release builds embed the
//! source revision, so development builds never depend on Git state.
use std::{path::Path, process::Command};

pub fn embed() {
    if std::env::var_os("CARGO_CFG_DEBUG_ASSERTIONS").is_some() {
        return;
    }
    let git = |arguments: &[&str]| {
        Command::new("git")
            .args(arguments)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|value| value.trim().to_owned())
    };
    let revision = git(&["rev-parse", "--short=12", "HEAD"])
        .filter(|value| {
            value.len() >= 7 && value.chars().all(|character| character.is_ascii_hexdigit())
        })
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=SAILRY_GIT_SHA={revision}");
    let reference = git(&["symbolic-ref", "-q", "HEAD"]);
    for input in ["HEAD", "packed-refs"]
        .into_iter()
        .chain(reference.as_deref())
    {
        if let Some(path) =
            git(&["rev-parse", "--git-path", input]).filter(|path| Path::new(path).is_file())
        {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}
