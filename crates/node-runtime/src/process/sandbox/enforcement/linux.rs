//! Adapted from ADK-Rust 675c02b6c0df6d632b953a06a5a12d1f814b7780.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
//! Linux bubblewrap sandbox enforcer.
//!
//! Uses `bwrap` with user namespaces to create isolated filesystem and
//! process environments without requiring root privileges.
//!
//! ## How It Works
//!
//! 1. Bubblewrap arguments are generated from the [`Policy`]
//! 2. The original command is wrapped: `bwrap <args> -- <program> <args...>`
//! 3. The kernel enforces namespace-based isolation on the child process
//!
//! ## Key bwrap Arguments
//!
//! - `--die-with-parent` — kill child when parent exits
//! - `--unshare-pid` — isolate process ID namespace
//! - `--unshare-net` — isolate network namespace (no network access)
//! - `--ro-bind <src> <dest>` — read-only filesystem bind mount
//! - `--bind <src> <dest>` — read-write filesystem bind mount
//! - `--new-session` — always: a new session with no controlling terminal, so the child
//!   cannot inject input into the caller's terminal with `TIOCSTI`
//! - `--seccomp <fd>` — seccomp program that denies process creation
//!
//! ## Nested Paths
//!
//! bubblewrap shows the last mount made over a path, so the policy's paths are mounted
//! shallowest first and the most specific entry decides (the nested-path policy): a read-only entry
//! inside a read-write entry is mounted after it and stays read-only. Each writable directory
//! above such an entry is also bound onto itself, which makes it a mount point the kernel
//! refuses to rename or remove, so the entry cannot be carried away from its path and the
//! path recreated writable.
//!
//! ## Process Spawning
//!
//! When `allow_process_spawn` is `false`, the wrapped program runs under a seccomp filter
//! that fails `fork`, `vfork`, and non-thread `clone` with `EPERM` and `clone3` with
//! `ENOSYS`. Threads and `execve` keep working. The filter is generated for x86-64 and
//! AArch64; on any other architecture [`Enforcer::configure_command`] returns
//! `EnforcerFailed` rather than running without it.
//!
//! The filter reaches bubblewrap as file descriptor [`SECCOMP_FD`], which
//! [`Enforcer::configure_command`] installs on the command. Running the output of
//! [`Bubblewrap::arguments`] without that call makes bubblewrap exit with an error.

use std::ffi::{OsStr, OsString};

use super::layering::Layering;
use super::seccomp;
use super::{AccessMode, AllowedPath, Enforcer, Policy, WrappedCommand};
use super::{failed, violation};
use sailry_protocol::Fault;

/// File descriptor bubblewrap reads the process-creation seccomp program from.
///
/// High enough to stay clear of stdio and the descriptors the standard library uses while
/// spawning, and below every default `RLIMIT_NOFILE`.
pub const SECCOMP_FD: i32 = 64;

/// Linux bubblewrap sandbox enforcer.
///
/// Wraps child processes with `bwrap` arguments to enforce namespace-based
/// filesystem, network, and process isolation.
///
pub struct Bubblewrap;

impl Bubblewrap {
    /// Creates a new Linux bubblewrap enforcer.
    pub fn new() -> Self {
        Self
    }

    /// Generates bubblewrap arguments from the policy.
    ///
    /// Always starts with `--die-with-parent` and `--unshare-pid`, and always passes
    /// `--new-session`. Bind mounts follow, shallowest path first, with a read-write bind
    /// of each writable directory above a nested read-only entry (see
    /// [Nested Paths](self#nested-paths)).
    /// The returned arguments do NOT include the `--` separator or the
    /// original program/args — those are appended by `wrap_command`.
    ///
    #[cfg(test)]
    pub fn arguments(policy: &Policy) -> Vec<String> {
        Self::arguments_from_paths(
            &policy.allowed_paths,
            policy.allow_network,
            policy.allow_process_spawn,
        )
    }

    /// Internal: generates args from pre-canonicalized paths.
    fn arguments_from_paths(
        paths: &[AllowedPath],
        allow_network: bool,
        allow_process_spawn: bool,
    ) -> Vec<String> {
        let mut args = Vec::with_capacity(16);

        // Always: terminate child when parent exits
        args.push("--die-with-parent".to_string());

        // Always: isolate PID namespace
        args.push("--unshare-pid".to_string());

        // Network isolation
        if !allow_network {
            args.push("--unshare-net".to_string());
        }

        // Always: a new session with no controlling terminal, so the child cannot inject
        // input into the caller's terminal with `TIOCSTI` (spec 3.1 §10 S2).
        args.push("--new-session".to_string());

        // Process spawn restriction: the seccomp program is what denies fork and clone.
        if !allow_process_spawn {
            args.push("--seccomp".to_string());
            args.push(SECCOMP_FD.to_string());
        }

        // Filesystem bind mounts, shallowest first: a later mount covers an earlier one, so the
        // most specific entry decides. Pinned directories become mount points, which the
        // kernel refuses to rename or remove.
        for (path, mode) in Layering::new(paths).mounts() {
            let path_str = path.to_string_lossy().to_string();
            match mode {
                AccessMode::ReadOnly => {
                    args.push("--ro-bind".to_string());
                    args.push(path_str.clone());
                    args.push(path_str);
                }
                AccessMode::ReadWrite => {
                    args.push("--bind".to_string());
                    args.push(path_str.clone());
                    args.push(path_str);
                }
            }
        }

        args
    }
}

