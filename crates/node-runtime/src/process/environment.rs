#[cfg(unix)]
mod account;
#[cfg(unix)]
mod login;
#[cfg(test)]
mod tests;

use super::Launch;
#[cfg(unix)]
use account::Account;
#[cfg(any(test, feature = "test-support"))]
use std::path::PathBuf;
#[cfg(any(unix, test, feature = "test-support"))]
use std::time::Duration;
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::Path,
    process::Stdio,
};

#[cfg(unix)]
const LIMIT: usize = 1024 * 1024;
#[cfg(all(unix, not(any(test, feature = "test-support"))))]
const TIMEOUT: Duration = Duration::from_secs(5);
const VARIABLES: &[&str] = &[
    "HOME",
    "USERPROFILE",
    "PATH",
    "PATHEXT",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "SystemRoot",
    "SystemDrive",
    "COMSPEC",
    "TEMP",
    "TMP",
    "TMPDIR",
];

#[derive(Clone)]
pub(crate) struct Environment {
    values: BTreeMap<OsString, OsString>,
    terminal: BTreeMap<OsString, OsString>,
    scoped: BTreeMap<OsString, OsString>,
    unrestricted: BTreeMap<OsString, OsString>,
}

#[derive(Default)]
pub(crate) enum Source {
    // Ordinary test Nodes must not execute the developer's shell profiles.
    #[cfg(any(test, feature = "test-support"))]
    #[default]
    Ambient,
    #[cfg(not(any(test, feature = "test-support")))]
    #[default]
    System,
    #[cfg(any(test, feature = "test-support"))]
    Fixture(EnvironmentFixture),
}

/// Explicit login inputs for isolated service tests; never reads user shell profiles.
#[cfg(any(test, feature = "test-support"))]
pub struct EnvironmentFixture {
    pub home: PathBuf,
    pub shell: PathBuf,
    pub environment: BTreeMap<OsString, OsString>,
    pub timeout: Duration,
}

impl Environment {
    pub(crate) fn ambient() -> Self {
        Self::filtered(std::env::vars_os())
    }

    pub(crate) fn filtered(values: impl IntoIterator<Item = (OsString, OsString)>) -> Self {
        let mut result = Self {
            values: BTreeMap::new(),
            terminal: BTreeMap::new(),
            scoped: BTreeMap::new(),
            unrestricted: BTreeMap::new(),
        };
        for (key, value) in values {
            if !secret(&key) {
                result.unrestricted.insert(key.clone(), value.clone());
                if scoped_variable(&key, cfg!(windows)) {
                    result.scoped.insert(key.clone(), value.clone());
                }
            }
            #[cfg(target_os = "macos")]
            if key == OsStr::new("DEVELOPER_DIR") {
                if let Some(path) = developer_directory(Path::new(&value)) {
                    result
                        .values
                        .insert(key.clone(), path.clone().into_os_string());
                    result
                        .scoped
                        .insert(key.clone(), path.clone().into_os_string());
                    result.terminal.insert(key, path.into_os_string());
                }
                continue;
            }
            if let Some(name) = canonical(&key, cfg!(windows)) {
                let key = OsString::from(name);
                result.values.insert(key.clone(), value.clone());
                result.scoped.insert(key.clone(), value.clone());
                result.terminal.insert(key, value);
            } else if terminal_variable(&key) {
                result.terminal.insert(key, value);
            }
        }
        result
    }

    pub(crate) async fn capture(source: Source) -> Self {
        match source {
            #[cfg(any(test, feature = "test-support"))]
            Source::Ambient => Self::ambient(),
            #[cfg(not(any(test, feature = "test-support")))]
            Source::System => {
                let environment = Self::ambient();
                #[cfg(unix)]
                {
                    let deadline = tokio::time::Instant::now() + TIMEOUT;
                    let account = tokio::time::timeout_at(
                        deadline,
                        tokio::task::spawn_blocking(Account::current),
                    )
                    .await;
                    if let Ok(Ok(Ok(account))) = account {
                        return environment.login(account, deadline).await;
                    }
                }
                environment
            }
            #[cfg(any(test, feature = "test-support"))]
            Source::Fixture(fixture) => {
                let environment = Self::filtered(fixture.environment);
                #[cfg(unix)]
                {
                    let account = Account {
                        name: "fixture".into(),
                        home: fixture.home.into_os_string(),
                        shell: fixture.shell.into_os_string(),
                    };
                    environment
                        .login(account, tokio::time::Instant::now() + fixture.timeout)
                        .await
                }
                #[cfg(not(unix))]
                environment
            }
        }
    }

    #[cfg(unix)]
    async fn login(mut self, account: Account, deadline: tokio::time::Instant) -> Self {
        self.values.insert("HOME".into(), account.home.clone());
        self.scoped.insert("HOME".into(), account.home.clone());
        self.scoped.insert("USER".into(), account.name.clone());
        account.apply(&mut self.unrestricted);
        account.apply(&mut self.terminal);
        if let Some(values) = login::capture(&account.shell, &self.terminal, deadline).await {
            let mut captured = Self::filtered(values);
            account.apply(&mut captured.terminal);
            // Shell exports supplement the frozen, safe startup inputs.
            self.values.append(&mut captured.values);
            self.terminal.append(&mut captured.terminal);
            self.scoped.append(&mut captured.scoped);
            self.unrestricted.append(&mut captured.unrestricted);
        }
        self
    }

    pub(crate) fn shell(&self) -> &OsStr {
        #[cfg(unix)]
        {
            OsStr::new("/bin/sh")
        }
        #[cfg(windows)]
        {
            self.values
                .get(OsStr::new("COMSPEC"))
                .map(OsString::as_os_str)
                .unwrap_or_else(|| OsStr::new("cmd.exe"))
        }
    }

