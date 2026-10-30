//! Settings belong to this Client's profile owner, independently of other Nodes.
use crate::Client;
use sailry_protocol::{Command, ErrorCode, Fault, Output, telemetry};

impl Client {
    pub async fn telemetry_settings(&self) -> Result<telemetry::Settings, Fault> {
        let request = self.prepare(Command::ReadTelemetrySettings);
        match self.execute(request).await? {
            Output::TelemetrySettings(settings) => Ok(settings),
            _ => Err(unexpected()),
        }
    }

    pub async fn save_telemetry(
        &self,
        update: telemetry::Update,
    ) -> Result<telemetry::Settings, Fault> {
        let request = self.prepare(Command::SaveTelemetrySettings(update));
        match self.execute(request).await? {
            Output::TelemetrySettings(settings) => Ok(settings),
            _ => Err(unexpected()),
        }
    }

    pub async fn reset_telemetry(
        &self,
        expected_revision: u64,
    ) -> Result<telemetry::Settings, Fault> {
        let request = self.prepare(Command::ResetTelemetryIdentity { expected_revision });
        match self.execute(request).await? {
            Output::TelemetrySettings(settings) => Ok(settings),
            _ => Err(unexpected()),
        }
    }

    pub async fn send_diagnostics(&self) -> Result<(), Fault> {
        let request = self.prepare(Command::SendDiagnostics);
        match self.execute(request).await? {
            Output::DiagnosticsSent => Ok(()),
            _ => Err(unexpected()),
        }
    }
}

fn unexpected() -> Fault {
    Fault::new(ErrorCode::Internal, "unexpected telemetry response")
}