impl Default for Bubblewrap {
    fn default() -> Self {
        Self::new()
    }
}

impl Enforcer for Bubblewrap {
    fn wrap_command(
        &self,
        program: &OsStr,
        args: &[OsString],
        policy: &Policy,
    ) -> Result<WrappedCommand, Fault> {
        // Warn if domain-level network rules are present — bubblewrap can't enforce them
        if !policy.allow_network && !policy.network_rules.is_empty() {
            tracing::warn!(
                rules_count = policy.network_rules.len(),
                "bubblewrap does not support per-domain network filtering; \
                 network_rules will be ignored and all network access will be blocked"
            );
        }

        // 1. Canonicalize all paths in the policy
        let canonicalized_paths = canonicalize_paths(&policy.allowed_paths)?;

        // 2. Generate bwrap args from canonicalized paths
        let bwrap_args = Self::arguments_from_paths(
            &canonicalized_paths,
            policy.allow_network,
            policy.allow_process_spawn,
        );

        // 3. Build the wrapped command: bwrap <args> -- <program> <original_args...>
        let mut wrapped_args: Vec<OsString> = bwrap_args.into_iter().map(OsString::from).collect();
        wrapped_args.push(OsString::from("--"));
        wrapped_args.push(program.to_owned());
        wrapped_args.extend_from_slice(args);

        Ok(WrappedCommand {
            program: OsString::from("bwrap"),
            args: wrapped_args,
        })
    }

