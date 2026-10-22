//! Safe diagnostics contain identities and classifications, never execution payloads.
use crate::{ErrorCode, RequestId, SessionId, TurnId, WorktreeId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemSource {
    PluginMetadata,
    PluginInitialization,
    GitInspection,
    PluginCallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticPhase {
    Metadata,
    Initialization,
    Inspection,
    Callback,
    Sdk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticReason {
    Failure,
    Timeout,
    Cancelled,
}

/// Native error prototypes are classified before reading any package-authored text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptException {
    Error,
    TypeError,
    RangeError,
    ReferenceError,
    SyntaxError,
    UriError,
    EvalError,
    AggregateError,
    ThrownValue,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticPlugin {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemDiagnostic {
    pub incident: RequestId,
    pub source: SystemSource,
    pub phase: DiagnosticPhase,
    pub reason: DiagnosticReason,
    pub code: ErrorCode,
    #[serde(default)]
    pub exception: Option<ScriptException>,
    pub plugin: Option<DiagnosticPlugin>,
    pub request: Option<RequestId>,
    pub worktree: Option<WorktreeId>,
    pub session: Option<SessionId>,
    pub turn: Option<TurnId>,
    /// A host-authored operation identifier, not arguments or an error message.
    pub operation: Option<String>,
    pub budget_ms: Option<u64>,
    pub elapsed_ms: Option<u64>,
    /// Version and platform of the failing execution, which may be a controller.
    pub version: String,
    pub platform: String,
}

impl SystemDiagnostic {
    pub fn new(incident: RequestId, source: SystemSource, code: ErrorCode) -> Self {
        Self {
            incident,
            source,
            code,
            exception: None,
            phase: match source {
                SystemSource::PluginMetadata => DiagnosticPhase::Metadata,
                SystemSource::PluginInitialization => DiagnosticPhase::Initialization,
                SystemSource::GitInspection => DiagnosticPhase::Inspection,
                SystemSource::PluginCallback => DiagnosticPhase::Callback,
            },
            reason: DiagnosticReason::Failure,
            plugin: None,
            request: None,
            worktree: None,
            session: None,
            turn: None,
            operation: None,
            budget_ms: None,
            elapsed_ms: None,
            version: env!("CARGO_PKG_VERSION").into(),
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        }
    }

    pub fn valid(&self) -> bool {
        fn token(value: &str, max: usize) -> bool {
            !value.is_empty()
                && value.len() <= max
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-+".contains(&byte))
        }
        token(&self.version, 64)
            && token(&self.platform, 64)
            && self
                .plugin
                .as_ref()
                .is_none_or(|plugin| token(&plugin.name, 256) && token(&plugin.version, 64))
            && self
                .operation
                .as_ref()
                .is_none_or(|value| token(value, 128))
            && self.budget_ms.is_none_or(|value| value <= 86_400_000)
            && self.elapsed_ms.is_none_or(|value| value <= 86_400_000)
    }
}
