//! Protected MCP slots bind to a concrete immutable server declaration.
use super::*;

pub(in crate::store::plugins) async fn current(
    db: &Connection,
    info: &Info,
    configured: Option<&plugin::mcp::Configuration>,
) -> Result<BTreeMap<String, String>, Fault> {
    let fields = info
        .settings
        .iter()
        .flat_map(|schema| &schema.properties)
        .filter_map(|(name, field)| Some((name, field.secret.as_ref()?.server.as_ref()?)))
        .collect::<Vec<_>>();
    if fields.is_empty() {
        return Ok(BTreeMap::new());
    }
    let declared = if let Some(configured) = configured {
        configured.clone()
    } else {
        let file: String = {
            let mut statement = db
                .prepare("SELECT file FROM pragma_database_list WHERE name='main'")
                .await
                .map_err(storage_error)?;
            let row = statement.query_row(()).await.map_err(storage_error)?;
            let mapped: turso::Result<_> = row.get(0);
            mapped.map_err(storage_error)?
        };
        let path = std::path::Path::new(&file);
        let profile = (!file.is_empty())
            .then(|| {
                path.parent()
                    .and_then(std::path::Path::parent)
                    .map(std::path::Path::to_owned)
            })
            .flatten();
        crate::plugins::Host::new(profile).mcp_configuration(
            &info.summary.reference(),
            sailry_link::CancellationToken::new(),
        )?
    };
    fields
        .into_iter()
        .map(|(field, server)| {
            let definition = declared.servers.get(server).ok_or_else(|| {
                Fault::new(
                    ErrorCode::NotConfigured,
                    "MCP credential target is unavailable",
                )
            })?;
            let bytes = encode(definition)?;
            Ok((field.clone(), blake3::hash(&bytes).to_hex().to_string()))
        })
        .collect()
}
