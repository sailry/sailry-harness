use super::*;

pub(super) async fn read(
    operation: &Operation,
    database: Option<String>,
    stop: &CancellationToken,
    closed: &CancellationToken,
) -> Result<Outcome, Fault> {
    let engine = operation.profile.connection.engine();
    let sql = match (engine, database.as_deref()) {
        (Engine::Sqlite, None) => "SELECT 'main'".into(),
        (Engine::Mysql, None) => "SHOW DATABASES".into(),
        (Engine::Postgres, None) => "SELECT datname FROM pg_database WHERE datallowconn AND NOT datistemplate ORDER BY datname".into(),
        (Engine::Sqlite, Some(_)) => "SELECT 'main', name FROM sqlite_schema WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' ORDER BY name".into(),
        (Engine::Mysql, Some(_)) => "SELECT TABLE_SCHEMA, TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() ORDER BY TABLE_NAME".into(),
        (Engine::Postgres, Some(_)) => "SELECT table_schema, table_name FROM information_schema.tables WHERE table_schema NOT IN ('pg_catalog','information_schema') ORDER BY table_schema,table_name".into(),
    };
    let mut profile = operation.profile.clone();
    profile.read_only = true;
    let operation = Operation {
        profile,
        password: operation.password.clone(),
        ssh: operation.ssh.clone(),
        storage: operation.storage.clone(),
        browse: None,
        query: Some(Query {
            sql,
            row_limit: 10_000,
            timeout_ms: 30_000,
        }),
    };
    let Outcome::Query(result) = Box::pin(execute(operation, stop, closed)).await? else {
        unreachable!()
    };
    if result.truncated {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "database catalog exceeds the result limit",
        ));
    }
    let text = |value: &Value| match value {
        Value::Text(value) => Ok(value.clone()),
        _ => Err(Fault::new(
            ErrorCode::Internal,
            "database catalog returned an invalid name",
        )),
    };
    Ok(Outcome::Catalog(if let Some(database) = database {
        Catalog::Tables {
            database,
            tables: result
                .rows
                .iter()
                .map(|row| {
                    if row.len() != 2 {
                        return Err(Fault::new(ErrorCode::Internal, "invalid table catalog row"));
                    }
                    Ok(Table {
                        schema: text(&row[0])?,
                        name: text(&row[1])?,
                    })
                })
                .collect::<Result<_, Fault>>()?,
        }
    } else {
        Catalog::Databases(
            result
                .rows
                .iter()
                .map(|row| {
                    row.first()
                        .ok_or_else(|| {
                            Fault::new(ErrorCode::Internal, "invalid database catalog row")
                        })
                        .and_then(text)
                })
                .collect::<Result<_, _>>()?,
        )
    }))
}
