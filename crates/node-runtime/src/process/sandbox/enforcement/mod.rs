//! Adapted from ADK-Rust 675c02b6c0df6d632b953a06a5a12d1f814b7780.
//! Copyright 2026 Zavora Technologies Ltd.; Apache-2.0, see third_party_licenses/adk-LICENSE.
//! Native policy enforcement for the Node's existing command service.
mod layering;
#[cfg(target_os = "linux")]
pub(super) mod linux;
#[cfg(target_os = "macos")]
pub(super) mod macos;
#[cfg(any(test, target_os = "linux"))]
mod seccomp;

use sailry_protocol::Fault;
use serde::{Deserialize, Serialize};
use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
};

/// Filesystem access mode for an allowed path.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum AccessMode {
    /// Read-only access.
    ReadOnly,
    /// Read and write access.
    ReadWrite,
}

/// A filesystem path entry with an access mode.
///
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AllowedPath {
    /// The filesystem path (directory or file).
    pub path: PathBuf,
    /// The access mode: read-only or read-write.
    pub mode: AccessMode,
}

/// A network access rule specifying an allowed domain and ports.
///
/// Declares intent for per-domain network filtering. **No enforcer can apply
/// it:** Seatbelt network filters accept only `*` or `localhost` as the host,
/// and bubblewrap has no domain-level control. Network access
/// is binary everywhere (all or nothing via `allow_network`), and rules present
/// with `allow_network = false` block all network access and log a
/// `tracing::warn`.
///
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NetworkRule {
    /// The domain name to allow (e.g., "api.openai.com").
    pub domain: String,
    /// The ports to allow on this domain. Empty means all ports.
    pub ports: Vec<u16>,
}

/// A declarative sandbox policy describing allowed operations.
///
/// Constructed via [`Builder`]. Defaults to deny-all when
/// no permissions are granted.
///
/// # Network Access
///
/// Network access has two levels of control:
///
/// 1. **Binary** (`allow_network`): When `true`, all network access is allowed.
///    When `false`, all network is blocked. Works on all platforms.
///
/// 2. **Domain allowlist** (`network_rules`): recorded for callers and
///    future enforcers, but **not enforced on any platform**. Non-empty
///    `network_rules` with `allow_network = false` block all network access —
///    the rules are ignored with a `tracing::warn`. This fails closed.
///
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Policy {
    /// Filesystem paths the process may access. Each path must exist; enforcers resolve
    /// them and fail with `PolicyViolation` otherwise.
    ///
    /// When entries nest, the most specific one decides, whatever their order (the nested-path policy). A `ReadOnly` entry inside a `ReadWrite` entry — `.git` inside a workspace —
    /// refuses every write, creation, removal, rename, and link beneath it on Seatbelt and
    /// bubblewrap alike, and the writable directories above it cannot be renamed or removed.
    /// For the same path, `ReadWrite` wins.
    pub allowed_paths: Vec<AllowedPath>,
    /// Whether the process may access the network (all domains/ports).
    pub allow_network: bool,
    /// Per-domain network allowlist. Not enforced on any platform: with
    /// `allow_network = false` these rules are ignored and all network access
    /// is blocked.
    #[serde(default)]
    pub network_rules: Vec<NetworkRule>,
    /// Whether the process may create child processes.
    ///
    /// When `false`, replacing the process image with `execve` remains allowed
    /// but creating a new process is refused:
    ///
    /// | Enforcer | Mechanism |
    /// |----------|-----------|
    /// | Seatbelt (macOS) | `process-fork` is not granted |
    /// | bubblewrap (Linux) | seccomp filter failing `fork`, `vfork`, and non-thread `clone` (x86-64 and AArch64) |
    ///
    /// A shell running more than one external command forks, so `sh -c` scripts
    /// usually need this enabled.
    pub allow_process_spawn: bool,
}

/// The result of wrapping a command with sandbox enforcement.
///
/// Contains the new program to execute and the full argument list
/// (sandbox wrapper args + original program + original args).
#[derive(Debug, Clone)]
pub(super) struct WrappedCommand {
    /// The program to execute ("sandbox-exec" or "bwrap").
    pub program: OsString,
    /// The full argument list including wrapper args, separator, and original args.
    pub args: Vec<OsString>,
}

