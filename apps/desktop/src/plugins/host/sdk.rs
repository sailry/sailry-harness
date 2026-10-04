//! Public plugin conveniences reuse the same scoped Host and durable requests.
mod computer;
mod connections;
pub(super) mod conversation;
mod files;
mod git;
mod media;
mod models;
mod roles;
mod terminals;
#[cfg(test)]
mod tests;
pub(in crate::plugins) mod values;
mod worktrees;
use super::*;
use sailry_protocol::{Output, RequestOutcome};
use serde_json::{Value, json};
use std::time::Duration;
use values::{decode, encode, revision};

impl Host {
    #[cfg(test)]
    pub(in crate::plugins) fn sdk(self: &Arc<Self>) -> HostModule {
        self.sdk_with_documents(None, false, Default::default())
    }

    pub(in crate::plugins) fn sdk_with_documents(
        self: &Arc<Self>,
        documents: Option<gpui_kit::Entity<crate::plugins::documents::Controller>>,
        write_files: bool,
        sources: super::usage::Sources,
    ) -> HostModule {
        let completed_documents = documents.clone();
        let forgotten_documents = documents.clone();
        let settings = self.clone();
        let catalog = self.clone();
        let read = self.clone();
        let list = self.clone();
        let write = self.clone();
        let indexed = self.clone();
        let search = self.clone();
        let remove = self.clone();
        let prepare = self.clone();
        let complete = self.clone();
        let forget = self.clone();
        let model = self.clone();
        let publish = self.clone();
        let events = self.clone();
        let http = self.clone();
        let notify = self.clone();
        let transaction = self.clone();
        let projects = self.clone();
        let start = self.clone();
        let context = self.clone();
        let identifiers = self.clone();
        let module = HostModule::new("sailry/sdk")
            .function("newId", move |_| {
                identifiers.check()?;
                Ok(HostValue::from(RequestId::new().to_string()))
            })
            .function("context", move |_| {
                context.check()?;
                let scope = context.context();
                let mut value = serde_json::to_value(&scope).map_err(|error| HostError::new(error.to_string()))?;
                value["package"]["settings_revision"] = json!(scope.package.settings_revision.to_string());
                encode(value)
            })
            .async_function("readProjectCatalog", move |_| projects.read_public(Command::ReadProjectCatalog))
            .function("startSession", move |args| {
                let draft = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                start.prepare_public(Command::StartSession(draft))
            })
            .function("prepareTransaction", move |args| {
                let operations = sailry_protocol::plugin::transaction::sdk::decode(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.message))?;
                transaction.prepare_public(Command::PluginTransaction { operations })
            })
            .function("prepareNotification", move |args| {
                let content = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                notify.prepare_public(Command::PublishNotification {
                    package: notify.context().package, content,
                })
            })
            .function("requestHttp", move |args| {
                let request = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                http.prepare_public(Command::RequestPluginHttp(request))
            })
            .function("publishContributions", move |args| {
                publish.check()?;
                let bridge = publish.contributions.as_ref().ok_or_else(|| HostError::new("plugin has no contribution surface"))?;
                let states = serde_json::from_value(decode(args.value(0)?)?).map_err(|error| HostError::new(error.to_string()))?;
                bridge.publish(states).map_err(HostError::new)?;
                Ok(HostValue::Null)
            })
            .async_function("nextContributionEvent", move |_| {
                events.check()?;
                let bridge = events.contributions.clone().ok_or_else(|| HostError::new("plugin has no contribution surface"))?;
                let stop = events.stop.clone();
                Ok(async move {
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => Err(HostError::new("plugin view is closed")),
                        event = bridge.next() => {
                            let event = event.ok_or_else(|| HostError::new("contribution surface is closed"))?;
                            encode(serde_json::to_value(event).map_err(|error| HostError::new(error.to_string()))?)
                        }
                    }
                })
            })
            .function("faultCode", |args| { Ok(HostValue::from(values::fault_code(args.string(0)?))) })
            .function("errorCode", |args| {
                Ok(HostValue::from(values::error_code(args.string(0)?)))
            })
            .async_function("readSettings", move |_| {
                settings.read_public(Command::ReadPluginSettings {
                    package: settings.context().package,
                })
            })
            .async_function("listModels", move |_| {
                catalog.read_public(Command::ListPluginModels)
            })
            .function("resolveModel", |args| {
                models::resolve(
                    decode(args.value(0)?)?,
                    decode(args.value(1)?)?,
                    args.string(2)?,
                )
            })
            .function("modelCommand", |args| {
                models::command(decode(args.value(0)?)?, decode(args.value(1)?)?).and_then(encode)
            })
            .function("modelChoice", |args| {
                models::select(
                    decode(args.value(0)?)?,
                    decode(args.value(1)?)?,
                    decode(args.value(2)?)?,
                )
                .and_then(encode)
            })
            .function("prepareModel", move |args| {
                let value = models::command(decode(args.value(0)?)?, decode(args.value(1)?)?)?;
                model.prepare_public(
                    serde_json::from_value(value)
                        .map_err(|error| HostError::new(error.to_string()))?,
                )
            })
            .function("prepareRequest", move |args| {
                prepare.prepare_public(values::command(decode(args.value(0)?)?)?)
            })
            .async_function("completeRequest", move |args| {
                let id = args.string(0)?.parse::<RequestId>().map_err(|error| HostError::new(error.to_string()))?;
                type Completion = std::pin::Pin<Box<dyn Future<Output = Result<HostValue, HostError>> + Send>>;
                if let Some(receive) = crate::plugins::documents::sdk::entry(&completed_documents, &complete, id, write_files)? {
                    return Ok(Box::pin(crate::plugins::documents::sdk::complete(receive, true)) as Completion);
                }
                Ok(Box::pin(complete.complete_public(id)?) as Completion)
            })
            .function("forgetRequest", move |args| {
                let id: RequestId = args
                    .string(0)?
                    .parse::<RequestId>()
                    .map_err(|error| HostError::new(error.to_string()))?;
                forget.settings_forgettable(id)?;
                forget.requests.lock().map_err(lock_error)?.remove(&id);
                gpui_shell::with_current_app(|cx| { if let Some(controller) = &forgotten_documents { controller.update(cx, |controller,cx|{if controller.owns_entry(id, &forget.context.package.name){controller.forget_entry(id,cx);}}); } });
                Ok(HostValue::Null)
            })
            .async_function("getValue", move |args| {
                read.read_public(Command::ReadPluginValue {
                    key: args.string(0)?.into(),
                })
            })
            .async_function("listKeys", move |args| {
                list.read_public(Command::ListPluginKeys {
                    prefix: args.get(0).and_then(HostValue::as_str).unwrap_or("").into(),
                    after: args.get(1).and_then(HostValue::as_str).map(str::to_owned),
                    limit: args
                        .get(2)
                        .map(|_| args.integer(2))
                        .transpose()?
                        .unwrap_or(100)
                        .try_into()
                        .map_err(|_| HostError::new("invalid key page limit"))?,
                })
            })
            .function("setValue", move |args| {
                write.prepare_public(Command::WritePluginValue {
                    key: args.string(0)?.into(),
                    value: decode(args.value(1)?)?,
                    expected_revision: revision(args.string(2)?)?,
                })
            })
            .function("indexedValue", move |args| {
                indexed.prepare_public(Command::WriteIndexedPluginValue {
                    key: args.string(0)?.into(),
                    value: decode(args.value(1)?)?,
                    index: serde_json::from_value(decode(args.value(2)?)?).map_err(|error| HostError::new(error.to_string()))?,
                    expected_revision: revision(args.string(3)?)?,
                })
            })
            .async_function("searchValues", move |args| {
                let query = serde_json::from_value(decode(args.value(0)?)?).map_err(|error| HostError::new(error.to_string()))?;
                search.read_public(Command::SearchPluginValues(query))
            })
            .function("deleteValue", move |args| {
                remove.prepare_public(Command::RemovePluginValue {
                    key: args.string(0)?.into(),
                    expected_revision: revision(args.string(1)?)?,
                })
            });
        let module =
            self.session_model_module(self.worktree_module(self.usage_module(module, sources)));
        self.change_module(self.roles_module(self.media_module(self.computer_module(
            self.file_module(
                self.terminal_module(self.git_module(self.conversation_module(module))),
                documents,
            ),
        ))))
        .declarations(include_str!("sdk/api.d.ts"))
    }

    pub(super) fn prepare_public(&self, command: Command) -> Result<HostValue, HostError> {
        self.check()?;
        if serde_json::to_vec(&command)
            .map_err(|error| HostError::new(error.to_string()))?
            .len()
            > MAX_COMMAND_BYTES
        {
            return Err(HostError::new("plugin command exceeds the size limit"));
        }
        let mut requests = self.requests.lock().map_err(lock_error)?;
        if requests.len() >= MAX_DRAFTS {
            return Err(HostError::new("plugin request draft capacity exhausted"));
        }
        let request = self.client.prepare(command).with_plugin(self.context());
        let id = request.id;
        self.prepare_settings(id, &request.command)?;
        requests.insert(id, request);
        Ok(HostValue::from(id.to_string()))
    }

    pub(super) fn read_public(
        self: &Arc<Self>,
        command: Command,
    ) -> Result<
        impl Future<Output = Result<HostValue, HostError>> + Send + 'static + use<>,
        HostError,
    > {
        self.read_scoped(command, false)
    }

    fn read_scoped(
        self: &Arc<Self>,
        command: Command,
        structured: bool,
    ) -> Result<
        impl Future<Output = Result<HostValue, HostError>> + Send + 'static + use<>,
        HostError,
    > {
        self.check()?;
        self.settings_readable()?;
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| HostError::new("plugin request capacity exhausted"))?;
        let owner = self.clone();
        let request = owner.client.prepare(command).with_plugin(owner.context());
        Ok(async move {
            let runtime = owner.runtime.clone();
            runtime.spawn(async move {
                let _permit = permit;
                let result = tokio::select! {
                    biased;
                    _ = owner.stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                    result = owner.client.execute(request) => result,
                }.map_err(|fault| if structured { values::fault(fault) } else {HostError::new(fault.message)})?;
                let output = public_output(result)?;
                encode(output.get("data").cloned().unwrap_or(Value::Null))
            }).await.map_err(|_| HostError::new("plugin request worker failed"))?
        })
    }

    fn complete_public(
        self: &Arc<Self>,
        id: RequestId,
    ) -> Result<
        impl Future<Output = Result<HostValue, HostError>> + Send + 'static + use<>,
        HostError,
    > {
        self.check()?;
        let request = self
            .requests
            .lock()
            .map_err(lock_error)?
            .get(&id)
            .cloned()
            .ok_or_else(|| HostError::new("plugin request draft is unavailable"))?;
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| HostError::new("plugin request capacity exhausted"))?;
        let owner = self.clone();
        Ok(async move {
            let runtime = owner.runtime.clone();
            runtime
                .spawn(async move {
                    let _permit = permit;
                    let work = async {
                        let result = confirmed(&owner.client, request).await?;
                        owner.finish_settings(id, &result).await?;
                        match result {
                            Ok(output) => encode(json!({"Ok":public_output(output)?})),
                            Err(fault) => encode(json!({"Err":fault})),
                        }
                    };
                    tokio::select! {
                        biased;
                        _ = owner.stop.cancelled() => Err(HostError::new("plugin view is closed")),
                        result = work => result,
                    }
                })
                .await
                .map_err(|_| HostError::new("plugin request worker failed"))?
        })
    }
}