    pub(crate) fn path(&self) -> Option<&OsStr> {
        self.values.get(OsStr::new("PATH")).map(OsString::as_os_str)
    }

    #[cfg(target_os = "macos")]
    pub(super) fn developer(&self) -> Option<&Path> {
        self.values.get(OsStr::new("DEVELOPER_DIR")).map(Path::new)
    }

    pub(crate) fn home(&self) -> Option<&Path> {
        self.values
            .get(OsStr::new("HOME"))
            .or_else(|| self.values.get(OsStr::new("USERPROFILE")))
            .map(Path::new)
    }

    #[cfg(unix)]
    pub(crate) fn login_shell(&self) -> Option<&OsStr> {
        self.terminal
            .get(OsStr::new("SHELL"))
            .map(OsString::as_os_str)
    }

    pub(crate) fn apply(&self, command: &mut tokio::process::Command) {
        command.env_clear().envs(&self.values);
    }

    pub(crate) fn apply_native(&self, command: &mut tokio::process::Command) {
        // Native Git retains configured credentials while using frozen login paths.
        command.envs(&self.terminal);
    }

    pub(crate) fn apply_terminal(&self, command: &mut portable_pty::CommandBuilder) {
        command.env_clear();
        for (key, value) in &self.terminal {
            command.env(key, value);
        }
    }

    pub(super) fn apply_pty(
        &self,
        command: &mut portable_pty::CommandBuilder,
        access: sailry_protocol::process::Access,
    ) {
        command.env_clear();
        for (key, value) in self.command_values(access) {
            command.env(key, value);
        }
    }

    fn command_values(
        &self,
        access: sailry_protocol::process::Access,
    ) -> &BTreeMap<OsString, OsString> {
        match access {
            sailry_protocol::process::Access::Workspace => &self.scoped,
            sailry_protocol::process::Access::Unrestricted => &self.unrestricted,
        }
    }

    pub(super) fn command(
        &self,
        launch: &Launch,
        directory: &Path,
        inputs: Option<&Path>,
    ) -> tokio::process::Command {
        #[cfg(windows)]
        let native = super::pty_directory(directory);
        #[cfg(windows)]
        let directory = native.as_path();
        let mut command = tokio::process::Command::new(self.shell());
        #[cfg(unix)]
        command.args(["-c", &launch.command]);
        #[cfg(windows)]
        command
            .args(["/D", "/S", "/C"])
            .raw_arg(format!("\"{}\"", launch.command));
        command.env_clear().envs(self.command_values(launch.access));
        if let Some(python) = &launch.python {
            command.envs(python.variables());
        }
        if let Some(inputs) = inputs {
            command.env("SAILRY_ATTACHMENTS", inputs);
        }
        if let Some(scratch) = &launch.scratch {
            for name in ["TMPDIR", "TMP", "TEMP"] {
                command.env(name, scratch.path());
            }
        }
        command
            .current_dir(directory)
            .env("PWD", directory)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
}

fn secret(key: &OsStr) -> bool {
    let name = key.to_string_lossy().to_ascii_uppercase();
    name == "KEY"
        || ["TOKEN", "SECRET", "PASSW", "APIKEY", "CREDENTIAL"]
            .iter()
            .any(|part| name.contains(part))
        || name.ends_with("_KEY")
        || name.ends_with("_DSN")
        || name.starts_with("AWS_")
}

fn scoped_variable(key: &OsStr, insensitive: bool) -> bool {
    let name = key.to_string_lossy();
    let scoped = if insensitive {
        name.to_ascii_uppercase()
    } else {
        name.to_string()
    };
    matches!(scoped.as_str(), "SSH_AUTH_SOCK" | "USER")
        || scoped.starts_with("LC_")
        || matches!(
            name.to_ascii_lowercase().as_str(),
            "http_proxy" | "https_proxy" | "all_proxy" | "no_proxy" | "ftp_proxy"
        )
}

fn canonical(key: &OsStr, insensitive: bool) -> Option<&'static str> {
    VARIABLES.iter().copied().find(|name| {
        if insensitive {
            key.to_str()
                .is_some_and(|key| key.eq_ignore_ascii_case(name))
        } else {
            key == OsStr::new(name)
        }
    })
}

fn terminal_variable(key: &OsStr) -> bool {
    let key = key.to_string_lossy();
    matches!(
        key.as_ref(),
        "USER"
            | "LOGNAME"
            | "SHELL"
            | "DBUS_SESSION_BUS_ADDRESS"
            | "DISPLAY"
            | "SSH_AUTH_SOCK"
            | "WAYLAND_DISPLAY"
            | "XAUTHORITY"
            | "XDG_RUNTIME_DIR"
            | "__CF_USER_TEXT_ENCODING"
            | "LANGUAGE"
    ) || key.starts_with("LC_")
        || cfg!(windows) && key.eq_ignore_ascii_case("USERNAME")
}

#[cfg(target_os = "macos")]
fn developer_directory(path: &Path) -> Option<std::path::PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let path = if path.join("Contents/Developer").is_dir() {
        path.join("Contents/Developer")
    } else {
        path.to_owned()
    };
    let path = path.canonicalize().ok()?;
    let xcode = path.file_name() == Some(OsStr::new("Developer"))
        && path.parent()?.file_name() == Some(OsStr::new("Contents"))
        && path.parent()?.parent()?.extension() == Some(OsStr::new("app"))
        && path.join("usr/bin/xcodebuild").is_file();
    let tools = path == Path::new("/Library/Developer/CommandLineTools")
        && path.join("usr/bin/clang").is_file();
    (xcode || tools).then_some(path)
}
