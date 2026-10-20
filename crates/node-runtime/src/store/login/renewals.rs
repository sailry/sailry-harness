//! Refresh is Node-owned maintenance: a stopped turn drops observation, not the token write.
use super::*;
use crate::store::{
    Ingress,
    database::{record, storage_error},
    providers::{authentication, quota},
};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub(super) struct Renewals {
    pending: BTreeMap<CredentialId, PendingGrant>,
}

#[derive(Clone)]
pub(crate) struct Authorization {
    pub(crate) grant: Grant,
    pub(crate) credential: Credential,
}

struct PendingGrant {
    expected: Credential,
    previous: Grant,
    waiting: Vec<oneshot::Sender<Result<Authorization, Fault>>>,
}

impl Worker {
    pub(in crate::store) fn authorize(
        &mut self,
        database: &mut Database,
        reference: CredentialRef,
        provider: ProviderId,
        authentication: Authentication,
        options: sailry_protocol::conversation::oauth::Options,
        reply: oneshot::Sender<Result<Authorization, Fault>>,
    ) {
        let plugin = if super::model_auth::managed(authentication) {
            let prepared = crate::store::agent::providers::get(&database.connection, provider)
                .and_then(|value| {
                    value.ok_or_else(|| Fault::new(ErrorCode::NotFound, "provider does not exist"))
                })
                .and_then(|value| {
                    super::model_auth::configuration(
                        &database.connection,
                        &value,
                        RequestId::new(),
                        Some(&options),
                    )
                });
            match prepared {
                Ok(value) => Some(value),
                Err(error) => {
                    let _ = reply.send(Err(error));
                    return;
                }
            }
        } else {
            None
        };
        let authorization = (|| {
            let (credential, secret) = database.credential(&reference, provider, authentication)?;
            let grant = Grant::decode(authentication, &secret)?;
            if authentication != Authentication::Plugin
                && super::model_auth::managed(authentication)
            {
                super::model_auth::check_binding(
                    &database.connection,
                    &credential,
                    &plugin
                        .as_ref()
                        .ok_or_else(|| {
                            Fault::new(ErrorCode::NotConfigured, "channel plugin is not captured")
                        })?
                        .target,
                )?;
            }
            if let Grant::Plugin { record, .. } = &grant {
                record.matches(
                    &plugin
                        .as_ref()
                        .ok_or_else(|| {
                            Fault::new(ErrorCode::NotConfigured, "channel plugin is not captured")
                        })?
                        .target,
                )?;
            }
            Ok::<_, Fault>(Authorization { grant, credential })
        })();
        let authorization = match authorization {
            Ok(value) => value,
            Err(error) => {
                let _ = reply.send(Err(error));
                return;
            }
        };
        if authorization.grant.expires_at_ms()
            <= SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|time| time.as_millis() as u64)
                .unwrap_or(u64::MAX)
                .saturating_add(60_000)
            && plugin
                .as_ref()
                .is_some_and(|plugin| !plugin.target.refreshable())
        {
            let _ = reply.send(Err(Fault::new(
                ErrorCode::NotConfigured,
                "model sign-in expired; sign in again",
            )));
            return;
        }
        let Some((id, grant)) = self
            .renewals
            .begin(database, authorization, reply, &self.events)
        else {
            return;
        };
        self.pending += 1;
        let service = self.service.clone();
        let plugins = self.plugins.clone();
        let stop = self.closed.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let operation = async {
                if let Some(plugin) = plugin
                    .as_ref()
                    .filter(|plugin| plugin.declaration.authentication.is_some())
                {
                    let bundle = plugin.bundle(&plugins, &stop).await?;
                    let mut fields = plugin.target.fields.clone();
                    fields.insert(
                        "user_agent".into(),
                        Secret::new(options.user_agent().into()),
                    );
                    crate::providers::login::plugin::flow::renew(
                        crate::providers::login::plugin::flow::Input {
                            service: service.clone(),
                            declaration: plugin.declaration.clone(),
                            fields,
                            redirect: None,
                            callback: None,
                            exchange: bundle,
                            context: None,
                        },
                        &grant,
                        stop.clone(),
                    )
                    .await
                } else if let (Grant::Plugin { record, .. }, Some(plugin)) = (&grant, plugin) {
                    let bundle = plugin.bundle(&plugins, &stop).await?;
                    let issued = crate::providers::login::plugin::flow::refresh(
                        &service,
                        &plugin.declaration,
                        &plugin.target.fields,
                        &record.issued,
                        bundle,
                        stop.clone(),
                    )
                    .await
                    .map_err(|error| match error.code {
                        ErrorCode::Unavailable
                        | ErrorCode::PermissionDenied
                        | ErrorCode::OutcomeUnknown => error,
                        _ => Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "authorization refresh did not complete; sign in again",
                        ),
                    })?;
                    let grant = Grant::Plugin {
                        record: Box::new(crate::providers::login::plugin::Record {
                            target: plugin.target,
                            issued,
                        }),
                        renewal_pending: false,
                        renewal_rejected: false,
                    };
                    grant.validate(Authentication::Plugin)?;
                    Ok(grant)
                } else {
                    service.refresh(&grant, &options, &stop).await
                }
            };
            let result = AssertUnwindSafe(operation)
                .catch_unwind()
                .await
                .unwrap_or_else(|_| Err(super::super::unavailable()));
            let _ = sender
                .send(Job::Login(Box::new(Progress::Renewed { id, result })))
                .await;
        });
    }
}

