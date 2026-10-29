//! Only explicit pre-GDK storage markers authorize rebuilding the Node database.
use crate::Error;
use std::collections::BTreeSet;
use turso::Connection;

#[cfg(test)]
mod tests;

const CHECKS: [(&str, &str); 3] = [
    (
        "agent_events",
        "SELECT EXISTS(SELECT 1 FROM agent_events WHERE json_type(body,'$.kind') IS NULL)",
    ),
    (
        "search_settings",
        "SELECT EXISTS(SELECT 1 FROM search_settings WHERE json_type(body,'$.settings.prefer_native') IS NOT NULL)",
    ),
    (
        "roles",
        "SELECT EXISTS(SELECT 1 FROM roles r,json_each(r.body,'$.tools') t WHERE json_extract(t.value,'$.kind')='model')",
    ),
];

// These are the two indexes written by the retired pre-GDK history owner.
const INDEXES: [&str; 2] = [
    "CREATE INDEX agent_usage ON agent_events(
        turn,
        max(0,json_extract(body,'$.usage_metadata.prompt_token_count')),
        max(0,json_extract(body,'$.usage_metadata.candidates_token_count')),
        max(0,coalesce(json_extract(body,'$.usage_metadata.cache_read_input_token_count'),0)),
        max(0,coalesce(json_extract(body,'$.usage_metadata.thinking_token_count'),0))
    ) WHERE json_type(body,'$.usage_metadata')='object'",
    "CREATE INDEX agent_usage_time ON agent_events(
        substr(json_extract(body,'$.timestamp'),1,19),
        sequence,
        turn,
        json_extract(body,'$.timestamp'),
        max(0,json_extract(body,'$.usage_metadata.prompt_token_count')),
        max(0,json_extract(body,'$.usage_metadata.candidates_token_count')),
        max(0,coalesce(json_extract(body,'$.usage_metadata.cache_read_input_token_count'),0)),
        max(0,coalesce(json_extract(body,'$.usage_metadata.thinking_token_count'),0))
    ) WHERE json_type(body,'$.usage_metadata')='object'",
];

pub(super) async fn detected(connection: &Connection) -> Result<bool, Error> {
    let mut tables = BTreeSet::new();
    let mut rows = connection
        .query(
            "SELECT type,name,tbl_name,sql FROM sqlite_schema WHERE name IN ('agent_events','search_settings','roles','agent_usage','agent_usage_time')",
            (),
        )
        .await?;
    while let Some(row) = rows.next().await? {
        let kind = row.get::<String>(0)?;
        let name = row.get::<String>(1)?;
        if kind == "table" {
            tables.insert(name);
        } else if kind == "index"
            && matches!(name.as_str(), "agent_usage" | "agent_usage_time")
            && row.get::<String>(2)? == "agent_events"
            && row.get::<Option<String>>(3)?.is_some_and(|sql| {
                let sql = super::definition::normalize(&sql);
                INDEXES
                    .iter()
                    .any(|known| sql == super::definition::normalize(known))
            })
        {
            return Ok(true);
        }
    }
    drop(rows);
    for (table, query) in CHECKS {
        if tables.contains(table)
            && connection
                .prepare(query)
                .await?
                .query_row(())
                .await?
                .get::<bool>(0)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}