    /// Installs the process-creation seccomp program as file descriptor [`SECCOMP_FD`].
    ///
    /// # Errors
    ///
    /// Returns `EnforcerFailed` when the policy denies process spawning on an architecture
    /// without a generated filter, or when the descriptor cannot be prepared.
    fn configure_command(
        &self,
        cmd: &mut tokio::process::Command,
        policy: &Policy,
    ) -> Result<(), Fault> {
        use std::io::Write;
        use std::os::fd::{AsRawFd, OwnedFd};

        if policy.allow_process_spawn {
            return Ok(());
        }

        let abi = seccomp::Abi::native().ok_or_else(|| {
            failed(
                "bubblewrap",
                format!(
                    "no process-creation seccomp filter exists for {}; allow process spawning in \
                 the policy or run on x86_64 or aarch64",
                    std::env::consts::ARCH
                ),
            )
        })?;
        let program = seccomp::encode(&seccomp::deny_process_creation(abi));

        let failed = |error: std::io::Error| {
            failed(
                "bubblewrap",
                format!("failed to prepare the seccomp program descriptor: {error}"),
            )
        };
        // The program is a few hundred bytes, well inside a pipe buffer, so the write cannot
        // block, and closing the writer gives bubblewrap its end-of-file.
        let (reader, mut writer) = std::io::pipe().map_err(failed)?;
        writer.write_all(&program).map_err(failed)?;
        drop(writer);
        let program_fd = OwnedFd::from(reader);

        // SAFETY: the closure runs in the forked child before exec and calls only dup2(2) and
        // fcntl(2), which are async-signal-safe. `program_fd` is owned by the closure, so the
        // descriptor stays open until the command is dropped.
        unsafe {
            cmd.pre_exec(move || {
                let source = program_fd.as_raw_fd();
                let result = if source == SECCOMP_FD {
                    // dup2 onto itself leaves FD_CLOEXEC set, so clear the flag directly.
                    libc::fcntl(source, libc::F_SETFD, 0)
                } else {
                    libc::dup2(source, SECCOMP_FD)
                };
                if result == -1 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        Ok(())
    }
}

/// Canonicalizes all paths in the policy, logging warnings for changed paths.
fn canonicalize_paths(paths: &[AllowedPath]) -> Result<Vec<AllowedPath>, Fault> {
    let mut result = Vec::with_capacity(paths.len());

    for entry in paths {
        let canonical = std::fs::canonicalize(&entry.path).map_err(|e| {
            violation(format!(
                "failed to canonicalize allowed path '{}': {e}",
                entry.path.display()
            ))
        })?;

        if canonical != entry.path {
            tracing::warn!(
                original = %entry.path.display(),
                resolved = %canonical.display(),
                "allowed path resolved to a different location (possible symlink)"
            );
        }

        result.push(AllowedPath {
            path: canonical,
            mode: entry.mode,
        });
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::Builder;
    use super::*;

    #[test]
    fn deny_all() {
        let policy = Builder::new().build();
        let args = Bubblewrap::arguments(&policy);

        assert_eq!(args[0], "--die-with-parent");
        assert_eq!(args[1], "--unshare-pid");
        assert!(args.contains(&"--unshare-net".to_string()));
        assert!(args.contains(&"--new-session".to_string()));
        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "--seccomp" && pair[1] == SECCOMP_FD.to_string())
        );
        // No bind mounts
        assert!(!args.contains(&"--ro-bind".to_string()));
        assert!(!args.contains(&"--bind".to_string()));
    }

    #[test]
    fn read_only_path() {
        let policy = Builder::new().allow_read("/usr/lib").build();
        let args = Bubblewrap::arguments(&policy);

        let ro_idx = args.iter().position(|a| a == "--ro-bind").unwrap();
        assert_eq!(args[ro_idx + 1], "/usr/lib");
        assert_eq!(args[ro_idx + 2], "/usr/lib");
        assert!(!args.contains(&"--bind".to_string()));
    }

    #[test]
    fn read_write_path() {
        let policy = Builder::new().allow_read_write("/tmp/work").build();
        let args = Bubblewrap::arguments(&policy);

        let bind_idx = args.iter().position(|a| a == "--bind").unwrap();
        assert_eq!(args[bind_idx + 1], "/tmp/work");
        assert_eq!(args[bind_idx + 2], "/tmp/work");
        assert!(!args.contains(&"--ro-bind".to_string()));
    }

    #[test]
    fn network_allowed() {
        let policy = Builder::new().allow_network().build();
        let args = Bubblewrap::arguments(&policy);

        assert!(!args.contains(&"--unshare-net".to_string()));
    }

    #[test]
    fn network_denied() {
        let policy = Builder::new().build();
        let args = Bubblewrap::arguments(&policy);

        assert!(args.contains(&"--unshare-net".to_string()));
    }

    #[test]
    fn process_spawn_allowed() {
        let policy = Builder::new().allow_process_spawn().build();
        let args = Bubblewrap::arguments(&policy);

        assert!(!args.contains(&"--seccomp".to_string()));
    }

    /// A shell is the case that allows process spawning, and it is the one that can reach a
    /// controlling terminal, so the new session cannot depend on the seccomp filter.
    #[test]
    fn every_policy_starts_a_new_session() {
        for allow_process_spawn in [false, true] {
            for allow_network in [false, true] {
                let args =
                    Bubblewrap::arguments_from_paths(&[], allow_network, allow_process_spawn);
                assert_eq!(
                    args.iter().filter(|arg| *arg == "--new-session").count(),
                    1,
                    "spawn={allow_process_spawn} network={allow_network}: {args:?}"
                );
            }
        }
    }

    #[test]
    fn process_spawn_denied() {
        let policy = Builder::new().build();
        let args = Bubblewrap::arguments(&policy);

        assert!(args.contains(&"--new-session".to_string()));
        let seccomp = args
            .iter()
            .position(|arg| arg == "--seccomp")
            .expect("--seccomp present");
        assert_eq!(args[seccomp + 1], SECCOMP_FD.to_string());
    }

    #[test]
    fn preserves_spawn_configuration() {
        let policy = Builder::new().allow_process_spawn().build();
        let mut cmd = tokio::process::Command::new("true");

        Bubblewrap::new()
            .configure_command(&mut cmd, &policy)
            .expect("nothing to configure");
    }

    #[test]
    fn installs_supported_process_filter() {
        let policy = Builder::new().build();
        let mut cmd = tokio::process::Command::new("true");

        let result = Bubblewrap::new().configure_command(&mut cmd, &policy);
        if seccomp::Abi::native().is_some() {
            result.expect("the filter is prepared on a supported architecture");
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.code, sailry_protocol::ErrorCode::Unavailable);
            assert!(
                error.message.contains("enforcer 'bubblewrap' failed:"),
                "{error:?}"
            );
        }
    }

    #[test]
    fn starts_with_die_with_parent() {
        let policy = Builder::new()
            .allow_read("/tmp")
            .allow_network()
            .allow_process_spawn()
            .build();
        let args = Bubblewrap::arguments(&policy);

        assert_eq!(args[0], "--die-with-parent");
    }

    #[test]
    fn no_empty_strings() {
        let policy = Builder::new()
            .allow_read("/usr/lib")
            .allow_read_write("/tmp")
            .allow_network()
            .build();
        let args = Bubblewrap::arguments(&policy);

        for arg in &args {
            assert!(!arg.is_empty(), "found empty string in bwrap args");
        }
    }

    #[test]
    fn rejects_missing_paths() {
        let enforcer = Bubblewrap::new();
        let policy = Builder::new()
            .allow_read("/nonexistent/path/that/does/not/exist")
            .build();

        let result = enforcer.wrap_command(OsStr::new("echo"), &[], &policy);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.code == sailry_protocol::ErrorCode::Unavailable
                && err.message.contains("policy violation:"),
            "expected a policy failure, got: {err:?}"
        );
    }
}
