//! Python settings share the existing durable configuration and completion owner.
use super::{
    Ingress, Job,
    database::{encode, storage_error},
    unavailable,
};
use rusqlite::{Connection, OptionalExtension};
use sailry_link::Admission;
use sailry_protocol::{ErrorCode, Event, Fault, Output, Request, python::Settings};
use tokio::sync::oneshot;

pub(super) fn read(db: &Connection) -> Result<Settings, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM python_settings WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let settings = body
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?
        .unwrap_or_default();
    Settings::validate(&settings)?;
    Ok(settings)
}

pub(super) fn save(
    db: &Connection,
    request: &Request,
    settings: &Settings,
) -> Result<(Output, Option<Event>), Fault> {
    controller(request)?;
    settings.validate()?;
    super::commands::check_revision(read(db)?.revision, settings.revision)?;
    let mut updated = settings.clone();
    updated.revision = updated
        .revision
        .checked_add(1)
        .filter(|revision| *revision <= i64::MAX as u64)
        .ok_or_else(|| Fault::invalid("Python settings revision is exhausted"))?;
    db.execute("INSERT INTO python_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [encode(&updated)?])
        .map_err(storage_error)?;
    Ok((
        Output::PythonSettings(updated.clone()),
        Some(Event::PythonSettingsChanged {
            revision: updated.revision,
        }),
    ))
}

pub(super) fn controller(request: &Request) -> Result<(), Fault> {
    if request.plugin.is_some() {
        Err(Fault::new(
            ErrorCode::PermissionDenied,
            "Python settings require a controller",
        ))
    } else {
        Ok(())
    }
}

impl Ingress {
    pub(crate) async fn python_settings(&self) -> Result<Settings, Fault> {
        let (reply, result) = oneshot::channel();
        self.send_internal(Job::PythonSettings { reply }).await?;
        result.await.map_err(|_| unavailable())?
    }
}

pub(super) fn observe(ingress: &Ingress, mut admission: Admission) -> Admission {
    let ingress = ingress.clone();
    let (reply, completion) = oneshot::channel();
    let original = std::mem::replace(&mut admission.completion, completion);
    tokio::spawn(async move {
        let result = match original.await {
            Ok(Ok(output @ Output::PythonSettings(_))) => match ingress.python_settings().await {
                Ok(settings) => ingress
                    .plugins
                    .configure_python(settings.index)
                    .map(|()| output),
                Err(error) => Err(error),
            },
            Ok(result) => result,
            Err(_) => Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "Python settings completion was not received",
            )),
        };
        let _ = reply.send(result);
    });
    admission
}
