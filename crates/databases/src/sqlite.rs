use super::*;
use futures::TryStreamExt;
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection, SqliteRow};
use sqlx::{
    Column, ConnectOptions as _, Connection as _, Either, Executor, Row, SqlSafeStr, Statement,
    TypeInfo, ValueRef,
};
use std::{path::Path, time::Instant};

mod statement;

pub(super) async fn run(
    operation: &Operation,
    query: &Query,
    stop: &CancellationToken,
    closed: &CancellationToken,
) -> Result<ResultSet, Fault> {
    let Connection::Sqlite { path } = &operation.profile.connection else {
        unreachable!()
    };
    let deadline = Instant::now() + Duration::from_millis(query.timeout_ms);
    let requested = Path::new(path);
    let (path, create) = match requested.canonicalize() {
        Ok(path) => (path, false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if operation.profile.read_only
                || (operation.query.is_some() && !initializes(query).await?)
            {
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
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(operation.profile.read_only)
        .create_if_missing(create)
        .no_follow(create)
        .foreign_keys(false)
        .busy_timeout(Duration::from_millis(query.timeout_ms.min(2000)))
        .disable_statement_logging();
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .map_err(query_error)?;
    {
        let stop = stop.clone();
        let closed = closed.clone();
        connection
            .lock_handle()
            .await
            .map_err(query_error)?
            .set_progress_handler(1000, move || {
                !stop.is_cancelled() && !closed.is_cancelled() && Instant::now() < deadline
            });
    }
    if statement::inspect(&mut connection, &query.sql, false).await? && operation.profile.read_only
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "Database profile is read-only",
        ));
    }
    // The command is the user's complete SQL statement, admitted under its captured permissions.
    let sql = sqlx::AssertSqlSafe(query.sql.clone()).into_sql_str();
    let prepared = connection.prepare(sql.clone()).await.map_err(query_error)?;
    let mut result = Rows::new(
        prepared
            .columns()
            .iter()
            .map(|column| column.name().into())
            .collect(),
        query.row_limit,
    )?;
    let mut stream = connection.fetch_many(sql);
    while let Some(message) = stream.try_next().await.map_err(execution_error)? {
        match message {
            Either::Left(done) if result.result.columns.is_empty() => {
                result.result.affected_rows = done.rows_affected()
            }
            Either::Right(row) => result.push(
                (0..row.len())
                    .map(|index| value(&row, index))
                    .collect::<Result<Vec<_>, _>>()?,
            )?,
            _ => {}
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

async fn initializes(query: &Query) -> Result<bool, Fault> {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .foreign_keys(false)
            .disable_statement_logging(),
    )
    .await
    .map_err(query_error)?;
    statement::inspect(&mut connection, &query.sql, true).await
}
fn missing() -> Fault {
    Fault::new(ErrorCode::NotFound, "Database file does not exist")
}
fn value(row: &SqliteRow, index: usize) -> Result<Value, Fault> {
    let raw = row.try_get_raw(index).map_err(query_error)?;
    if raw.is_null() {
        return Ok(Value::Null);
    }
    Ok(match raw.type_info().name() {
        "INTEGER" => Value::Integer(row.try_get_unchecked(index).map_err(query_error)?),
        "REAL" => Value::Real(
            row.try_get_unchecked::<f64, _>(index)
                .map_err(query_error)?
                .to_string(),
        ),
        "BLOB" => Value::Blob(row.try_get_unchecked(index).map_err(query_error)?),
        _ => {
            let bytes: Vec<u8> = row.try_get_unchecked(index).map_err(query_error)?;
            Value::Text(String::from_utf8_lossy(&bytes).into())
        }
    })
}
