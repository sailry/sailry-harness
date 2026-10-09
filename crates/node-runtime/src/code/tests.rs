use super::*;
use sailry_protocol::WorktreeId;
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
pub(super) mod network;

pub(super) struct Fixture {
    pub _directory: tempfile::TempDir,
    pub root: PathBuf,
    pub bin: PathBuf,
    pub environment: Arc<crate::process::Environment>,
    pub worktree: WorktreeId,
}

impl Fixture {
    pub async fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap().join("project");
        let bin = directory.path().canonicalize().unwrap().join("bin");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(root.join("main.rs"), "🙂 broken symbol\n").unwrap();
        std::fs::write(root.join("target.rs"), "target function\n").unwrap();
        std::fs::write(directory.path().join("outside.rs"), "outside function\n").unwrap();
        let python = which::which("python3").unwrap();
        let script = format!(
            "#!{}\n{}",
            python.display(),
            include_str!("tests/server.py")
        );
        let executable = bin.join("rust-analyzer");
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let environment = Arc::new(
            crate::process::Environment::capture(crate::process::environment::Source::Fixture(
                crate::process::environment::EnvironmentFixture {
                    home: directory.path().into(),
                    shell: "/bin/false".into(),
                    environment: BTreeMap::from([("PATH".into(), bin.as_os_str().to_owned())]),
                    timeout: Duration::from_secs(1),
                },
            ))
            .await,
        );
        Self {
            _directory: directory,
            root,
            bin,
            environment,
            worktree: WorktreeId::new(),
        }
    }

    pub fn settings(&self, value: serde_json::Value) {
        std::fs::write(
            self.root.join("control"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
    pub fn plan(&self) -> resolve::Plan {
        resolve::resolve(
            self.worktree,
            &self.root,
            None,
            &self.environment,
            "main.rs",
        )
        .unwrap()
        .plan
        .unwrap()
    }
    pub fn messages(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.root.join("messages"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}
