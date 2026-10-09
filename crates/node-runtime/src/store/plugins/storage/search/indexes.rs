//! Rebuild only derived indexes inside the startup owner's validated transaction.
use super::*;

fn records(db: &Connection, name: &str) -> Result<Vec<(i64, String, Index)>, Fault> {
    let mut statement = db
        .prepare("SELECT rowid,key,revision,value IS NOT NULL,index_data FROM plugin_values WHERE name=?1 AND index_data IS NOT NULL ORDER BY rowid")
        .map_err(storage_error)?;
    let mut rows = statement.query([name]).map_err(storage_error)?;
    let mut records = Vec::new();
    while let Some(row) = rows.next().map_err(storage_error)? {
        let id: i64 = row.get(0).map_err(storage_error)?;
        let revision: i64 = row.get(2).map_err(storage_error)?;
        let present: bool = row.get(3).map_err(storage_error)?;
        let bytes: Vec<u8> = row.get(4).map_err(storage_error)?;
        if !present || revision < 1 || bytes.len() > storage::MAX_INDEX_BYTES {
            return Err(Fault::new(
                ErrorCode::Internal,
                format!("invalid stored plugin index, record {id}; existing data was not changed"),
            ));
        }
        let index: Index = serde_json::from_slice(&bytes).map_err(|error| {
            Fault::new(ErrorCode::Internal, format!(
                "invalid stored JSON in plugin_values.index_data, record {id}, line {}, column {}; existing data was not changed",
                error.line(), error.column()
            ))
        })?;
        validate_index(&index).map_err(|_| {
            Fault::new(
                ErrorCode::Internal,
                format!("invalid stored plugin index, record {id}; existing data was not changed"),
            )
        })?;
        records.push((id, row.get(1).map_err(storage_error)?, index));
    }
    Ok(records)
}

pub(in crate::store) fn validate(db: &Connection) -> Result<(), Fault> {
    for (name, _) in catalog(db)? {
        let records = records(db, &name)?;
        if !check(db, &name)? {
            continue;
        }
        let table = table(&name);
        let mut statement = db
            .prepare(&format!("SELECT cjk0,cjk1 FROM {table} WHERE rowid=?1"))
            .map_err(storage_error)?;
        for (id, _, index) in records {
            let actual = statement
                .query_row([id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(storage_error)?;
            if actual.0 != crate::knowledge::text::index(&index.fields[0])
                || actual.1 != crate::knowledge::text::index(&index.fields[1])
            {
                return Err(storage_error("plugin keyword index is inconsistent"));
            }
        }
    }
    Ok(())
}

pub(in crate::store) fn apply(db: &Connection) -> Result<(), Fault> {
    for (name, indexed) in catalog(db)? {
        let records = records(db, &name)?;
        let table = table(&name);
        if !indexed && !exists(db, &table)? {
            continue;
        }
        db.execute_batch(&format!("DROP TABLE IF EXISTS {table}"))
            .map_err(storage_error)?;
        db.execute_batch(&definition(&name))
            .map_err(storage_error)?;
        for (_, key, index) in records {
            replace(db, &name, &key, Some(&index))?;
        }
    }
    Ok(())
}
