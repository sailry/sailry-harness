//! Explicit controller injection and model drafts; execution stays in its service owner.
mod controller;
use super::sdk::values::{decode, encode, fault};
use super::*;
use sailry_protocol::{
    installation::{ModelRef, Registration, Target},
    models,
};
use serde_json::json;

#[derive(Default)]
pub(super) struct Services {
    manager: Option<Arc<crate::dictation::models::Manager>>,
    pub(super) runtime: Option<Arc<tokio::runtime::Runtime>>,
    install: bool,
    drafts: BTreeMap<RequestId, controller::Draft>,
}
impl Services {
    pub(super) fn len(&self) -> usize {
        self.drafts.len()
    }
}

impl Host {
    pub(in crate::plugins) fn model_services(
        &self,
        manager: Option<Arc<crate::dictation::models::Manager>>,
        runtime: Arc<tokio::runtime::Runtime>,
        install: bool,
    ) {
        let mut models = self.models.lock().expect("model services");
        models.manager = manager;
        models.runtime = Some(runtime);
        models.install = install;
    }

    pub(in crate::plugins) fn models_module(self: &Arc<Self>) -> HostModule {
        // The pinned Kit HostModule exposes flat named exports, not nested
        // namespaces. Separate modules preserve Kit's resolver and lifecycle.
        let search = self.clone();
        let versions = self.clone();
        let resolve = self.clone();
        let register = self.clone();
        let prepare = self.clone();
        let list = self.clone();
        HostModule::new("sailry/models")
            .async_function("list", move |args| {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input {
                    target: Target,
                    kind: Option<models::Kind>,
                }
                let input: Input = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                if input.target != Target::Node || input.kind == Some(models::Kind::Speech) {
                    return Err(fault(sailry_protocol::Fault::invalid(
                        "Node local model list required",
                    )));
                }
                list.read_public(Command::ReadLocalModels { kind: input.kind })
            })
            .async_function("search", move |args| {
                let query = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                search.read_public(Command::SearchModels(query))
            })
            .async_function("versions", move |args| {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input {
                    hub: models::Hub,
                    repository: String,
                }
                let input: Input = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                versions.read_public(Command::ReadModelVersions {
                    hub: input.hub,
                    repository: input.repository,
                })
            })
            .async_function("resolve", move |args| {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input {
                    kind: models::Kind,
                    source: models::Source,
                }
                let input: Input = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                resolve.read_public(Command::ResolveModel {
                    kind: input.kind,
                    source: input.source,
                })
            })
            .function("register", move |args| {
                let input: Registration = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                input.validate().map_err(fault)?;
                if input.target == Target::Controller {
                    register.prepare_controller(controller::Operation::Register(input.definition))
                } else {
                    register.prepare_public(input.node_command().map_err(fault)?)
                }
            })
            .function("prepare", move |args| {
                let input: ModelRef = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                input.validate().map_err(fault)?;
                if let ModelRef::Controller { id, .. } = input {
                    prepare.prepare_controller(controller::Operation::Install(id))
                } else {
                    prepare.prepare_public(input.node_command().map_err(fault)?)
                }
            })
            .declarations(include_str!("model_assets/api.d.ts"))
    }

    pub(super) fn model_feedback(
        &self,
        request: &Request,
    ) -> Option<tokio::sync::oneshot::Sender<Result<(), sailry_protocol::Fault>>> {
        let runtime = self.models.lock().ok()?.runtime.clone()?;
        gpui_shell::with_current_app(|cx| {
            let window = cx.active_window()?;
            window
                .update(cx, |_, window, cx| {
                    crate::installations::external(
                        self.client.clone(),
                        runtime,
                        request,
                        window,
                        cx,
                    )
                })
                .ok()
                .flatten()
        })
        .flatten()
    }

    pub(super) fn forget_model(&self, id: RequestId) -> Result<(), HostError> {
        let mut models = self.models.lock().map_err(lock_error)?;
        if models.drafts.get(&id).is_some_and(|draft| draft.pending) {
            return Err(HostError::new(
                "unconfirmed model request cannot be forgotten",
            ));
        }
        models.drafts.remove(&id);
        Ok(())
    }
}
