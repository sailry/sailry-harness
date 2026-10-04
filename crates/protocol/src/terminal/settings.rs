// Based on Sailry Code 67ae9fa0 terminal_settings.dart; values stay out of public snapshots.
use crate::{ErrorCode, Fault};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub revision: u64,
    pub shell: String,
    pub environment: BTreeMap<String, String>,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Settings")
            .field("revision", &self.revision)
            .field("shell", &self.shell)
            .field("environment", &self.environment.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), Fault> {
        if self.shell.len() > 4096 || self.shell.chars().any(char::is_control) {
            return Err(invalid("invalid terminal shell path"));
        }
        let mut size = 0;
        for (key, value) in &self.environment {
            validate_key(key)?;
            if value.contains('\0') {
                return Err(invalid("terminal environment value contains a null byte"));
            }
            size += key.len() + value.len();
        }
        if size > 64 * 1024 {
            return Err(invalid("terminal environment exceeds the supported size"));
        }
        Ok(())
    }
}

pub fn validate_key(key: &str) -> Result<(), Fault> {
    let upper = key.to_uppercase();
    if key.is_empty()
        || key != key.trim()
        || key.contains('=')
        || key.chars().any(char::is_control)
        || ["SAILRY_", "DMUX_", "LD_", "DYLD_"]
            .iter()
            .any(|prefix| upper.starts_with(prefix))
        || upper.ends_with("_PRELOAD")
        || [
            "TERM",
            "TERM_PROGRAM",
            "TERM_PROGRAM_VERSION",
            "COLORTERM",
            "COLORFGBG",
            "COLOR_SCHEME",
            "PWD",
            "VTE_VERSION",
            "BASH_ENV",
            "ENV",
            "ZDOTDIR",
            "IFS",
            "NODE_OPTIONS",
            "PERL5OPT",
            "PYTHONSTARTUP",
        ]
        .contains(&upper.as_str())
    {
        return Err(invalid("invalid or reserved terminal environment name"));
    }
    Ok(())
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}
