//! Exact durable admission retains a digest without duplicating credentials.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(in crate::store) struct Prepared {
    pub(in crate::store) command: Command,
    digest: String,
}

impl Prepared {
    pub(in crate::store) fn new(command: &Command) -> Result<Option<Self>, Fault> {
        if !matches!(
            command,
            Command::SaveDatabase { .. }
                | Command::InstallOcrModel { .. }
                | Command::DeleteOcrModel { .. }
                | Command::SaveSsh { .. }
                | Command::PutRole { .. }
                | Command::RemoveRole { .. }
                | Command::SetSessionRoles { .. }
        ) {
            return Ok(None);
        }
        let digest = blake3::hash(&encode(command)?).to_hex().to_string();
        let mut command = command.clone();
        match &mut command {
            Command::SaveDatabase { password, .. } => *password = None,
            Command::SaveSsh { credential, .. } => *credential = None,
            _ => {}
        }
        Ok(Some(Self { command, digest }))
    }

    pub(in crate::store) fn matches(&self, command: &Command) -> Result<bool, Fault> {
        Ok(self.digest == blake3::hash(&encode(command)?).to_hex().as_str())
    }
}