pub(in crate::plugins) fn public_output(output: Output) -> Result<Value, HostError> {
    if matches!(output, Output::PluginSecret(_) | Output::ProviderKey(_)) {
        return Err(HostError::new(
            "protected credentials are not exposed to scripts",
        ));
    }
    if matches!(
        output,
        Output::PluginTransaction(_) | Output::PluginSearch(_)
    ) {
        return Ok(sailry_protocol::plugin::transaction::sdk::output(output));
    }
    let revision = match &output {
        Output::PluginValue(entry) => Some(entry.revision.to_string()),
        Output::Session(session) => Some(session.revision.to_string()),
        _ => None,
    };
    let mut value =
        serde_json::to_value(&output).map_err(|error| HostError::new(error.to_string()))?;
    connections::exact_rows(&output, &mut value);
    if let Output::PluginSettings(settings) = &output {
        value["data"]["package"]["settings_revision"] =
            json!(settings.package.settings_revision.to_string());
    }
    if let Some(revision) = revision {
        value["data"]["revision"] = revision.into();
    }
    Ok(value)
}

pub(in crate::plugins) async fn confirmed(
    client: &Client,
    request: Request,
) -> Result<Result<Output, sailry_protocol::Fault>, HostError> {
    let mut result = client.execute(request.clone()).await;
    if result.is_err() && request.command.durable() {
        loop {
            match client.outcome(&request).await {
                Ok(RequestOutcome::Completed(confirmed)) => {
                    result = *confirmed;
                    break;
                }
                Ok(RequestOutcome::Admitted) => {
                    tokio::time::sleep(Duration::from_millis(250)).await
                }
                _ => return Err(HostError::new("unconfirmed")),
            }
        }
    }
    Ok(result)
}
