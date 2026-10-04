use sailry_protocol::{ErrorCode, Fault, office::Environment};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const REQUIREMENTS: &str = include_str!("runtime/requirements.in");
const LOCK: &[u8] = include_bytes!("runtime/requirements.lock");
const PYTHON: &str = "3.12.13";

#[derive(Deserialize)]
struct Manifest {
    version: u32,
    target: String,
    python: String,
    requirements_sha256: String,
    executable: String,
}

pub(crate) fn environment() -> Result<Environment, Fault> {
    let exe = std::env::current_exe().map_err(|_| missing())?;
    locate(&roots(&exe))
}

fn roots(executable: &Path) -> Vec<PathBuf> {
    let Some(parent) = executable.parent() else {
        return Vec::new();
    };
    let mut roots = vec![
        parent.join("office-runtime"),
        parent.join("../Resources/office-runtime"),
    ];
    // Cargo test binaries live in deps next to the development runtime.
    if cfg!(debug_assertions) && parent.file_name().is_some_and(|name| name == "deps") {
        roots.push(parent.join("../office-runtime"));
    }
    roots
}

fn target() -> String {
    let platform = if cfg!(target_os = "macos") {
        "apple-darwin"
    } else if cfg!(windows) {
        "pc-windows-msvc"
    } else {
        "unknown-linux-gnu"
    };
    format!("{}-{platform}", std::env::consts::ARCH)
}

fn locate(roots: &[PathBuf]) -> Result<Environment, Fault> {
    let relative = if cfg!(windows) {
        "python/python.exe"
    } else {
        "python/bin/python3.12"
    };
    let digest = format!("{:x}", Sha256::digest(LOCK));
    for root in roots {
        let Ok(data) = std::fs::read(root.join("runtime.json")) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_slice::<Manifest>(&data) else {
            continue;
        };
        if manifest.target != target()
            || manifest.version != 1
            || manifest.python != PYTHON
            || manifest.requirements_sha256 != digest
            || manifest.executable != relative
        {
            continue;
        }
        let executable = root.join(relative);
        if !executable.is_file() {
            continue;
        }
        return Ok(Environment {
            python: executable
                .canonicalize()
                .map_err(|_| missing())?
                .to_string_lossy()
                .into_owned(),
            python_version: manifest.python,
            packages: REQUIREMENTS
                .lines()
                .filter(|s| !s.trim().is_empty() && !s.starts_with('#'))
                .map(str::to_owned)
                .collect(),
        });
    }
    Err(missing())
}

fn missing() -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        "The bundled Office authoring runtime is missing or does not match this build; package the runtime for this execution Node.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_bundled_dependencies_and_executable() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let executable = if cfg!(windows) {
            "python/python.exe"
        } else {
            "python/bin/python3.12"
        };
        let file = root.join(executable);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"fixture").unwrap();
        let mut manifest = serde_json::json!({"version":1,"target":target(),"python":PYTHON,"requirements_sha256":format!("{:x}", Sha256::digest(LOCK)),"executable":executable});
        let save = |value: &serde_json::Value| {
            std::fs::write(root.join("runtime.json"), value.to_string()).unwrap()
        };
        save(&manifest);
        let environment = locate(&[root.into()]).unwrap();
        assert!(
            environment
                .packages
                .iter()
                .any(|p| p.starts_with("python-docx=="))
        );
        assert_eq!(Path::new(&environment.python), file.canonicalize().unwrap());
        manifest["requirements_sha256"] = "outdated".into();
        save(&manifest);
        assert_eq!(
            locate(&[root.into()]).unwrap_err().code,
            ErrorCode::Unavailable
        );
        manifest["requirements_sha256"] = format!("{:x}", Sha256::digest(LOCK)).into();
        manifest["executable"] = "../../python".into();
        save(&manifest);
        assert!(locate(&[root.into()]).is_err());
    }
}
