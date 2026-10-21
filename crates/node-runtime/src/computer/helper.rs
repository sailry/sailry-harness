//! Locate an execution Node's packaged helper without starting package code.
use sailry_protocol::{ErrorCode, Fault};
use std::path::{Path, PathBuf};

pub(crate) const TOKEN: &str = "${SAILRY_COMPUTER_USE}";

pub(crate) fn executable() -> Result<PathBuf, Fault> {
    #[cfg(target_os = "macos")]
    {
        static VERSION: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
        let version = VERSION.get_or_init(|| {
            sysinfo::System::os_version()?
                .split('.')
                .next()?
                .parse()
                .ok()
        });
        if !version.is_some_and(|major| major >= 14) {
            return Err(unavailable("computer use requires macOS 14 or later"));
        }
    }
    let platform = if cfg!(target_os = "macos") {
        Platform::Mac
    } else if cfg!(windows) {
        Platform::Windows
    } else {
        return Err(unavailable("computer use is unavailable on this platform"));
    };
    let executable = std::env::current_exe()
        .map_err(|_| unavailable("execution Node location is unavailable"))?;
    let helpers = std::env::var_os("SAILRY_HELPERS_DIR").map(PathBuf::from);
    let path = locate(platform, &executable, helpers.as_deref())?;
    validate(&path)?;
    Ok(path)
}

#[derive(Clone, Copy)]
enum Platform {
    Mac,
    Windows,
}

fn locate(platform: Platform, executable: &Path, helpers: Option<&Path>) -> Result<PathBuf, Fault> {
    let parent = executable
        .parent()
        .ok_or_else(|| unavailable("execution Node location is unavailable"))?;
    let helpers = helpers.map(Path::to_owned).unwrap_or_else(|| {
        if matches!(platform, Platform::Mac)
            && parent.file_name().is_some_and(|name| name == "MacOS")
            && parent
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "Contents")
        {
            parent.parent().expect("Contents directory").join("Helpers")
        } else {
            parent.join("helpers")
        }
    });
    Ok(helpers.join(match platform {
        Platform::Mac => "Sailry Computer Use.app/Contents/MacOS/OpenComputerUse",
        Platform::Windows => "open-computer-use.exe",
    }))
}

fn validate(path: &Path) -> Result<(), Fault> {
    let metadata = path
        .metadata()
        .map_err(|_| unavailable("computer helper is missing"))?;
    if !metadata.is_file() {
        return Err(unavailable("computer helper is not a regular executable"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(unavailable("computer helper is not executable"));
        }
    }
    Ok(())
}

fn unavailable(message: &str) -> Fault {
    Fault::new(ErrorCode::Unavailable, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_packaged_layouts() {
        let suffix = "Sailry Computer Use.app/Contents/MacOS/OpenComputerUse";
        assert_eq!(
            locate(
                Platform::Mac,
                Path::new("/Applications/Sailry.app/Contents/MacOS/sailry"),
                None
            )
            .unwrap(),
            Path::new("/Applications/Sailry.app/Contents/Helpers").join(suffix)
        );
        assert_eq!(
            locate(Platform::Mac, Path::new("/host/sailry-host"), None).unwrap(),
            Path::new("/host/helpers").join(suffix)
        );
        assert_eq!(
            locate(Platform::Windows, Path::new("/host/sailry-host.exe"), None).unwrap(),
            Path::new("/host/helpers/open-computer-use.exe")
        );
        for platform in [Platform::Mac, Platform::Windows] {
            let path = locate(
                platform,
                Path::new("/host/sailry"),
                Some(Path::new("/development/helpers")),
            )
            .unwrap();
            assert!(path.starts_with("/development/helpers"));
        }
    }

    #[test]
    fn rejects_missing_and_nonexecutables() {
        let root = tempfile::tempdir().unwrap();
        assert!(validate(&root.path().join("missing")).is_err());
        assert!(validate(root.path()).is_err());
        let file = root.path().join("helper");
        std::fs::write(&file, "fixture").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert!(validate(&file).is_err());
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        validate(&file).unwrap();
    }
}