/// Builder for constructing [`Policy`] values incrementally.
///
/// Defaults to deny-all: no allowed paths, no network, no process spawning,
/// and no process environment. The Node supplies its captured environment separately.
///
pub(super) struct Builder {
    policy: Policy,
}

impl Builder {
    /// Creates a new builder with deny-all defaults.
    pub(super) fn new() -> Self {
        Self {
            policy: Policy {
                allowed_paths: Vec::new(),
                allow_network: false,
                network_rules: Vec::new(),
                allow_process_spawn: false,
            },
        }
    }

    /// Adds a read-only allowed path.
    #[cfg(test)]
    pub(super) fn allow_read(mut self, path: impl Into<PathBuf>) -> Self {
        self.policy.allowed_paths.push(AllowedPath {
            path: path.into(),
            mode: AccessMode::ReadOnly,
        });
        self
    }

    /// Adds a read-write allowed path.
    pub(super) fn allow_read_write(mut self, path: impl Into<PathBuf>) -> Self {
        self.policy.allowed_paths.push(AllowedPath {
            path: path.into(),
            mode: AccessMode::ReadWrite,
        });
        self
    }

    /// Enables full network access (all domains, all ports).
    ///
    /// This overrides any domain-specific rules added via [`allow_domain`](Self::allow_domain).
    #[cfg(test)]
    pub(super) fn allow_network(mut self) -> Self {
        self.policy.allow_network = true;
        self
    }

    /// Records a domain and ports the process is intended to reach.
    ///
    /// Pass an empty slice for `ports` to mean all ports on the domain.
    ///
    /// **Platform support:** not enforced on any platform. Seatbelt network
    /// filters accept only `*` or `localhost` as the host, and bubblewrap and
    /// AppContainer have no domain-level control. If any rules are present but
    /// `allow_network` is false, all network access is blocked.
    ///
    #[cfg(test)]
    pub(super) fn allow_domain(mut self, domain: impl Into<String>, ports: &[u16]) -> Self {
        self.policy.network_rules.push(NetworkRule {
            domain: domain.into(),
            ports: ports.to_vec(),
        });
        self
    }

    /// Enables child process spawning.
    pub(super) fn allow_process_spawn(mut self) -> Self {
        self.policy.allow_process_spawn = true;
        self
    }

    /// Consumes the builder and returns the constructed [`Policy`].
    pub(super) fn build(self) -> Policy {
        self.policy
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

/// Translates a process policy into native command restrictions.
pub(super) trait Enforcer: Send + Sync {
    /// Wraps the original command with sandbox enforcement.
    ///
    /// Given the original program and its arguments, returns a [`WrappedCommand`]
    /// containing the sandbox wrapper program and the full argument list.
    ///
    /// This method:
    /// 1. Canonicalizes all paths in the policy (logs `tracing::warn` if changed)
    /// 2. Returns a service fault if any path cannot be resolved
    /// 3. Generates the platform-specific wrapper (Seatbelt profile, bwrap args, etc.)
    /// 4. Returns the wrapped program and args
    fn wrap_command(
        &self,
        program: &OsStr,
        args: &[OsString],
        policy: &Policy,
    ) -> Result<WrappedCommand, Fault>;

    /// Configures the Command with platform-specific process attributes.
    ///
    /// Must be called on the Command constructed from `wrap_command()` output,
    /// after its program and arguments are final. The default implementation is
    /// a no-op. Linux uses it to install the process-creation seccomp program
    /// that the wrapped arguments reference.
    fn configure_command(
        &self,
        _cmd: &mut tokio::process::Command,
        _policy: &Policy,
    ) -> Result<(), Fault> {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn failed(enforcer: &str, message: impl std::fmt::Display) -> Fault {
    super::runtime::failure(format!("enforcer '{enforcer}' failed: {message}"))
}

fn violation(message: impl std::fmt::Display) -> Fault {
    super::runtime::failure(format!("policy violation: {message}"))
}
