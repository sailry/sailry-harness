use sailry_node_runtime::EnvironmentFixture;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) struct Login {
    pub(super) home: PathBuf,
    programs: PathBuf,
}

impl Login {
    pub(super) fn new(directory: &Path) -> Self {
        let home = directory.join("login-home");
        let programs = directory.join("first-programs");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&programs).unwrap();
        std::fs::write(
            programs.join("gh.cmd"),
            include_str!("windows.cmd")
                .lines()
                .map(|line| format!("{line}\r\n"))
                .collect::<String>(),
        )
        .unwrap();
        Self { home, programs }
    }

    pub(super) fn source(&self) -> EnvironmentFixture {
        let mut environment: BTreeMap<OsString, OsString> = [
            ("HOME".into(), self.home.clone().into_os_string()),
            ("USERPROFILE".into(), self.home.clone().into_os_string()),
            ("PATH".into(), self.programs.clone().into_os_string()),
            ("PATHEXT".into(), ".COM;.EXE;.BAT;.CMD".into()),
            ("SAILRY_MODEL_API_KEY".into(), "ambient-model-secret".into()),
        ]
        .into();
        for name in ["SystemRoot", "SystemDrive", "COMSPEC", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(name) {
                environment.insert(name.into(), value);
            }
        }
        EnvironmentFixture {
            home: self.home.clone(),
            shell: std::env::var_os("COMSPEC")
                .map(PathBuf::from)
                .unwrap_or_else(|| "cmd.exe".into()),
            environment,
            timeout: Duration::from_secs(5),
        }
    }
}
