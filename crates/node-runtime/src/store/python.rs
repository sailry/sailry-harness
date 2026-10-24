//! Python settings share the existing durable configuration and completion owner.
use super::{
    Ingress, Job,
    database::{encode, storage_error},
    unavailable,
};
use sailry_link::Admission;
use sailry_protocol::{ErrorCode, Event, Fault, Output, Request, python::Settings};
use tokio::sync::oneshot;
use turso::Connection;

pub(super) async fn read(db: &Connection) -> Result<Settings, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM python_settings WHERE singleton=1")
            .await
            .map_err(storage_error)?;
        match statement.query_row(()).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let settings = body
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?
        .unwrap_or_default();
    Settings::validate(&settings)?;
    Ok(settings)
}

pub(super) async fn save(
    db: &Connection,
    request: &Request,
    settings: &Settings,
) -> Result<(Output, Option<Event>), Fault> {
    controller(request)?;
    settings.validate()?;
    super::commands::check_revision(read(db).await?.revision, settings.revision)?;
    let mut updated = settings.clone();
    updated.revision = updated
        .revision
        .checked_add(1)
        .filter(|revision| *revision <= i64::MAX as u64)
        .ok_or_else(|| Fault::invalid("Python settings revision is exhausted"))?;
    db.execute("INSERT INTO python_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [encode(&updated)?]).await
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

pub(super) fn subscribe(
    node: sailry_protocol::NodeId,
    progress: tokio::sync::watch::Receiver<Vec<sailry_protocol::python::Preparation>>,
    closed: sailry_link::CancellationToken,
) -> Box<dyn sailry_link::Subscription> {
    Box::new(Updates {
        node,
        progress,
        closed,
        initial: true,
    })
}

struct Updates {
    node: sailry_protocol::NodeId,
    progress: tokio::sync::watch::Receiver<Vec<sailry_protocol::python::Preparation>>,
    closed: sailry_link::CancellationToken,
    initial: bool,
}

impl sailry_link::Subscription for Updates {
    fn next(&mut self) -> sailry_link::Pending<'_, Result<sailry_protocol::Update, Fault>> {
        Box::pin(async move {
            if self.closed.is_cancelled() {
                return Err(unavailable());
            }
            if !self.initial {
                tokio::select! {
                    biased;
                    _ = self.closed.cancelled() => return Err(unavailable()),
                    changed = self.progress.changed() => changed.map_err(|_| unavailable())?,
                }
            }
            self.initial = false;
            Ok(sailry_protocol::Update::Python {
                node: self.node,
                preparations: self.progress.borrow_and_update().clone(),
            })
        })
    }
}
