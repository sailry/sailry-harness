use super::*;
use sailry_protocol::{Effort, WorkMode};

impl Adapter {
    pub(super) async fn controls(
        &self,
        session: &Session,
    ) -> acp::Result<(acp::SessionModeState, Vec<acp::SessionConfigOption>)> {
        let context = self.context(session);
        let mode = mode_id(session.config.mode);
        let modes = acp::SessionModeState::new(
            mode,
            vec![
                acp::SessionMode::new("plan", "Plan"),
                acp::SessionMode::new("code", "Code"),
            ],
        )
        .meta(metadata(context.clone()));
        let mut models =
            BTreeMap::from([(session.config.model.clone(), session.config.model.clone())]);
        let configured = if let Some(profile) = &session.profile {
            Some(profile.provider.clone())
        } else {
            let (output, _) = self.execute(Command::ListProviders, None).await?;
            let Output::Providers(providers) = output else {
                return Err(invalid("provider list expected"));
            };
            providers
                .into_iter()
                .find(|provider| provider.id == session.config.provider)
        };
        if let Some(provider) = &configured {
            for model in &provider.models {
                models.insert(
                    model.id.clone(),
                    model
                        .display_name
                        .clone()
                        .unwrap_or_else(|| model.id.clone()),
                );
            }
        }
        let models = models
            .into_iter()
            .map(|(id, label)| acp::SessionConfigSelectOption::new(id, label))
            .collect::<Vec<_>>();
        let mut efforts = vec![Effort::Default];
        if let Some(provider) = &configured {
            if let Some(model) = provider
                .models
                .iter()
                .find(|model| model.id == session.config.model)
            {
                efforts.extend(
                    model
                        .efforts
                        .iter()
                        .copied()
                        .filter(|effort| *effort != Effort::Default),
                );
            } else {
                efforts.extend(
                    [
                        Effort::Disabled,
                        Effort::Minimal,
                        Effort::Low,
                        Effort::Medium,
                        Effort::High,
                        Effort::XHigh,
                        Effort::Max,
                    ]
                    .into_iter()
                    .filter(|effort| effort.validate(provider.api, 0).is_ok()),
                );
            }
        }
        let current = effort_id(&session.config.effort);
        if !efforts.contains(&session.config.effort) {
            efforts.push(session.config.effort);
        }
        let efforts = efforts
            .into_iter()
            .map(|effort| {
                let id = effort_id(&effort);
                acp::SessionConfigSelectOption::new(id.clone(), id)
            })
            .collect::<Vec<_>>();
        let options = vec![
            acp::SessionConfigOption::select(
                "mode",
                "Mode",
                mode,
                vec![
                    acp::SessionConfigSelectOption::new("plan", "Plan"),
                    acp::SessionConfigSelectOption::new("code", "Code"),
                ],
            )
            .category(acp::SessionConfigOptionCategory::Mode)
            .meta(metadata(context.clone())),
            acp::SessionConfigOption::select(
                "model",
                "Model",
                session.config.model.clone(),
                models,
            )
            .category(acp::SessionConfigOptionCategory::Model)
            .meta(metadata(context.clone())),
            acp::SessionConfigOption::select("reasoning", "Reasoning", current, efforts)
                .category(acp::SessionConfigOptionCategory::ThoughtLevel)
                .meta(metadata(context)),
        ];
        Ok((modes, options))
    }

    pub(super) fn context(&self, session: &Session) -> Value {
        json!({"node":self.client.target(),"session":session.id,"worktree":session.worktree,"revision":session.revision})
    }

    pub(super) async fn emit_controls(
        &self,
        session: &Session,
        connection: &ConnectionTo<sdk::Client>,
    ) -> acp::Result<(acp::SessionModeState, Vec<acp::SessionConfigOption>)> {
        let id = acp::SessionId::new(session.id.to_string());
        let context = self.context(session);
        let (modes, options) = self.controls(session).await?;
        notify(
            connection,
            &id,
            acp::SessionUpdate::CurrentModeUpdate(acp::CurrentModeUpdate::new(mode_id(
                session.config.mode,
            ))),
            context.clone(),
        )?;
        notify(
            connection,
            &id,
            acp::SessionUpdate::ConfigOptionUpdate(acp::ConfigOptionUpdate::new(options.clone())),
            context.clone(),
        )?;
        notify(
            connection,
            &id,
            acp::SessionUpdate::SessionInfoUpdate(
                acp::SessionInfoUpdate::new().title(session.activity.title.clone()),
            ),
            context,
        )?;
        Ok((modes, options))
    }

