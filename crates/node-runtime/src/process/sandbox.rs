#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod policy;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod repositories;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod runtime;
#[cfg(test)]
mod tests;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod toolchains;

use super::{Environment, Launch};
use sailry_protocol::{Fault, process::Access};
use std::{path::Path, sync::Arc};

pub(crate) const SUPPORTED: bool = cfg!(any(target_os = "macos", target_os = "linux"));

pub(crate) struct Sandbox {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    policy: adk_sandbox::sandbox::SandboxPolicy,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    wrapper: std::path::PathBuf,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    variables: std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    links: toolchains::Links,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    protected: Vec<std::path::PathBuf>,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    path: Option<std::ffi::OsString>,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    home: Option<std::path::PathBuf>,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    profile: Option<std::path::PathBuf>,
    _scratch: Arc<tempfile::TempDir>,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    _state: tempfile::TempDir,
}

impl Sandbox {
    pub(crate) fn prepare(
        launch: &Launch,
        environment: &Environment,
        inputs: Option<&Path>,
    ) -> Result<Option<Self>, Fault> {
        if launch.access == Access::Unrestricted || !SUPPORTED {
            return Ok(None);
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let scratch = match &launch.scratch {
                Some(scratch) => scratch.clone(),
                None => Arc::new(
                    tempfile::Builder::new()
                        .prefix("sailry-command-")
                        .tempdir()
                        .map_err(runtime::failure)?,
                ),
            };
            let state = tempfile::Builder::new()
                .prefix("environment-")
                .tempdir_in(scratch.path())
                .map_err(runtime::failure)?;
            let (mut policy, variables, links) = policy::create(
                &launch.root,
                launch.profile.as_deref(),
                &scratch,
                state.path(),
                environment,
                inputs,
            )?;
            let home = environment.home().and_then(|path| path.canonicalize().ok());
            let profile = launch
                .profile
                .as_deref()
                .map(Path::canonicalize)
                .transpose()
                .map_err(runtime::failure)?;
            let protected = repositories::capture(
                &launch.root,
                &mut policy,
                home.as_deref(),
                profile.as_deref(),
            )?;
            Ok(Some(Self {
                policy,
                variables,
                links,
                protected,
                path: environment.path().map(std::ffi::OsStr::to_owned),
                home,
                profile,
                wrapper: runtime::wrapper()?,
                _scratch: scratch,
                _state: state,
            }))
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = (environment, inputs);
            Ok(None)
        }
    }

    pub(crate) fn command(
        &self,
        original: tokio::process::Command,
    ) -> Result<tokio::process::Command, Fault> {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let source = original.as_std();
            let args = source
                .get_args()
                .map(std::ffi::OsString::from)
                .collect::<Vec<_>>();
            let mut policy = self.policy.clone();
            let mut variables = self.variables.clone();
            let mut links = self.links.clone();
            let path = source
                .get_envs()
                .find(|(key, _)| *key == std::ffi::OsStr::new("PATH"))
                .and_then(|(_, value)| value);
            if path != self.path.as_deref() {
                links.extend(toolchains::paths(
                    &mut policy,
                    &mut variables,
                    path,
                    self.home.as_deref(),
                    self.profile.as_deref(),
                )?);
            }
            let wrapped = self.wrap(source.get_program(), &args, &policy, &links)?;
            let mut command = tokio::process::Command::new(wrapped.program);
            command.args(wrapped.args).env_clear();
            for (key, value) in source.get_envs() {
                // The dynamic loader runs before the wrapper applies confinement.
                if !loader_variable(key)
                    && let Some(value) = value
                {
                    command.env(key, value);
                }
            }
            command.envs(&variables);
            if let Some(directory) = source.get_current_dir() {
                command.current_dir(directory);
            }
            command
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            runtime::enforcer()
                .configure_command(&mut command, &policy)
                .map_err(runtime::failure)?;
            Ok(command)
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Ok(original)
        }
    }

    pub(crate) fn terminal(
        &self,
        command: &mut portable_pty::CommandBuilder,
        device: Option<&Path>,
    ) -> Result<(), Fault> {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            // A wrapper replaces argv[0], so explicitly retain login-shell behavior.
            let args = if command.get_argv().is_empty() {
                vec![std::ffi::OsString::from(command.get_shell()), "-l".into()]
            } else {
                command.get_argv().to_vec()
            };
            let wrapped = self.wrap(&args[0], &args[1..], &self.policy, &self.links)?;
            #[cfg(target_os = "macos")]
            let wrapped = {
                let mut wrapped = wrapped;
                macos::terminal(&mut wrapped, device)?;
                wrapped
            };
            #[cfg(not(target_os = "macos"))]
            let _ = device;
            // The dynamic loader runs before the wrapper applies confinement.
            let injections = command
                .iter_full_env_as_str()
                .filter(|(key, _)| loader_variable(std::ffi::OsStr::new(key)))
                .map(|(key, _)| key.to_owned())
                .collect::<Vec<_>>();
            for key in injections {
                command.env_remove(key);
            }
            *command.get_argv_mut() = std::iter::once(wrapped.program)
                .chain(wrapped.args)
                .collect();
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        let _ = (command, device);
        Ok(())
    }

    pub(crate) fn apply_terminal(&self, command: &mut portable_pty::CommandBuilder) {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        for (key, value) in &self.variables {
            command.env(key, value);
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        let _ = command;
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn wrap(
        &self,
        program: &std::ffi::OsStr,
        args: &[std::ffi::OsString],
        policy: &adk_sandbox::sandbox::SandboxPolicy,
        links: &toolchains::Links,
    ) -> Result<adk_sandbox::sandbox::WrappedCommand, Fault> {
        let mut wrapped = runtime::enforcer()
            .wrap_command(program, args, policy)
            .map_err(runtime::failure)?;
        wrapped.program = self.wrapper.clone().into_os_string();
        #[cfg(target_os = "linux")]
        linux::complete(&mut wrapped.args, program, links, &self.protected)?;
        #[cfg(target_os = "macos")]
        {
            let _ = links;
            macos::protect(&mut wrapped, &self.protected)?;
        }
        Ok(wrapped)
    }

    pub(crate) fn scratch(&self) -> &Path {
        self._scratch.path()
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn loader_variable(key: &std::ffi::OsStr) -> bool {
    let key = key.as_encoded_bytes();
    key.starts_with(b"LD_") || key.starts_with(b"DYLD_")
}
