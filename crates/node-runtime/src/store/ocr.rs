//! Controller model configuration uses the Node's ordinary durable ledger.
use super::{
    Ingress, Job,
    database::{Database, encode, storage_error},
    unavailable,
};
use rusqlite::{Connection, OptionalExtension};
use sailry_link::{Admission, CancellationToken};
use sailry_protocol::{
    Command, Event, EventEnvelope, Fault, Output, Request, models::Definition, ocr::Settings,
};
use std::sync::Arc;
use tokio::sync::{broadcast, oneshot};
pub(in crate::store) mod approvals;

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Catalog {
    pub settings: Settings,
    pub records: Vec<Definition>,
}

pub(super) fn read(db: &Connection) -> Result<Catalog, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM ocr_settings WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let catalog: Catalog = body
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?
        .unwrap_or_default();
    catalog.settings.validate()?;
    let definitions = crate::ocr::catalog::definitions(&catalog.records)?;
    if catalog
        .settings
        .model
        .as_ref()
        .is_some_and(|id| !definitions.contains_key(id))
    {
        return Err(Fault::invalid("OCR model is not registered"));
    }
    Ok(catalog)
}

pub(super) fn execute(
    db: &Connection,
    request: &Request,
) -> Result<(Output, Option<Event>), Fault> {
    if !matches!(request.command, Command::RegisterOcrModel { .. }) {
        controller(request)?;
    }
    let mut catalog = read(db)?;
    let (output, event) = match &request.command {
        Command::ReadOcrSettings => return Ok((Output::OcrSettings(catalog.settings), None)),
        Command::SaveOcrSettings(settings) => {
            settings.validate()?;
            super::commands::check_revision(catalog.settings.revision, settings.revision)?;
            if settings.model.as_ref().is_some_and(|id| {
                !crate::ocr::catalog::definitions(&catalog.records)
                    .is_ok_and(|definitions| definitions.contains_key(id))
            }) {
                return Err(Fault::invalid("OCR model is not registered"));
            }
            catalog.settings = settings.clone();
            catalog.settings.revision = settings
                .revision
                .checked_add(1)
                .filter(|revision| *revision <= i64::MAX as u64)
                .ok_or_else(|| Fault::invalid("OCR settings revision is exhausted"))?;
            (
                Output::OcrSettings(catalog.settings.clone()),
                Event::OcrSettingsChanged {
                    revision: catalog.settings.revision,
                },
            )
        }
        Command::RegisterOcrModel { definition } => {
            definition.validate()?;
            if definition.format.kind() != sailry_protocol::models::Kind::Ocr {
                return Err(Fault::invalid("OCR model format required"));
            }
            let id = crate::ocr::catalog::id(definition);
            if !crate::ocr::catalog::definitions(&catalog.records)?.contains_key(&id) {
                catalog.records.push(definition.clone());
            }
            crate::ocr::catalog::definitions(&catalog.records)?;
            (
                Output::OcrModel { id },
                Event::OcrSettingsChanged {
                    revision: catalog.settings.revision,
                },
            )
        }
        _ => return Err(Fault::invalid("OCR configuration command expected")),
    };
    db.execute("INSERT INTO ocr_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [encode(&catalog)?]).map_err(storage_error)?;
    Ok((output, Some(event)))
}

pub(super) fn models(
    database: &mut Database,
    models: Vec<sailry_protocol::ocr::Model>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let envelopes = super::database::record(
        &transaction,
        database.node,
        vec![Event::OcrModelsChanged { models }],
    )?;
    transaction.commit().map_err(storage_error)?;
    for envelope in envelopes {
        let _ = events.send(envelope);
    }
    Ok(())
}

pub(super) async fn start(ingress: &Arc<Ingress>) -> Result<(), Fault> {
    let catalog = ingress.ocr_catalog().await?;
    ingress.ocr.configure(catalog.settings, &catalog.records)?;
    if ingress.profile.is_some() {
        ingress.ocr.models().await?;
    }
    let mut updates = ingress.ocr.subscribe();
    let weak = Arc::downgrade(ingress);
    let stop = ingress.closed.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! { _ = stop.cancelled() => break, result = updates.changed() => if result.is_err() { break } }
            let Some(ingress) = weak.upgrade() else { break };
            let models = updates.borrow_and_update().clone();
            let (reply, result) = oneshot::channel();
            if ingress
                .send_internal(Job::OcrModels { models, reply })
                .await
                .is_err()
                || result.await.is_err()
            {
                break;
            }
            if let Some(knowledge) = ingress.knowledge.get() {
                knowledge.refresh();
            }
        }
    });
    Ok(())
}

impl Ingress {
    pub(super) async fn ocr_catalog(&self) -> Result<Catalog, Fault> {
        let (reply, result) = oneshot::channel();
        self.send_internal(Job::OcrCatalog { reply }).await?;
        result.await.map_err(|_| unavailable())?
    }

    pub(super) async fn read_ocr(&self, request: Request) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                sailry_protocol::ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        if !matches!(request.command, Command::ReadOcrModels) {
            controller(&request)?;
        }
        let output = match &request.command {
            Command::ReadOcrModels => Output::OcrModels(self.ocr.models().await?),
            Command::CancelOcrModel { id } => {
                self.ocr.cancel(id);
                Output::OcrModel { id: id.clone() }
            }
            _ => return Err(Fault::invalid("OCR read expected")),
        };
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(Ok(output));
        Ok(Admission {
            receipt: sailry_protocol::Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }

    pub(super) async fn change_ocr(
        &self,
        request: &Request,
        stop: &CancellationToken,
        reporter: Option<&crate::installation::Reporter>,
    ) -> Result<Output, Fault> {
        if !matches!(request.command, Command::InstallOcrModel { .. }) {
            if request.plugin.is_some() {
                self.check_plugin(self.node, request.clone()).await?;
            } else {
                controller(request)?;
            }
        }
        let catalog = self.ocr_catalog().await?;
        self.ocr.configure(catalog.settings, &catalog.records)?;
        let id = match &request.command {
            Command::InstallOcrModel { id } => {
                let mirror = self.knowledge_catalog().await?.settings.mirror;
                self.ocr
                    .install(id, mirror.as_deref(), stop, reporter)
                    .await?;
                id
            }
            Command::DeleteOcrModel { id } => {
                self.ocr.remove(id).await?;
                id
            }
            _ => return Err(Fault::invalid("OCR installation command expected")),
        };
        Ok(Output::OcrModel { id: id.clone() })
    }
}

fn controller(request: &Request) -> Result<(), Fault> {
    if request.plugin.is_some() {
        Err(Fault::new(
            sailry_protocol::ErrorCode::PermissionDenied,
            "OCR settings require a controller",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn observe(ingress: &Ingress, mut admission: Admission) -> Admission {
    let ingress = ingress.clone();
    let (reply, completion) = oneshot::channel();
    let original = std::mem::replace(&mut admission.completion, completion);
    tokio::spawn(async move {
        if let Ok(mut result) = original.await {
            if result.is_ok() {
                match ingress.ocr_catalog().await {
                    Ok(catalog) => {
                        if let Err(error) =
                            ingress.ocr.configure(catalog.settings, &catalog.records)
                        {
                            result = Err(error);
                        }
                    }
                    Err(error) => result = Err(error),
                }
                if let Some(knowledge) = ingress.knowledge.get() {
                    knowledge.refresh();
                }
            }
            let _ = reply.send(result);
        }
    });
    admission
}
