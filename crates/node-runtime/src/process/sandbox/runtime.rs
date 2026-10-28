use super::enforcement::Enforcer;
use sailry_protocol::{ErrorCode, Fault};
use std::path::PathBuf;

pub(super) fn enforcer() -> Box<dyn Enforcer> {
    #[cfg(target_os = "macos")]
    {
        Box::new(super::enforcement::macos::Seatbelt::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(super::enforcement::linux::Bubblewrap::new())
    }
}

pub(super) fn wrapper() -> Result<PathBuf, Fault> {
    #[cfg(target_os = "macos")]
    let candidates = ["/usr/bin/sandbox-exec"];
    #[cfg(target_os = "linux")]
    let candidates = ["/usr/bin/bwrap", "/bin/bwrap"];
    trusted(candidates.iter().map(std::path::Path::new))
}

fn trusted<'a>(
    candidates: impl IntoIterator<Item = &'a std::path::Path>,
) -> Result<PathBuf, Fault> {
    for candidate in candidates {
        if let Ok(path) = std::fs::canonicalize(candidate) {
            let metadata = std::fs::metadata(&path).map_err(failure)?;
            use std::os::unix::fs::MetadataExt;
            if metadata.is_file() && metadata.uid() == 0 && metadata.mode() & 0o022 == 0 {
                return Ok(path);
            }
        }
    }
    Err(Fault::new(
        ErrorCode::Unavailable,
        "command sandbox is unavailable; request unrestricted access to continue",
    ))
}

pub(super) fn failure(error: impl std::fmt::Display) -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        format!("command sandbox failed: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_never_selects_user_binary() {
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path().join("bwrap");
        std::fs::write(&program, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(
            trusted([directory.path().join("missing").as_path()])
                .unwrap_err()
                .code,
            ErrorCode::Unavailable
        );
        assert_eq!(
            trusted([program.as_path()]).unwrap_err().code,
            ErrorCode::Unavailable
        );
    }
}
