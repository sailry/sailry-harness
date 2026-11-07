//! Captured tool declarations and handlers for the Node execution loop.
use crate::agent::engine::Emitter;
use async_trait::async_trait;
use rmcp::model::{CallToolRequestParams, CallToolResult, ErrorData, Tool};
use sailry_link::CancellationToken;
use sailry_protocol::{Fault, tool::Presentation};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
mod context;
pub(in crate::agent) use context::CallContext;
pub(crate) use context::CatalogSession;
pub(super) mod display;
mod observed;
pub(in crate::agent) use observed::Observed;

#[async_trait]
pub(crate) trait ToolProvider<S>: Send + Sync {
    async fn tools(&self, session: &S) -> anyhow::Result<Vec<Tool>>;
    async fn call(
        &self,
        session: &S,
        id: &str,
        call: CallToolRequestParams,
        emit: &Emitter,
    ) -> Result<CallToolResult, ErrorData>;
    fn concurrent(&self, _: &S, _: &CallToolRequestParams) -> bool {
        false
    }
}

pub(super) struct Registration {
    pub tool: Tool,
    pub provider: Arc<dyn ToolProvider<CallContext>>,
    pub plugin: Option<String>,
    pub presentation: Presentation,
    pub grouping: sailry_protocol::tool::Grouping,
    pub display: Option<(String, sailry_protocol::tool::Display)>,
    pub managed: bool,
    pub deferred: bool,
    pub delegation: bool,
    /// The call receives the file revisions its session has observed.
    pub observes: bool,
    /// The bound Node operation; file operation results establish observed revisions.
    pub operation: Option<sailry_protocol::tool::Operation>,
    /// Read-only calls that neither change nor depend on state other calls of a
    /// response observe, so they may run alongside each other.
    pub concurrent: bool,
}
impl Registration {
    pub fn new(
        tool: Tool,
        provider: Arc<dyn ToolProvider<CallContext>>,
        presentation: Presentation,
    ) -> Self {
        Self {
            tool,
            provider,
            plugin: None,
            presentation,
            grouping: Default::default(),
            display: None,
            managed: false,
            deferred: false,
            delegation: false,
            observes: false,
            operation: None,
            concurrent: false,
        }
    }
    pub fn managed(tool: Tool, provider: Arc<dyn ToolProvider<CallContext>>) -> Self {
        Self {
            managed: true,
            ..Self::new(tool, provider, Presentation::default())
        }
    }
    pub fn read_only(&self) -> bool {
        self.tool
            .annotations
            .as_ref()
            .and_then(|value| value.read_only_hint)
            .unwrap_or(false)
    }
}

