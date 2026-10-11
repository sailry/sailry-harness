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
    let mut input = crate::plugins::http::prepare(
        http,
        info.settings.as_ref(),
        resolved.as_ref(),
        &info.summary.reference(),
    )?;
    if let Some(field) = &http.credential {
        let stored = settings::stored(&db.connection, &info)?;
        if let Some(authorization) = stored.oauth
            && let Some(grant) = authorization.grant
            && grant.declaration.credentials.contains(field)
            && info
                .extension
                .as_ref()
                .and_then(|extension| extension.oauth.as_ref())
                == Some(&grant.declaration)
        {
            crate::plugins::oauth::validate(&grant.declaration, info.settings.as_ref())?;
            let binding = info
                .settings
                .as_ref()
                .and_then(|schema| schema.properties.get(field))
                .and_then(|field| field.secret.as_ref())
                .ok_or_else(|| Fault::invalid("OAuth credential field is unavailable"))?;
            let (_, header, prefix) = crate::plugins::http::binding(binding)?;
            input.oauth = Some(crate::plugins::oauth::Target {
                plugin: info.summary.name,
                id: authorization.id,
                declaration: grant.declaration,
                header,
                prefix: prefix.into(),
            });
        }
    }
    Ok(input)
}
