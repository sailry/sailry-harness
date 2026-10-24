//! Card status is a public classification of the canonical connection record.
use super::*;
use sailry_protocol::plugin::connection::{Authorization, Configuration, Credential, Status};
use sailry_protocol::plugin::connector::{Auth, Kind, State};

pub(in crate::store) async fn read(
    db: &Connection,
    profile: Option<&std::path::Path>,
    package: &plugin::Reference,
    card: &str,
) -> Result<Status, Fault> {
    let info = settings::current(db, package).await?;
    read_admitted(db, profile, &info, card).await
}
pub(in crate::store) async fn read_admitted(
    db: &Connection,
    profile: Option<&std::path::Path>,
    info: &Info,
    card: &str,
) -> Result<Status, Fault> {
    let declaration = info
        .extension
        .as_ref()
        .and_then(|extension| {
            extension
                .cards
                .iter()
                .find(|declaration| declaration.key == card)
        })
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "plugin card is unavailable"))?;
    let server = declaration
        .mcp_server
        .as_deref()
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "plugin card has no MCP server"))?;
    let connector = connectors::declarations(info)
        .into_iter()
        .find(|connector| {
            connector
                .auth
                .iter()
                .any(|auth| matches!(auth,Auth::McpOauth{server:named,..} if named==server))
        })
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "plugin card has no connector"))?;
    let record = connectors::records(db)
        .await?
        .into_iter()
        .find(|record| record.plugin == info.summary.name && record.connector == connector.id);
    let row = match record {
        Some(record) => Some(connectors::project(db, profile, &record).await?),
        None => None,
    };
    let ready = row
        .as_ref()
        .is_some_and(|row| row.enabled && row.state == State::Connected);
    let configuration = if row
        .as_ref()
        .is_some_and(|row| row.state == State::NeedsConfirmation)
    {
        Configuration::Changed
    } else {
        Configuration::Ready
    };
    let oauth = row.as_ref().map(|row| row.method).unwrap_or(Kind::McpOauth);
    let authorization = if matches!(oauth, Kind::Oauth2 | Kind::Application | Kind::McpOauth) {
        if ready {
            Authorization::Configured
        } else {
            Authorization::Missing
        }
    } else {
        Authorization::NotDeclared
    };
    Ok(Status {
        package: info.summary.reference(),
        card: card.into(),
        server: server.into(),
        configuration,
        authorization,
        credential: Credential::NotDeclared,
    })
}
