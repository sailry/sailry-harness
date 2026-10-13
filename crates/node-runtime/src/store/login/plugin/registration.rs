//! Application registration shares account attempts, cancellation and durable receipt.
use super::*;

impl Worker {
    pub(super) fn register_plugin(
        &mut self,
        database: &mut Database,
        caller: NodeId,
        request: &Request,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Response {
        let Command::BeginPluginRegistration { package } = &request.command else {
            unreachable!()
        };
        let prepared = (|| {
            if self.closed.is_cancelled() {
                return Err(super::super::super::unavailable());
            }
            let (info, declaration) =
                crate::store::plugins::registration::current(&database.connection, package)?;
            let baseline = crate::store::plugins::registration::Baseline::capture(
                &database.connection,
                &info,
                &declaration,
            )?;
            if self.plugin.values().filter(|entry| !entry.finished).count() >= 4
                || self.plugin.values().any(|entry| {
                    let update = entry.updates.borrow();
                    !entry.finished
                        && update.state.active()
                        && update.attempt.package.name == package.name
                })
            {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "plugin account authorization is already active or capacity is exhausted",
                ));
            }
            Ok((declaration, baseline))
        })();
        let (declaration, baseline) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return database.finish_external(caller, request, Err(error), events),
        };
        let attempt = Attempt {
            id: request.id,
            package: package.clone(),
        };
        let output = database.finish_external(
            caller,
            request,
            Ok(Output::PluginLogin(attempt.clone())),
            events,
        )?;
        while self.plugin.len() >= 32 {
            let Some(key) = self
                .plugin
                .iter()
                .find(|(_, entry)| entry.finished)
                .map(|(key, _)| *key)
            else {
                break;
            };
            self.plugin.remove(&key);
        }
        let (updates, _) = watch::channel(LoginUpdate {
            attempt,
            revision: 1,
            state: State::Starting,
        });
        let stop = self.closed.child_token();
        self.plugin.insert(
            (caller, request.id),
            Entry {
                updates,
                callback: None,
                stop: stop.clone(),
                finished: false,
            },
        );
        self.pending += 1;
        let sender = self.sender.clone();
        let service = self.registration.clone();
        let id = request.id;
        self.runtime.spawn(async move {
            let progress = sender.clone();
            let operation = service.authorize(declaration, |url, expires_at_ms| async move {
                progress.send(Job::Login(Box::new(super::super::Progress::Plugin(Progress::Prompt { caller, id, url, expires_at_ms })))).await
                    .map_err(|_| super::super::super::unavailable())
            }, &stop);
            let result = AssertUnwindSafe(operation).catch_unwind().await.unwrap_or_else(|_| Err(Fault::new(ErrorCode::OutcomeUnknown, "application registration outcome is unknown; check the provider before creating another application")))
                .map(|credentials| Authorized::Application(credentials, baseline));
            let _ = sender.send(Job::Login(Box::new(super::super::Progress::Plugin(Progress::Finished { caller, id, result })))).await;
        });
        Ok(output)
    }
}
