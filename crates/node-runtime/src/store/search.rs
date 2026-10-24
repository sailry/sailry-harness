//! Private search credentials and public revision-checked settings share one Node owner.
use super::{
    Ingress, Job,
    database::{encode, storage_error},
    unavailable,
};
use crate::web::search::{Auth, Plan};
use sailry_link::Admission;
use sailry_protocol::{
    Command, ErrorCode, Event, Fault, NodeId, Output, Request, Secret,
    plugin::settings::SecretUpdate,
    search::{self, Driver, Selection},
};
use std::collections::BTreeMap;
use turso::Connection;

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct Stored {
    settings: search::Settings,
    credentials: BTreeMap<Driver, Auth>,
}

async fn load(db: &Connection) -> Result<Stored, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM search_settings WHERE singleton=1")
            .await
            .map_err(storage_error)?;
        match statement.query_row(()).await {
            Ok(row) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
    let stored = body
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?
        .unwrap_or_default();
    validate(&stored)?;
    Ok(stored)
}

pub(super) async fn read(db: &Connection) -> Result<search::Settings, Fault> {
    Ok(load(db).await?.settings)
}

pub(super) async fn plan(db: &Connection) -> Result<Plan, Fault> {
    let stored = load(db).await?;
    Ok(Plan {
        settings: stored.settings,
        credentials: stored.credentials,
    })
}

pub(super) async fn validate_stored(db: &Connection) -> Result<(), Fault> {
    load(db).await.map(|_| ())
}

fn validate(stored: &Stored) -> Result<(), Fault> {
    if stored.settings.providers.len() != Driver::ALL.len()
        || stored.settings.revision > i64::MAX as u64
    {
        return Err(Fault::invalid("invalid search settings"));
    }
    for driver in Driver::ALL {
        let config = stored
            .settings
            .providers
            .get(&driver)
            .ok_or_else(|| Fault::invalid("search driver configuration is missing"))?;
        let auth = stored.credentials.get(&driver).cloned().unwrap_or_default();
        if config.key_configured != auth.key.is_some()
            || config.basic_configured != auth.basic.is_some()
        {
            return Err(Fault::invalid(
                "search credentials do not match their configuration",
            ));
        }
        if driver != Driver::Searxng && (config.endpoint.is_some() || auth.basic.is_some()) {
            return Err(Fault::invalid(
                "custom search endpoints and Basic authentication require SearXNG",
            ));
        }
        if let Some(endpoint) = &config.endpoint {
            let url =
                url::Url::parse(endpoint).map_err(|_| Fault::invalid("invalid search endpoint"))?;
            if endpoint.len() > 4096
                || !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(Fault::invalid("invalid search endpoint"));
            }
        }
        for secret in [&auth.key, &auth.basic].into_iter().flatten() {
            if secret.expose().trim().is_empty()
                || secret.expose().len() > 4096
                || secret.expose().chars().any(char::is_control)
            {
                return Err(Fault::invalid("invalid search credential"));
            }
        }
        if auth.basic.as_ref().is_some_and(|secret| {
            secret
                .expose()
                .split_once(':')
                .is_none_or(|(user, _)| user.is_empty())
        }) {
            return Err(Fault::invalid(
                "Basic authentication requires username:password",
            ));
        }
    }
    Ok(())
}

pub(super) async fn save(
    db: &Connection,
    request: &Request,
    update: &search::Update,
) -> Result<(Output, Option<Event>), Fault> {
    if request
        .plugin
        .as_ref()
        .is_some_and(|context| !context.controller_settings())
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "search settings require a controller",
        ));
    }
    let mut stored = load(db).await?;
    if stored.settings.revision != update.settings.revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "search settings changed",
        ));
    }
    stored.settings = update.settings.clone();
    stored.settings.revision = stored
        .settings
        .revision
        .checked_add(1)
        .filter(|revision| *revision <= i64::MAX as u64)
        .ok_or_else(|| Fault::invalid("search revision is exhausted"))?;
    for (driver, changes) in &update.credentials {
        let auth = stored.credentials.entry(*driver).or_default();
        replace(&mut auth.key, &changes.key);
        replace(&mut auth.basic, &changes.basic);
    }
    for (driver, config) in &mut stored.settings.providers {
        let auth = stored.credentials.get(driver).cloned().unwrap_or_default();
        config.key_configured = auth.key.is_some();
        config.basic_configured = auth.basic.is_some();
    }
    validate(&stored)?;
    db.execute("INSERT INTO search_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [encode(&stored)?]).await.map_err(storage_error)?;
    Ok((
        Output::SearchSettings(stored.settings.clone()),
        Some(Event::SearchSettingsChanged {
            revision: stored.settings.revision,
        }),
    ))
}

fn replace(slot: &mut Option<Secret>, update: &SecretUpdate) {
    match update {
        SecretUpdate::Keep => {}
        SecretUpdate::Replace(secret) => *slot = Some(secret.clone()),
        SecretUpdate::Clear => *slot = None,
    }
}

struct Operation<'a> {
    controls: &'a crate::agent::Controls,
    turn: sailry_protocol::TurnId,
}
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        self.controls.finish_operation(self.turn);
    }
}

impl Ingress {
    pub(super) async fn search_web(
        &self,
        caller: NodeId,
        request: Request,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let output = match &request.command {
            Command::SearchWeb(input) => {
                let turn = request.plugin.as_ref().and_then(|context| context.turn);
                let (_operation, stop) = if let Some(turn) = turn {
                    let stop = self.agents.begin_operation(turn).ok_or_else(|| {
                        Fault::new(ErrorCode::Cancelled, "web search turn is no longer running")
                    })?;
                    (
                        Some(Operation {
                            controls: &self.agents,
                            turn,
                        }),
                        stop,
                    )
                } else {
                    (None, self.closed.clone())
                };
                Output::SearchPage(
                    self.search
                        .search(caller, request.id, self.search_plan().await?, input, &stop)
                        .await?,
                )
            }
            Command::TestSearchDriver { driver } => {
                if request
                    .plugin
                    .as_ref()
                    .is_some_and(|context| !context.controller_settings())
                {
                    return Err(Fault::new(
                        ErrorCode::PermissionDenied,
                        "search driver tests require a controller",
                    ));
                }
                let mut plan = self.search_plan().await?;
                plan.settings.driver = Some(Selection::Driver(*driver));
                plan.settings.fallback = false;
                plan.settings
                    .providers
                    .get_mut(driver)
                    .expect("saved search driver exists")
                    .enabled = true;
                let input = search::Request {
                    query: "Sailry".into(),
                    limit: 1,
                };
                Output::SearchPage(
                    self.search
                        .search(caller, request.id, plan, &input, &self.closed)
                        .await?,
                )
            }
            Command::CancelWebSearch { request } => {
                self.search.cancel(caller, *request);
                Output::WebSearchCancelled { request: *request }
            }
            _ => unreachable!("search command expected"),
        };
        let (reply, completion) = tokio::sync::oneshot::channel();
        let _ = reply.send(Ok(output));
        Ok(Admission {
            receipt: sailry_protocol::Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }

    async fn search_plan(&self) -> Result<Plan, Fault> {
        let (reply, response) = tokio::sync::oneshot::channel();
        self.sender
            .try_send(Job::SearchPlan { reply })
            .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }
}
