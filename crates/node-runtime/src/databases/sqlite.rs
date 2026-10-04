use super::*;
use rusqlite::fallible_iterator::FallibleIterator;
use rusqlite::{Batch, Connection as Sqlite, OpenFlags, types::ValueRef};
use std::{path::Path, time::Instant};

pub(super) fn run(
    operation: &Operation,
    query: &Query,
    stop: CancellationToken,
    closed: CancellationToken,
) -> Result<ResultSet, Fault> {
    let Connection::Sqlite { path } = &operation.profile.connection else {
        unreachable!()
    };
    let path = Path::new(path)
        .canonicalize()
        .map_err(crate::files::io_error)?;
    if operation
        .storage
        .as_ref()
        .is_some_and(|storage| path.starts_with(storage))
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "Node storage cannot be opened as a query database",
        ));
    }
    let flags = if operation.profile.read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Sqlite::open_with_flags(path, flags).map_err(query_error)?;
    connection
        .busy_timeout(Duration::from_millis(query.timeout_ms.min(2000)))
        .map_err(query_error)?;
    let deadline = Instant::now() + Duration::from_millis(query.timeout_ms);
    if stop.is_cancelled() || closed.is_cancelled() {
        return Err(unknown());
    }
    connection
        .progress_handler(
            1000,
            Some(move || {
                stop.is_cancelled() || closed.is_cancelled() || Instant::now() >= deadline
            }),
        )
        .map_err(query_error)?;
    let mut batch = Batch::new(&connection, &query.sql);
    let mut statement = batch
        .next()
        .map_err(query_error)?
        .ok_or_else(|| query_error("SQL statement is empty"))?;
    if batch.next().map_err(query_error)?.is_some() {
        return Err(query_error("Run one SQL statement at a time"));
    }
    if operation.profile.read_only && !statement.readonly() {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "Database profile is read-only",
        ));
    }
    let columns = statement
        .column_names()
        .iter()
        .map(|name| (*name).into())
        .collect::<Vec<_>>();
    let mut result = Rows::new(columns, query.row_limit)?;
    if result.result.columns.is_empty() {
        result.result.affected_rows = statement.execute([]).map_err(execution_error)? as u64;
    } else {
        let count = result.result.columns.len();
        let mut cursor = statement.query([]).map_err(execution_error)?;
        while let Some(row) = cursor.next().map_err(execution_error)? {
            let values = (0..count)
                .map(|index| row.get_ref(index).map(value).map_err(execution_error))
                .collect::<Result<Vec<_>, _>>()?;
            result.push(values)?;
        }
    }
    Ok(result.result)
}
fn value(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => Value::Integer(value),
        ValueRef::Real(value) => Value::Real(value.to_string()),
        ValueRef::Text(value) => Value::Text(String::from_utf8_lossy(value).into()),
        ValueRef::Blob(value) => Value::Blob(value.into()),
    }
}
fn execution_error(error: rusqlite::Error) -> Fault {
    if error.sqlite_error_code() == Some(rusqlite::ErrorCode::OperationInterrupted) {
        unknown()
    } else {
        query_error(error)
    }
}
