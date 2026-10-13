use super::*;
use rusqlite::fallible_iterator::FallibleIterator;
use rusqlite::{
    Batch, Connection as Sqlite, OpenFlags,
    hooks::{AuthAction, AuthContext, Authorization},
    types::ValueRef,
};
use std::{path::Path, time::Instant};

pub(super) fn run(
    operation: &Operation,
    query: &Query,
    stop: CancellationToken,
    closed: CancellationToken,
) -> Result<ResultSet, Fault> {
    if stop.is_cancelled() || closed.is_cancelled() {
        return Err(unknown());
    }
    let Connection::Sqlite { path } = &operation.profile.connection else {
        unreachable!()
    };
    let requested = Path::new(path);
    let (path, create) = match requested.canonicalize() {
        Ok(path) => (path, false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if operation.profile.read_only || (operation.query.is_some() && !initializes(query)?) {
                return Err(missing());
            }
            (
                creation_path(requested, operation.storage.as_deref())?,
                true,
            )
        }
        Err(error) => return Err(crate::files::io_error(error)),
    };
    check_storage(&path, operation.storage.as_deref())?;
    let mut flags = if operation.profile.read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if create {
        flags |= OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    }
    if stop.is_cancelled() || closed.is_cancelled() {
        return Err(unknown());
    }
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

fn creation_path(requested: &Path, storage: Option<&Path>) -> Result<PathBuf, Fault> {
    if requested.components().any(|component| {
        matches!(
            component,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    }) {
        return Err(Fault::invalid("Database path must be normalized"));
    }
    let name = requested
        .file_name()
        .ok_or_else(|| Fault::invalid("Database filename is missing"))?;
    let mut relative = PathBuf::from(name);
    let mut parent = requested
        .parent()
        .ok_or_else(|| Fault::invalid("Database parent is missing"))?;
    let root = loop {
        match parent.canonicalize() {
            Ok(root) => break root,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = parent
                    .file_name()
                    .ok_or_else(|| Fault::invalid("Database parent is missing"))?;
                relative = PathBuf::from(name).join(relative);
                parent = parent
                    .parent()
                    .ok_or_else(|| Fault::invalid("Database parent is missing"))?;
            }
            Err(error) => return Err(crate::files::io_error(error)),
        }
    };
    let path = root.join(&relative);
    check_storage(&path, storage)?;
    let relative = relative
        .to_str()
        .ok_or_else(|| Fault::invalid("Database path must be UTF-8"))?;
    crate::files::path::create_parents(root.as_path(), relative)?;
    Ok(path)
}

fn check_storage(path: &Path, storage: Option<&Path>) -> Result<(), Fault> {
    if storage.is_some_and(|storage| path.starts_with(storage)) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "Node storage cannot be opened as a query database",
        ));
    }
    Ok(())
}

fn initializes(query: &Query) -> Result<bool, Fault> {
    let connection = Sqlite::open_in_memory().map_err(query_error)?;
    // Some PRAGMAs act during preparation, even under EXPLAIN. This guard belongs
    // only to the isolated classifier, not the actual execution connection.
    connection
        .authorizer(Some(|context: AuthContext<'_>| {
            if matches!(context.action, AuthAction::Pragma { .. }) {
                Authorization::Deny
            } else {
                Authorization::Allow
            }
        }))
        .map_err(query_error)?;
    let mut batch = Batch::new(&connection, &query.sql);
    let statement = batch
        .next()
        .map_err(preflight_error)?
        .ok_or_else(|| query_error("SQL statement is empty"))?;
    let write = !statement.readonly() && statement.is_explain() == 0;
    if batch.next().map_err(preflight_error)?.is_some() {
        return Err(query_error("Run one SQL statement at a time"));
    }
    Ok(write)
}
fn preflight_error(error: rusqlite::Error) -> Fault {
    if error.sqlite_error_code() == Some(rusqlite::ErrorCode::AuthorizationForStatementDenied) {
        missing()
    } else {
        query_error(error)
    }
}
fn missing() -> Fault {
    Fault::new(ErrorCode::NotFound, "Database file does not exist")
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
