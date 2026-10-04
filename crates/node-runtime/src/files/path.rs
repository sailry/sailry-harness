//! Capability traversal adapted from sailry-code 67ae9fa0:
//! sailry-code-secure-file/src/secure/{anchor,resolve,path}.rs (Apache-2.0).
//! Read operations retain directory handles; they never reopen an ambient child path.
use std::path::{Component, Path};

use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use sailry_protocol::{ErrorCode, Fault};

use super::io_error;

#[path = "path/identity.rs"]
pub(crate) mod identity;

pub(crate) fn entry_components(value: &str) -> Result<Vec<&str>, Fault> {
    let parts = components(value, false)?;
    if parts.iter().any(|part| part.eq_ignore_ascii_case(".git")) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "Git metadata cannot be changed through file operations",
        ));
    }
    Ok(parts)
}

pub(crate) fn components(path: &str, allow_root: bool) -> Result<Vec<&str>, Fault> {
    if allow_root && path.is_empty() {
        return Ok(Vec::new());
    }
    if path.len() > 4096 || path.contains(['\\', ':']) || path.chars().any(char::is_control) {
        return Err(invalid());
    }
    let parts: Vec<_> = path.split('/').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return Err(invalid());
    }
    #[cfg(windows)]
    if parts.iter().any(|part| !windows_component(part)) {
        return Err(invalid());
    }
    Ok(parts)
}

pub(crate) fn root(path: &Path) -> Result<Dir, Fault> {
    if !path.is_absolute() {
        return Err(invalid());
    }
    // Registration canonicalizes the root. Reject subsequent symlink replacements
    // of every ancestor instead of following them into a different project.
    let mut base = std::path::PathBuf::new();
    let mut names = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => base.push(component),
            Component::Normal(name) => names.push(name),
            _ => return Err(invalid()),
        }
    }
    #[cfg(windows)]
    let mut dir = identity::windows::open(&base)?;
    #[cfg(not(windows))]
    let mut dir = Dir::open_ambient_dir(base, cap_std::ambient_authority()).map_err(io_error)?;
    for name in names {
        dir = dir.open_dir_nofollow(name).map_err(io_error)?;
        #[cfg(windows)]
        identity::directory(&dir)?;
    }
    Ok(dir)
}

pub(crate) fn descend(mut dir: Dir, parts: &[&str]) -> Result<Dir, Fault> {
    for name in parts {
        dir = dir.open_dir_nofollow(name).map_err(io_error)?;
        #[cfg(windows)]
        identity::directory(&dir)?;
    }
    Ok(dir)
}

/// Prepare missing parent directories for an admitted file creation.
/// Existing entries are opened without following symlinks on every step.
pub(crate) fn create_parents(base: &Path, relative: &str) -> Result<(), Fault> {
    let parts = components(relative, false)?;
    let mut dir = root(base)?;
    for name in &parts[..parts.len() - 1] {
        match dir.create_dir(name) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io_error(error)),
        }
        dir = dir.open_dir_nofollow(name).map_err(io_error)?;
        #[cfg(windows)]
        identity::directory(&dir)?;
    }
    Ok(())
}

// Win32 aliases must not turn a relative filename into a device or a different
// entry. This is execution-platform validation, not controller-side parsing.
#[cfg(any(windows, test))]
fn windows_component(name: &str) -> bool {
    if name.ends_with(['.', ' ']) || name.contains(['<', '>', '"', '|', '?', '*']) {
        return false;
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    !matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) && !["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}

fn invalid() -> Fault {
    Fault::new(
        ErrorCode::InvalidRequest,
        "expected a normalized worktree-relative path",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unconfined_paths() {
        // Regression cases retained from the old SecureRelativePath tests.
        for value in [
            "",
            "/etc/passwd",
            "../x",
            "a/../x",
            "./x",
            "a//x",
            "a/",
            "C:/x",
            "C:x",
            r"C:\x",
            r"\\server\share",
            r"a\b",
            "a\0b",
            "a\n/b",
            "a\u{7f}b",
        ] {
            assert!(components(value, false).is_err(), "accepted {value:?}");
        }
        assert!(components("", true).unwrap().is_empty());
        assert_eq!(
            components("项目/中文 文件.rs", false).unwrap(),
            ["项目", "中文 文件.rs"]
        );
    }

    #[test]
    fn rejects_windows_aliases() {
        for name in [
            "NUL",
            "nul.txt",
            "CON",
            "CON .txt",
            "COM1",
            "LPT9.log",
            "COM¹",
            "LPT².txt",
            "CONOUT$",
            "file.",
            "file ",
            "a?b",
        ] {
            assert!(!windows_component(name), "accepted {name:?}");
        }
        for name in ["COM10", "console.rs", "文件.txt", "my file", ".gitignore"] {
            assert!(windows_component(name), "rejected {name:?}");
        }
    }

    #[test]
    fn retains_native_root_identity() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let folder = base.join("project with spaces").join("资料");
        std::fs::create_dir_all(&folder).unwrap();
        let retained = root(&folder).unwrap();
        assert_eq!(
            identity::directory(&retained).unwrap(),
            identity::directory(&root(&folder).unwrap()).unwrap()
        );
        assert!(root(Path::new("relative/root")).is_err());
        assert!(root(&base.join("..")).is_err());
    }
}
