//! Validate and rebuild only the existing private corpus owner.
use super::*;

pub(super) async fn records(db: &Connection, name: &str) -> Result<Vec<(String, Index)>, Fault> {
    let mut rows = db.query("SELECT key,revision,value IS NOT NULL,index_data FROM plugin_values WHERE name=?1 AND index_data IS NOT NULL ORDER BY key", [name]).await.map_err(storage_error)?;
    let mut records = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let revision: i64 = row.get(1).map_err(storage_error)?;
        let present: bool = row.get(2).map_err(storage_error)?;
        let bytes: Vec<u8> = row.get(3).map_err(storage_error)?;
        if !present || revision < 1 || bytes.len() > storage::MAX_INDEX_BYTES {
            return Err(storage_error("invalid stored plugin index metadata"));
        }
        let index: Index = serde_json::from_slice(&bytes).map_err(|error| {
            storage_error(format!(
                "invalid stored plugin index JSON, line {}, column {}",
                error.line(),
                error.column()
            ))
        })?;
        validate_index(&index)
            .map_err(|_| storage_error("invalid stored plugin index metadata"))?;
        records.push((row.get(0).map_err(storage_error)?, index));
    }
    Ok(records)
}

pub(super) async fn consistent(
    db: &Connection,
    table: &str,
    records: &[(String, Index)],
) -> Result<(), Fault> {
    let mut rows = db
        .query(
            format!("SELECT key,field0,field1,cjk0,cjk1,tags,ordinal FROM {table} ORDER BY key"),
            (),
        )
        .await
        .map_err(storage_error)?;
    let mut expected = records.iter();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let Some((key, index)) = expected.next() else {
            return Err(storage_error("plugin search index is inconsistent"));
        };
        let actual = (
            row.get::<String>(0).map_err(storage_error)?,
            row.get::<String>(1).map_err(storage_error)?,
            row.get::<String>(2).map_err(storage_error)?,
            row.get::<String>(3).map_err(storage_error)?,
            row.get::<String>(4).map_err(storage_error)?,
            row.get::<String>(5).map_err(storage_error)?,
            row.get::<i64>(6).map_err(storage_error)?,
        );
        let wanted = (
            key.clone(),
            crate::knowledge::text::field(&index.fields[0]),
            crate::knowledge::text::field(&index.fields[1]),
            crate::knowledge::text::index(&index.fields[0]),
            crate::knowledge::text::index(&index.fields[1]),
            serde_json::to_string(&index.tags).map_err(storage_error)?,
            index.order,
        );
        if actual != wanted {
            return Err(storage_error("plugin search index is inconsistent"));
        }
    }
    if expected.next().is_some() {
        return Err(storage_error("plugin search index is inconsistent"));
    }
    Ok(())
}

pub(in crate::store) async fn validate(db: &Connection) -> Result<(), Fault> {
    for (name, _) in catalog(db, true).await? {
        check(db, &name).await?;
    }
    Ok(())
}

pub(in crate::store) async fn apply(db: &Connection) -> Result<(), Fault> {
    for (name, indexed) in catalog(db, true).await? {
        let records = records(db, &name).await?;
        let table = table(&name);
        if !indexed && !exists(db, &table).await? {
            continue;
        }
        db.execute_batch(format!("DROP TABLE IF EXISTS {table}"))
            .await
            .map_err(storage_error)?;
        for ddl in definition(&name) {
            db.execute_batch(ddl).await.map_err(storage_error)?;
        }
        for (key, index) in records {
            replace(db, &name, &key, Some(&index)).await?;
        }
    }
    Ok(())
}
