//! Refresh is Node-owned maintenance: a stopped turn drops observation, not the token write.
use super::*;
use crate::store::{Ingress, providers::authentication};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub(super) struct Renewals {
    pending: BTreeMap<CredentialId, PendingGrant>,
}

struct PendingGrant {
    expected: Credential,
    waiting: Vec<oneshot::Sender<Result<Grant, Fault>>>,
}

impl Worker {
    pub(in crate::store) fn authorize(
        &mut self,
        database: &mut Database,
        reference: CredentialRef,
        provider: ProviderId,
        authentication: Authentication,
        reply: oneshot::Sender<Result<Grant, Fault>>,
    ) {
        let Some((id, grant)) =
            self.renewals
                .begin(database, reference, provider, authentication, reply)
        else {
            return;
        };
        self.pending += 1;
        let service = self.service.clone();
        let stop = self.closed.clone();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let result = AssertUnwindSafe(service.refresh(&grant, &stop))
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
        reference: CredentialRef,
        provider: ProviderId,
        authentication: Authentication,
        reply: oneshot::Sender<Result<Grant, Fault>>,
    ) -> Option<(CredentialId, Grant)> {
        let prepared = (|| {
            let (metadata, secret) = database.credential(&reference, provider, authentication)?;
            let grant = Grant::decode(authentication, &secret)?;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| Fault::new(ErrorCode::Internal, "system clock is invalid"))?
                .as_millis() as u64;
            Ok::<_, Fault>((metadata, grant, now))
        })();
        let (metadata, mut grant, now) = match prepared {
            Ok(value) => value,
            Err(error) => {
                let _ = reply.send(Err(error));
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
        if grant.renewal_pending() {
            let _ = reply.send(Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "authorization refresh did not complete; sign in again",
            )));
            return None;
        }
        if grant.expires_at_ms() > now.saturating_add(60_000) {
            let _ = reply.send(Ok(grant));
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
        grant.begin_renewal();
        // Copilot's GET exchange does not rotate the GitHub grant; a later
        // generation may request it again after a failed read.
        let saved = if grant.renewal_pending() {
            grant
                .encode()
                .and_then(|secret| authentication::put(&database.connection, metadata, &secret))
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
                waiting: vec![reply],
            },
        );
        Some((id, grant))
    }

    pub(super) fn finish(
        &mut self,
        database: &Database,
        id: CredentialId,
        result: Result<Grant, Fault>,
    ) -> bool {
        let Some(pending) = self.pending.remove(&id) else {
            return false;
        };
        let result = result.and_then(|grant| {
            if authentication::get(&database.connection, id)?.as_ref() != Some(&pending.expected) {
                return Err(changed());
            }
            authentication::put(&database.connection, pending.expected, &grant.encode()?)?;
            Ok(grant)
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
        stop: &CancellationToken,
    ) -> Result<Grant, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Job::Authorization {
                reference,
                provider,
                authentication,
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

fn changed() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "credential changed during authorization refresh",
    )
}
