use sailry_node_runtime::EnvironmentFixture;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Login {
    pub home: PathBuf,
    pub first: PathBuf,
    pub second: PathBuf,
    shell: PathBuf,
}

impl Login {
    pub fn new(directory: &Path) -> Self {
        let home = directory.join("login-home");
        let first = directory.join("first-programs");
        let second = directory.join("second-programs");
        for directory in [&home, &first, &second] {
            std::fs::create_dir(directory).unwrap();
        }
        let shell = home.join("login-shell");
        executable(
            &shell,
            concat!(
                "#!/bin/sh\n",
                "printf x >> \"$HOME/captures\"\n",
                ". \"$HOME/login.env\"\n",
                "for command do :; done\n",
                "eval \"$command\"\n",
            ),
        );
        let login = Self {
            home,
            first,
            second,
            shell,
        };
        login.select(false);
        for (directory, label) in [(&login.first, "first"), (&login.second, "second")] {
            executable(
                &directory.join("environment-fixture"),
                &format!(
                    "#!/bin/sh\nprintf '{label}:%s:%s' \"${{LOGIN_SECRET-unset}}\" \"${{SAILRY_MODEL_API_KEY-unset}}\"\n"
                ),
            );
            executable(
                &directory.join("codex"),
                &format!(
                    concat!(
                        "#!/bin/sh\n",
                        "printf 'tool-ready:{label}:%s\\n' \"${{EXPLICIT_FIXTURE-unset}}\"\n",
                        "while IFS= read -r value; do printf 'tool-result:{label}:%s\\n' \"$value\"; done\n",
                    ),
                    label = label
                ),
            );
        }
        login
    }

    pub fn select(&self, second: bool) {
        let path = if second { &self.second } else { &self.first };
        std::fs::write(self.home.join("login.env"), format!(
            "PATH={}:\"/usr/bin:/bin\"; export PATH\nLOGIN_SECRET='login-secret'; export LOGIN_SECRET\nSAILRY_MCP_TOKEN='login-token'; export SAILRY_MCP_TOKEN\nprintf 'login banner\\n'\n",
            quote(path.to_str().unwrap()),
        )).unwrap();
    }

    pub fn source(&self) -> EnvironmentFixture {
        EnvironmentFixture {
            home: self.home.clone(),
            shell: self.shell.clone(),
            timeout: Duration::from_secs(5),
            environment: [
                ("PATH".into(), "/usr/bin:/bin".into()),
                ("LANG".into(), "C".into()),
                ("SAILRY_MODEL_API_KEY".into(), "ambient-model-secret".into()),
            ]
            .into(),
        }
    }

    pub fn captures(&self) -> usize {
        std::fs::read(self.home.join("captures"))
            .unwrap_or_default()
            .len()
    }
}

pub fn executable(path: &Path, source: &str) {
    std::fs::write(path, source).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
