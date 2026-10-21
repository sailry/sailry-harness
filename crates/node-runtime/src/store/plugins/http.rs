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
    let mut input = crate::plugins::http::prepare(http, info.settings.as_ref(), resolved.as_ref())?;
    if let Some(connector) = &http.connector {
        if http.credential.is_some() {
            return Err(Fault::invalid(
                "HTTP request cannot mix credential and connector references",
            ));
        }
        let (record, declaration) = super::connectors::resolve(
            &db.connection,
            db.profile.as_deref(),
            context,
            connector,
            http.connection,
        )?;
        let origin = url::Url::parse(&http.url)
            .map_err(|_| Fault::invalid("invalid plugin HTTP URL"))?
            .origin()
            .ascii_serialization();
        let injections = declaration
            .inject
            .iter()
            .filter(|injection| injection.origin.as_deref() == Some(&origin))
            .cloned()
            .collect::<Vec<_>>();
        if injections.is_empty() {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "connector does not authorize this HTTP origin",
            ));
        }
        input.connector = Some((
            super::connectors::target(&record, declaration, info.summary.reference()),
            injections,
            matches!(http.method.as_str(), "GET" | "HEAD") || http.idempotent,
        ));
    } else if http.connection.is_some() {
        return Err(Fault::invalid("connection requires a connector reference"));
    }
    Ok(input)
}
