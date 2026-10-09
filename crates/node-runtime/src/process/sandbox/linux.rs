use sailry_protocol::Fault;
use std::{ffi::OsString, path::Path};

pub(super) fn complete(args: &mut Vec<OsString>) -> Result<(), Fault> {
    let mut mounts = vec![
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
    ];
    for alias in ["/bin", "/sbin", "/lib", "/lib64"] {
        if let Ok(path) = std::fs::canonicalize(alias)
            && path != Path::new(alias)
        {
            mounts.extend(["--ro-bind".into(), path.into_os_string(), alias.into()]);
        }
    }
    let separator = args
        .iter()
        .position(|arg| arg == "--")
        .ok_or_else(|| super::runtime::failure("sandbox wrapper has no command separator"))?;
    args.splice(separator..separator, mounts);
    Ok(())
}
