//! Reports use one stable durable identity, including after a reconnect.
use crate::*;
use sailry_protocol::notification::SystemDiagnostic;

impl Client {
    pub async fn report_diagnostic(&self, report: SystemDiagnostic) -> Result<(), Fault> {
        let id = report.incident;
        let mut request = self.prepare(Command::ReportDiagnostic(report));
        request.id = id;
        // Notification persistence must not invalidate unrelated shared Git reads.
        match execute(&*self.transport, request).await? {
            Output::NotificationsUpdated | Output::Notification(_) => Ok(()),
            _ => Err(Fault::new(
                ErrorCode::Internal,
                "diagnostic response expected",
            )),
        }
    }
}