pub(super) struct Catalog {
    timeline: Option<Arc<super::observation::Timeline>>,
    session: String,
    stop: CancellationToken,
    entries: BTreeMap<String, Registration>,
    children: Option<super::engine::Children>,
    hooks: Option<Arc<crate::hooks::Binding>>,
    /// Tool hook scripts run around each call, so every call runs alone.
    serial: bool,
    metrics: Option<(std::path::PathBuf, Arc<crate::store::agent::Invocation>)>,
}
impl Catalog {
    pub fn new(
        session: String,
        stop: CancellationToken,
        timeline: Option<Arc<super::observation::Timeline>>,
    ) -> Self {
        Self {
            timeline,
            session,
            stop,
            entries: BTreeMap::new(),
            children: None,
            hooks: None,
            serial: false,
            metrics: None,
        }
    }
    pub fn with_children(mut self, children: super::engine::Children) -> Self {
        self.children = Some(children);
        self
    }
    pub fn with_hooks(mut self, hooks: Option<Arc<crate::hooks::Binding>>) -> Self {
        self.serial = hooks.as_ref().is_some_and(|hooks| hooks.wraps_tools());
        self.hooks = hooks;
        self
    }
    pub fn with_metrics(
        mut self,
        profile: Option<&std::path::Path>,
        invocation: &Arc<crate::store::agent::Invocation>,
    ) -> Self {
        self.metrics = profile.map(|profile| (profile.to_owned(), invocation.clone()));
        self
    }
    pub fn register(&mut self, registration: Registration) -> Result<(), Fault> {
        let name = registration.tool.name.to_string();
        if self.entries.contains_key(&name) {
            return Err(Fault::invalid("tool registration has a duplicate name"));
        }
        self.entries.insert(name, registration);
        Ok(())
    }
    pub fn groupings(&self) -> BTreeMap<String, sailry_protocol::tool::Grouping> {
        self.entries
            .iter()
            .map(|(name, entry)| (name.clone(), entry.grouping))
            .collect()
    }
    pub fn displays(&self) -> BTreeMap<String, sailry_protocol::tool::Display> {
        self.entries
            .values()
            .filter_map(|entry| entry.display.clone())
            .collect()
    }
    pub fn delegations(&self) -> std::collections::BTreeSet<String> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.delegation)
            .map(|(name, _)| name.clone())
            .collect()
    }
    pub fn presentations(&self) -> BTreeMap<String, Presentation> {
        self.entries
            .iter()
            .map(|(name, entry)| (name.clone(), entry.presentation))
            .collect()
    }
    /// Whether a call may run alongside the adjacent concurrent calls of its
    /// response.
    pub fn concurrent(&self, call: &CallToolRequestParams) -> bool {
        !self.serial
            && self
                .entries
                .get(call.name.as_ref())
                .is_some_and(|entry| entry.concurrent && entry.read_only())
    }
}
#[async_trait]
impl<S: CatalogSession + Send + Sync> ToolProvider<S> for Catalog {
    async fn tools(&self, session: &S) -> anyhow::Result<Vec<Tool>> {
        anyhow::ensure!(
            session.id() == self.session,
            "tool catalog belongs to another session"
        );
        Ok(self
            .entries
            .values()
            .map(|entry| entry.tool.clone())
            .collect())
    }
    async fn call(
        &self,
        session: &S,
        id: &str,
        call: CallToolRequestParams,
        emit: &Emitter,
    ) -> Result<CallToolResult, ErrorData> {
        if session.id() != self.session || self.stop.is_cancelled() {
            return Err(ErrorData::invalid_request(
                "tool catalog is unavailable",
                None,
            ));
        }
        let entry = self
            .entries
            .get(call.name.as_ref())
            .ok_or_else(|| ErrorData::invalid_params("tool is not registered", None))?;
        if emit.cancel_token().is_cancelled() {
            return Err(fault(Fault::new(
                sailry_protocol::ErrorCode::Cancelled,
                "tool call was cancelled before dispatch",
            )));
        }
        let mut context = CallContext::new(
            self.session.clone(),
            id,
            &call,
            emit.cancel_token().clone(),
            session.context().clone(),
        )?;
        context.children = self.children.clone();
        if entry.observes {
            context.observed = Some(Arc::new(Observed::collect(
                session.conversation(),
                |name| {
                    self.entries
                        .get(name)
                        .and_then(|entry| entry.operation)
                        .is_some_and(Observed::source)
                },
            )));
        }
        let name = entry.tool.name.as_ref();
        let metric = self.metrics.as_ref().and_then(|(profile, invocation)| {
            if !crate::telemetry::metrics_enabled(Some(profile)) {
                return None;
            }
            crate::telemetry::metrics::Call::new(
                Some(profile),
                crate::telemetry::metrics::Tool::capture(name, &invocation.plugins),
                &self.stop,
            )
        });
        if let Some(timeline) = &self.timeline {
            timeline.forget_failure(context.branch(), id, name);
        }
        if let Some(hooks) = &self.hooks
            && let Err(error) = hooks.before(name).await
        {
            if let Some(metric) = metric {
                metric.finish(
                    if error.code == sailry_protocol::ErrorCode::PermissionDenied {
                        crate::telemetry::metrics::ToolResult::Denied
                    } else {
                        crate::telemetry::metrics::ToolResult::Error
                    },
                    Some(error.code),
                );
            }
            return Err(fault(error));
        }
        let work = async {
            let call = entry.provider.call(&context, id, call, emit);
            match &self.timeline {
                Some(timeline) => timeline.tool(entry.delegation, call).await,
                None => call.await,
            }
        };
        let result = tokio::select! {
            biased;
            _ = emit.cancelled() => Err(fault(Fault::new(
                sailry_protocol::ErrorCode::Cancelled,
                "tool call was interrupted; its outcome may be unknown; do not replay it automatically",
            ))),
            result = work => result,
        };
        if let Err(error) = &result
            && let Some(timeline) = &self.timeline
        {
            let code = error
                .data
                .clone()
                .and_then(|data| serde_json::from_value::<Fault>(data).ok())
                .map(|fault| fault.code);
            timeline.fail_tool(context.branch(), id, name, code);
        }
        if let Some(metric) = metric {
            let failure = self
                .timeline
                .as_ref()
                .and_then(|timeline| timeline.tool_failure(context.branch(), id, name));
            let (outcome, code) = classify(&result, failure.as_ref());
            metric.finish(outcome, code);
        }
        if let Some(hooks) = &self.hooks {
            let success = result
                .as_ref()
                .is_ok_and(|result| result.is_error != Some(true));
            hooks.after(name, success).await;
        }
        result
    }
    fn concurrent(&self, _: &S, call: &CallToolRequestParams) -> bool {
        self.concurrent(call)
    }
}