    pub(super) async fn set_mode(
        &self,
        request: acp::SetSessionModeRequest,
        connection: &ConnectionTo<sdk::Client>,
    ) -> acp::Result<acp::SetSessionModeResponse> {
        let (session, id) = self
            .change(
                &request.session_id,
                "mode",
                &request.mode_id.to_string(),
                request.meta.as_ref(),
            )
            .await?;
        let context = json!({"node":self.client.target(),"session":session.id,"request_id":id,"revision":session.revision,"state":"configured"});
        self.emit_controls(&session, connection)
            .await
            .map_err(|error| captured(error, context.clone()))?;
        Ok(acp::SetSessionModeResponse::new().meta(metadata(context)))
    }

    pub(super) async fn set_config(
        &self,
        request: acp::SetSessionConfigOptionRequest,
        connection: &ConnectionTo<sdk::Client>,
    ) -> acp::Result<acp::SetSessionConfigOptionResponse> {
        let value = request
            .value
            .as_value_id()
            .ok_or_else(|| invalid("this configuration option requires a selected value"))?;
        let (session, id) = self
            .change(
                &request.session_id,
                &request.config_id.to_string(),
                &value.to_string(),
                request.meta.as_ref(),
            )
            .await?;
        let context = json!({"node":self.client.target(),"session":session.id,"request_id":id,"revision":session.revision,"state":"configured"});
        let (_, options) = self
            .emit_controls(&session, connection)
            .await
            .map_err(|error| captured(error, context.clone()))?;
        Ok(acp::SetSessionConfigOptionResponse::new(options).meta(metadata(context)))
    }

    async fn change(
        &self,
        id: &acp::SessionId,
        key: &str,
        value: &str,
        meta: Option<&acp::Meta>,
    ) -> acp::Result<(Session, RequestId)> {
        let opened = self.opened(id).await?;
        let captured = opened.session.lock().await.clone();
        let (output, _) = self
            .execute(
                Command::ReadSession {
                    session: captured.id,
                },
                None,
            )
            .await?;
        let Output::Session(current) = output else {
            return Err(invalid("session configuration response expected"));
        };
        let (_, options) = self.controls(&current).await?;
        let selected = options
            .iter()
            .find(|option| option.id.to_string() == key)
            .ok_or_else(|| invalid("configuration option is unavailable"))?;
        let acp::SessionConfigKind::Select(select) = &selected.kind else {
            return Err(invalid("configuration option is unavailable"));
        };
        let acp::SessionConfigSelectOptions::Ungrouped(choices) = &select.options else {
            return Err(invalid("configuration option is unavailable"));
        };
        if !choices
            .iter()
            .any(|choice| choice.value.to_string() == value)
        {
            return Err(invalid("configuration choice is unavailable"));
        }
        let mut config = current.config.clone();
        match key {
            "mode" => config.mode = parse_mode(value)?,
            "model" => config.model = value.into(),
            "reasoning" => config.effort = parse_effort(value)?,
            _ => return Err(invalid("configuration option is unavailable")),
        }
        let revision = member(meta, "expected_revision")?
            .map(|value| {
                value
                    .as_u64()
                    .ok_or_else(|| invalid("expected_revision must be an integer"))
            })
            .transpose()?
            .unwrap_or(captured.revision);
        let (output, request) = self
            .execute(
                Command::SetSessionConfig {
                    session: captured.id,
                    expected_revision: revision,
                    config,
                },
                meta,
            )
            .await?;
        let Output::Session(session) = output else {
            return Err(invalid("session configuration response expected"));
        };
        *opened.session.lock().await = session.clone();
        Ok((session, request))
    }
}

pub(super) fn mode_id(mode: WorkMode) -> &'static str {
    match mode {
        WorkMode::Plan => "plan",
        WorkMode::Code => "code",
    }
}

fn parse_mode(value: &str) -> acp::Result<WorkMode> {
    match value {
        "plan" => Ok(WorkMode::Plan),
        "code" => Ok(WorkMode::Code),
        _ => Err(invalid("mode is unavailable")),
    }
}

fn effort_id(value: &Effort) -> String {
    match value {
        Effort::Budget(budget) => format!("budget:{budget}"),
        _ => serde_json::to_value(value)
            .expect("effort serialization")
            .as_str()
            .expect("named effort")
            .into(),
    }
}

fn parse_effort(value: &str) -> acp::Result<Effort> {
    if let Some(budget) = value.strip_prefix("budget:") {
        return budget
            .parse()
            .map(Effort::Budget)
            .map_err(|_| invalid("reasoning budget is invalid"));
    }
    serde_json::from_value(json!(value)).map_err(|_| invalid("reasoning choice is unavailable"))
}
