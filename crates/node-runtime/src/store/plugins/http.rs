use super::super::database::Database;
use super::*;

pub(super) fn prepare(
    db: &Database,
    request: &sailry_protocol::Request,
    http: &plugin::http::Request,
) -> Result<crate::plugins::http::Input, Fault> {
    let context = request
        .plugin
        .as_ref()
        .ok_or_else(|| Fault::new(ErrorCode::PermissionDenied, "plugin provenance is required"))?;
    let info = if context.turn.is_some() {
        super::super::agent::plugin_package(&db.connection, context)?
    } else {
        settings::current(&db.connection, &context.package)?
    };
    let resolved = http
        .credential
        .as_ref()
        .map(|_| settings::resolve(&db.connection, &info))
        .transpose()?;
    crate::plugins::http::prepare(http, info.settings.as_ref(), resolved.as_ref())
}