impl Renewals {
    fn begin(
        &mut self,
        database: &mut Database,
        authorization: Authorization,
        reply: oneshot::Sender<Result<Authorization, Fault>>,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Option<(CredentialId, Grant)> {
        let Authorization {
            credential: metadata,
            mut grant,
        } = authorization;
        let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(value) => value.as_millis() as u64,
            Err(_) => {
                let _ = reply.send(Err(Fault::new(
                    ErrorCode::Internal,
                    "system clock is invalid",
                )));
                return None;
            }
        };
        if let Some(pending) = self.pending.get_mut(&metadata.id) {
            if pending.expected != metadata {
                let _ = reply.send(Err(changed()));
            } else if pending.waiting.len() >= 64 {
                let _ = reply.send(Err(Fault::new(
                    ErrorCode::Busy,
                    "authorization observer capacity exhausted",
                )));
            } else {
                pending.waiting.push(reply);
            }
            return None;
        }
        if matches!(
            &grant,
            Grant::Plugin {
                renewal_rejected: true,
                ..
            }
        ) {
            let _ = reply.send(Err(Fault::new(
                ErrorCode::PermissionDenied,
                "authorization refresh was rejected; sign in again",
            )));
            return None;
        }
        if grant.renewal_pending() {
            let _ = reply.send(Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "authorization refresh did not complete; sign in again",
            )));
            return None;
        }
        if grant.expires_at_ms() > now.saturating_add(60_000) {
            let _ = reply.send(Ok(Authorization {
                grant,
                credential: metadata,
            }));
            return None;
        }
        if let Grant::Plugin { record, .. } = &grant
            && (record.issued.refresh.is_none() || !record.target.refreshable())
        {
            let _ = reply.send(Err(Fault::new(
                ErrorCode::NotConfigured,
                "model sign-in expired; sign in again",
            )));
            return None;
        }
        if self.pending.len() >= 4 {
            let _ = reply.send(Err(Fault::new(
                ErrorCode::Busy,
                "authorization refresh capacity exhausted",
            )));
            return None;
        }
        // A lost token-exchange result may consume a rotating grant. Mark the
        // existing credential before HTTP, so restart cannot replay that exchange.
        let previous = grant.clone();
        grant.begin_renewal();
        // Copilot's GET exchange does not rotate the GitHub grant; a later
        // generation may request it again after a failed read.
        let saved = if grant.renewal_pending() {
            grant
                .encode()
                .and_then(|secret| save(database, metadata, &secret, events))
        } else {
            Ok(metadata)
        };
        let expected = match saved {
            Ok(value) => value,
            Err(error) => {
                let _ = reply.send(Err(error));
                return None;
            }
        };
        let id = expected.id;
        self.pending.insert(
            id,
            PendingGrant {
                expected,
                previous,
                waiting: vec![reply],
            },
        );
        Some((id, grant))
    }

    pub(super) fn finish(
        &mut self,
        database: &mut Database,
        id: CredentialId,
        result: Result<Grant, Fault>,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> bool {
        let Some(pending) = self.pending.remove(&id) else {
            return false;
        };
        // A connection setup failure proves no exchange was delivered. A definite
        // rejection is retained without deleting the old token. Other failures
        // keep the persisted uncertain marker and cannot replay a rotating grant.
        let result = match result {
            Err(error)
                if matches!(
                    error.code,
                    ErrorCode::Unavailable | ErrorCode::PermissionDenied
                ) && matches!(&pending.previous, Grant::Plugin { .. }) =>
            {
                let mut previous = pending.previous.clone();
                if let Grant::Plugin {
                    renewal_pending,
                    renewal_rejected,
                    ..
                } = &mut previous
                {
                    *renewal_pending = false;
                    *renewal_rejected = error.code == ErrorCode::PermissionDenied;
                }
                if authentication::get(&database.connection, id)
                    .ok()
                    .flatten()
                    .as_ref()
                    == Some(&pending.expected)
                {
                    match previous.encode().and_then(|secret| {
                        save(database, pending.expected.clone(), &secret, events)
                    }) {
                        Ok(_) => Err(error),
                        Err(storage) => Err(storage),
                    }
                } else {
                    Err(changed())
                }
            }
            result => result,
        };
        let result = result.and_then(|grant| {
            if authentication::get(&database.connection, id)?.as_ref() != Some(&pending.expected) {
                return Err(changed());
            }
            let credential = save(database, pending.expected, &grant.encode()?, events)?;
            Ok(Authorization { grant, credential })
        });
        for reply in pending.waiting {
            let _ = reply.send(result.clone());
        }
        true
    }
}

impl Ingress {
    pub(crate) async fn authorization(
        &self,
        reference: CredentialRef,
        provider: ProviderId,
        authentication: Authentication,
        options: &sailry_protocol::conversation::oauth::Options,
        stop: &CancellationToken,
    ) -> Result<Authorization, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Job::Authorization {
                reference,
                provider,
                authentication,
                options: options.clone(),
                reply,
            })
            .map_err(|_| super::super::unavailable())?;
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err(super::super::unavailable()),
            _ = self.closed.cancelled() => Err(super::super::unavailable()),
            result = response => result.map_err(|_| super::super::unavailable())?,
        }
    }
}

fn save(
    database: &mut Database,
    credential: Credential,
    secret: &Secret,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Credential, Fault> {
    let node = database.node;
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let credential = authentication::put(&transaction, credential, secret)?;
    let changes = quota::reconcile(&transaction, node)?;
    let envelopes = record(&transaction, node, changes)?;
    transaction.commit().map_err(storage_error)?;
    for envelope in envelopes {
        let _ = events.send(envelope);
    }
    Ok(credential)
}

fn changed() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "credential changed during authorization refresh",
    )
}
