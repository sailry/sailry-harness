//! Profile-local consent shares the ordinary durable settings boundary.
use super::{
    Ingress,
    commands::check_revision,
    database::{encode, storage_error},
};
use sailry_link::Admission;
use sailry_protocol::{
    ErrorCode, Event, Fault, Output, Receipt, Request,
    telemetry::{Settings, Update},
};
use serde_json::Value;
use tokio::sync::oneshot;
use turso::Connection;

pub(super) fn controller(request: &Request) -> Result<(), Fault> {
    if request.plugin.is_some() {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "telemetry settings require a controller",
        ));
    }
    Ok(())
}

async fn body(db: &Connection) -> Result<Option<Vec<u8>>, Fault> {
    let mut statement = db
        .prepare("SELECT body FROM telemetry_settings WHERE singleton=1")
        .await
        .map_err(storage_error)?;
    match statement.query_row(()).await {
        Ok(row) => row.get(0).map(Some).map_err(storage_error),
        Err(turso::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(storage_error(error)),
    }
}

pub(super) async fn read(db: &Connection) -> Result<Settings, Fault> {
    let body = body(db)
        .await?
        .ok_or_else(|| Fault::new(ErrorCode::Internal, "telemetry settings are missing"))?;
    let settings: Settings = serde_json::from_slice(&body).map_err(storage_error)?;
    settings.validate()?;
    Ok(settings)
}

async fn exists(db: &Connection) -> Result<bool, Fault> {
    db.prepare("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='telemetry_settings')").await.map_err(storage_error)?
        .query_row(()).await.map_err(storage_error)?.get(0).map_err(storage_error)
}

pub(in crate::store) async fn pending_defaults(db: &Connection) -> Result<bool, Fault> {
    if !exists(db).await? {
        return Ok(false);
    }
    let Some(body) = body(db).await? else {
        return Ok(true);
    };
    let settings: Settings = serde_json::from_slice(&body).map_err(storage_error)?;
    settings.validate()?;
    let value: Value = serde_json::from_slice(&body).map_err(storage_error)?;
    Ok(["revision", "errors", "metrics", "installation_id"]
        .iter()
        .any(|field| value.get(field).is_none()))
}

pub(in crate::store) async fn initialize(db: &Connection) -> Result<(), Fault> {
    if !exists(db).await? || !pending_defaults(db).await? {
        return Ok(());
    }
    let settings: Settings = body(db)
        .await?
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?
        .unwrap_or_default();
    settings.validate()?;
    write(db, &settings).await
}

async fn write(db: &Connection, settings: &Settings) -> Result<(), Fault> {
    db.execute("INSERT INTO telemetry_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [encode(settings)?]).await.map_err(storage_error)?;
    Ok(())
}

pub(super) async fn save(
    db: &Connection,
    request: &Request,
    update: &Update,
) -> Result<(Output, Option<Event>), Fault> {
    controller(request)?;
    let mut settings = read(db).await?;
    check_revision(settings.revision, update.revision)?;
    settings.errors = update.errors;
    settings.metrics = update.metrics;
    publish(db, settings).await
}

pub(super) async fn reset(
    db: &Connection,
    request: &Request,
    expected_revision: u64,
) -> Result<(Output, Option<Event>), Fault> {
    controller(request)?;
    let mut settings = read(db).await?;
    check_revision(settings.revision, expected_revision)?;
    settings.installation_id = Settings::default().installation_id;
    publish(db, settings).await
}

async fn publish(
    db: &Connection,
    mut settings: Settings,
) -> Result<(Output, Option<Event>), Fault> {
    settings.revision = settings
        .revision
        .checked_add(1)
        .filter(|revision| *revision <= i64::MAX as u64)
        .ok_or_else(|| Fault::invalid("telemetry settings revision is exhausted"))?;
    write(db, &settings).await?;
    let event = Event::TelemetrySettingsChanged {
        revision: settings.revision,
    };
    Ok((Output::TelemetrySettings(settings), Some(event)))
}

impl Ingress {
    pub(super) async fn send_diagnostics(&self, request: Request) -> Result<Admission, Fault> {
        controller(&request)?;
        crate::telemetry::diagnostics().await?;
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(Ok(Output::DiagnosticsSent));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }
}

#[cfg(test)]
mod tests;
