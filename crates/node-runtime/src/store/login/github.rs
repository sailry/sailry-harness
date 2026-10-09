use super::*;
use crate::store::Ingress;

pub(in crate::store) struct Completed {
    pub caller: NodeId,
    pub request: Request,
    pub reply: oneshot::Sender<Response>,
    pub result: Result<crate::plugins::github::login::Identity, Fault>,
}

impl Worker {
    pub(super) fn import_github(
        &mut self,
        database: &mut Database,
        caller: NodeId,
        request: Request,
        reply: oneshot::Sender<Response>,
        events: &broadcast::Sender<EventEnvelope>,
    ) {
        let prepared = (|| {
            if request.plugin.is_some() {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "GitHub login requires a controller",
                ));
            }
            if self.pending >= super::super::CAPACITY || self.closed.is_cancelled() {
                return Err(super::super::unavailable());
            }
            let Command::ImportGithubLogin { package } = &request.command else {
                unreachable!()
            };
            super::super::plugins::github::prepare(&database.connection, &self.plugins, package)
        })();
        if let Err(error) = prepared {
            let _ = reply.send(database.finish_external(caller, &request, Err(error), events));
            return;
        }
        self.pending += 1;
        let (sender, environment, stop) = (
            self.sender.clone(),
            self.environment.clone(),
            self.closed.child_token(),
        );
        self.runtime.spawn(async move {
            let result =
                AssertUnwindSafe(crate::plugins::github::login::import(&environment, &stop))
                    .catch_unwind()
                    .await
                    .unwrap_or_else(|_| Err(super::super::unavailable()));
            let _ = sender
                .send(Job::Login(Box::new(Progress::Github(Box::new(
                    Completed {
                        caller,
                        request,
                        reply,
                        result,
                    },
                )))))
                .await;
        });
    }

    pub(super) fn github_progress(
        &mut self,
        database: &mut Database,
        completed: Completed,
        events: &broadcast::Sender<EventEnvelope>,
    ) {
        self.pending -= 1;
        let Completed {
            caller,
            request,
            reply,
            result,
        } = completed;
        let Command::ImportGithubLogin { package } = &request.command else {
            unreachable!()
        };
        let output = database.finish_external_with(
            caller,
            &request,
            Err(super::super::unavailable()),
            events,
            |db, output| {
                let result = result.and_then(|token| {
                    super::super::plugins::github::save(db, &self.plugins, package, token)
                });
                match result {
                    Ok((value, event)) => {
                        *output = Ok(value);
                        Ok(event)
                    }
                    Err(error) if error.code == ErrorCode::Internal => Err(error),
                    Err(error) => {
                        *output = Err(error);
                        Ok(None)
                    }
                }
            },
        );
        let _ = reply.send(output);
    }
}

impl Ingress {
    pub(in crate::store) async fn read_github_login(
        &self,
        request: Request,
    ) -> Result<sailry_link::Admission, Fault> {
        if request.target != self.node || request.version != VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        if request.plugin.is_some() {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "GitHub login requires a controller",
            ));
        }
        let Command::ReadGithubLogin { package } = &request.command else {
            unreachable!()
        };
        let binding = self.github_target(package.clone()).await?;
        let (available, identity) =
            crate::plugins::github::login::status(&self.environment, &self.closed).await?;
        self.github_target(package.clone()).await?;
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(Ok(Output::GithubLogin(plugin::github::Status {
            package: package.clone(),
            available,
            authenticated: identity.is_some(),
            bound: binding.is_some() && binding == identity,
        })));
        Ok(sailry_link::Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }

    async fn github_target(
        &self,
        package: plugin::Reference,
    ) -> Result<Option<crate::plugins::github::login::Identity>, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Job::GithubTarget { package, reply })
            .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }
}