fn classify(
    result: &Result<CallToolResult, ErrorData>,
    failure: Option<&sailry_protocol::tool::Failure>,
) -> (
    crate::telemetry::metrics::ToolResult,
    Option<sailry_protocol::ErrorCode>,
) {
    use crate::telemetry::metrics::ToolResult;
    let code = failure.and_then(|failure| failure.code).or_else(|| {
        result.as_ref().err().and_then(|error| {
            error
                .data
                .as_ref()
                .and_then(|data| serde_json::from_value::<Fault>(data.clone()).ok())
                .map(|fault| fault.code)
        })
    });
    let outcome = if code == Some(sailry_protocol::ErrorCode::PermissionDenied) {
        ToolResult::Denied
    } else if failure.is_none()
        && result
            .as_ref()
            .is_ok_and(|result| result.is_error != Some(true))
    {
        ToolResult::Success
    } else {
        ToolResult::Error
    };
    (outcome, code)
}

pub(in crate::agent) fn json(mut value: Value) -> Result<CallToolResult, Fault> {
    let context = if value.get("sailry_context").is_some_and(Value::is_object) {
        value
            .as_object_mut()
            .unwrap()
            .remove("sailry_context")
            .map(serde_json::from_value::<super::canonical::Context>)
            .transpose()
            .map_err(|_| Fault::invalid("tool context is invalid"))?
    } else {
        None
    };
    let result = CallToolResult::structured(value);
    match context {
        Some(context) => with_context(result, context),
        None => Ok(result),
    }
}
pub(in crate::agent) fn result(value: CallToolResult) -> Result<CallToolResult, Fault> {
    Ok(value)
}
pub(in crate::agent) fn fault(error: Fault) -> ErrorData {
    let error = super::plugins::public_failure(error);
    ErrorData::internal_error(error.message.clone(), serde_json::to_value(error).ok())
}
pub(in crate::agent) fn with_context(
    mut result: CallToolResult,
    context: super::canonical::Context,
) -> Result<CallToolResult, Fault> {
    let value = serde_json::to_value(context)
        .map_err(|_| Fault::invalid("tool context could not be encoded"))?;
    result
        .meta
        .get_or_insert_with(Default::default)
        .insert("sailry_context".into(), value);
    Ok(result)
}

macro_rules! provider {
    ($kind:ty) => {
        $crate::agent::catalog::provider!($kind, $crate::agent::catalog::json);
    };
    ($kind:ty, $convert:path) => {
        impl $kind {
            pub(in crate::agent) fn descriptor(&self) -> rmcp::model::Tool {
                rmcp::model::Tool::new(
                    self.name().to_owned(),
                    self.description().to_owned(),
                    self.parameters_schema()
                        .and_then(|value| value.as_object().cloned())
                        .unwrap_or_default(),
                )
                .with_annotations(
                    rmcp::model::ToolAnnotations::new().read_only(self.is_read_only()),
                )
            }
        }
        #[async_trait::async_trait]
        impl crate::agent::catalog::ToolProvider<crate::agent::catalog::CallContext> for $kind {
            async fn tools(
                &self,
                _: &crate::agent::catalog::CallContext,
            ) -> anyhow::Result<Vec<rmcp::model::Tool>> {
                Ok(vec![self.descriptor()])
            }
            async fn call(
                &self,
                context: &crate::agent::catalog::CallContext,
                _: &str,
                call: rmcp::model::CallToolRequestParams,
                _: &crate::agent::engine::Emitter,
            ) -> Result<rmcp::model::CallToolResult, rmcp::model::ErrorData> {
                self.execute(
                    std::sync::Arc::new(context.clone()),
                    serde_json::Value::Object(call.arguments.unwrap_or_default()),
                )
                .await
                .and_then($convert)
                .map_err(crate::agent::catalog::fault)
            }
        }
    };
}
pub(in crate::agent) use provider;

#[cfg(test)]
mod tests;
