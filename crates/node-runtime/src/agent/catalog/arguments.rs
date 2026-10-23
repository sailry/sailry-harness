//! Final captured declarations validate unchanged input before tool execution.
use adk_core::{AdkError, Tool, ToolContext};
use async_trait::async_trait;
use jsonschema::{Validator, error::ValidationErrorKind};
use sailry_link::CancellationToken;
use sailry_protocol::Fault;
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};

#[cfg(test)]
mod tests;

struct NoRetrieval;
impl jsonschema::Retrieve for NoRetrieval {
    fn retrieve(
        &self,
        _: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err("tool schema retrieval is disabled".into())
    }
}

pub(super) struct Validated {
    tool: Arc<dyn Tool>,
    schema: Option<Value>,
    validator: Option<Validator>,
    fields: BTreeSet<String>,
    session: String,
    stop: CancellationToken,
    timeline: Option<Arc<crate::agent::observation::Timeline>>,
}

impl Validated {
    #[cfg(test)]
    pub(super) fn bind(
        tool: Arc<dyn Tool>,
        session: String,
        stop: CancellationToken,
    ) -> Result<Arc<dyn Tool>, Fault> {
        Self::observed(tool, session, stop, None)
    }

    pub(super) fn observed(
        tool: Arc<dyn Tool>,
        session: String,
        stop: CancellationToken,
        timeline: Option<Arc<crate::agent::observation::Timeline>>,
    ) -> Result<Arc<dyn Tool>, Fault> {
        let schema = tool.parameters_schema();
        let validator = schema
            .as_ref()
            .map(|schema| {
                jsonschema::draft202012::options()
                    .with_retriever(NoRetrieval)
                    .build(schema)
                    .map_err(|_| Fault::invalid("tool parameter schema is invalid or unresolved"))
            })
            .transpose()?;
        let mut fields = BTreeSet::new();
        if let Some(schema) = &schema {
            declared_fields(schema, &mut fields);
        }
        Ok(Arc::new(Self {
            tool,
            schema,
            validator,
            fields,
            session,
            stop,
            timeline,
        }))
    }

    fn invalid(&self, arguments: &Value) -> Option<Fault> {
        let validator = self.validator.as_ref()?;
        let mut messages = Vec::new();
        for error in validator.iter_errors(arguments).take(3) {
            // Only declared identifier names are exposed. Dynamic property names,
            // rejected values, enum members and schema patterns remain private.
            let mut path = String::new();
            for segment in error.instance_path().iter() {
                let segment = segment.to_string();
                if self.fields.contains(&segment) {
                    path.push('/');
                    path.push_str(&segment);
                } else if segment.bytes().all(|byte| byte.is_ascii_digit()) && !segment.is_empty() {
                    path.push_str("/[]");
                } else {
                    break;
                }
            }
            if let ValidationErrorKind::Required { property } = error.kind()
                && let Some(name) = property.as_str().filter(|name| self.fields.contains(*name))
            {
                path.push('/');
                path.push_str(name);
            }
            if path.is_empty() {
                path.push('/');
            }
            // Bound diagnostics independently of caller-controlled nesting depth.
            if path.len() > 256 {
                path.truncate(256);
                path.push_str("/...");
            }
            let message = format!("{path}: {}", reason(error.kind()));
            if !messages.contains(&message) {
                messages.push(message);
            }
        }
        (!messages.is_empty())
            .then(|| Fault::invalid(format!("Invalid tool arguments: {}", messages.join("; "))))
    }
}

fn declared_fields(schema: &Value, fields: &mut BTreeSet<String>) {
    match schema {
        Value::Object(object) => {
            if let Some(properties) = object.get("properties").and_then(Value::as_object) {
                fields.extend(
                    properties
                        .keys()
                        .filter(|name| {
                            !name.is_empty()
                                && name.len() <= 64
                                && name.bytes().all(|byte| {
                                    byte.is_ascii_alphanumeric() || b"_-".contains(&byte)
                                })
                        })
                        .cloned(),
                );
            }
            for value in object.values() {
                declared_fields(value, fields);
            }
        }
        Value::Array(values) => {
            for value in values {
                declared_fields(value, fields);
            }
        }
        _ => {}
    }
}

fn reason(kind: &ValidationErrorKind) -> String {
    match kind {
        ValidationErrorKind::Type { kind } => format!("expected {kind:?}"),
        ValidationErrorKind::Required { .. } => "required field is missing".into(),
        ValidationErrorKind::AdditionalProperties { .. }
        | ValidationErrorKind::UnevaluatedProperties { .. } => "unsupported field".into(),
        ValidationErrorKind::Maximum { limit } => format!("must be at most {limit}"),
        ValidationErrorKind::Minimum { limit } => format!("must be at least {limit}"),
        ValidationErrorKind::ExclusiveMaximum { limit } => format!("must be less than {limit}"),
        ValidationErrorKind::ExclusiveMinimum { limit } => format!("must exceed {limit}"),
        ValidationErrorKind::MaxItems { limit } => format!("must contain at most {limit} items"),
        ValidationErrorKind::AdditionalItems { .. } => "contains unsupported items".into(),
        ValidationErrorKind::MinItems { limit } => format!("must contain at least {limit} items"),
        ValidationErrorKind::MaxLength { limit } => {
            format!("must contain at most {limit} characters")
        }
        ValidationErrorKind::MinLength { limit } => {
            format!("must contain at least {limit} characters")
        }
        ValidationErrorKind::Enum { .. } | ValidationErrorKind::Constant { .. } => {
            "expected a declared option".into()
        }
        ValidationErrorKind::UniqueItems => "items must be unique".into(),
        ValidationErrorKind::Pattern { .. } | ValidationErrorKind::Format { .. } => {
            "does not match the declared format".into()
        }
        _ => "does not match the declared parameter schema".into(),
    }
}

#[async_trait]
impl Tool for Validated {
    fn name(&self) -> &str {
        self.tool.name()
    }
    fn description(&self) -> &str {
        self.tool.description()
    }
    fn declaration(&self) -> Value {
        self.tool.declaration()
    }
    fn enhanced_description(&self) -> String {
        self.tool.enhanced_description()
    }
    fn parameters_schema(&self) -> Option<Value> {
        self.schema.clone()
    }
    fn response_schema(&self) -> Option<Value> {
        self.tool.response_schema()
    }
    fn required_scopes(&self) -> &[&str] {
        self.tool.required_scopes()
    }
    fn annotations(&self) -> Option<&Value> {
        self.tool.annotations()
    }
    fn origin(&self) -> &str {
        self.tool.origin()
    }
    fn effect(&self) -> adk_core::ToolEffect {
        self.tool.effect()
    }
    fn timeout_override(&self) -> Option<Option<std::time::Duration>> {
        self.tool.timeout_override()
    }
    fn is_long_running(&self) -> bool {
        self.tool.is_long_running()
    }
    fn is_builtin(&self) -> bool {
        self.tool.is_builtin()
    }
    fn is_read_only(&self) -> bool {
        self.tool.is_read_only()
    }
    fn is_concurrency_safe(&self) -> bool {
        self.tool.is_concurrency_safe()
    }
    fn is_agent_delegation(&self) -> bool {
        self.tool.is_agent_delegation()
    }
    async fn execute(
        &self,
        context: Arc<dyn ToolContext>,
        arguments: Value,
    ) -> adk_core::Result<Value> {
        if let Some(timeline) = &self.timeline {
            timeline.forget_failure(context.branch(), context.function_call_id(), self.name());
        }
        let result = async {
            if context.session_id() != self.session {
                return Err(AdkError::tool("tool belongs to another session"));
            }
            if context.is_cancelled() || self.stop.is_cancelled() {
                return Err(AdkError::tool("tool execution was cancelled"));
            }
            if let Some(error) = self.invalid(&arguments) {
                if let Some(timeline) = &self.timeline {
                    timeline.fail_tool(
                        context.branch(),
                        context.function_call_id(),
                        self.name(),
                        Some(error.code),
                    );
                }
                return Ok(json!({"isError":true,"error":error}));
            }
            match &self.timeline {
                Some(timeline) => {
                    timeline
                        .tool(
                            self.tool.is_agent_delegation(),
                            self.tool.execute(context.clone(), arguments),
                        )
                        .await
                }
                None => self.tool.execute(context.clone(), arguments).await,
            }
        }
        .await;
        if let Err(error) = &result
            && let Some(timeline) = &self.timeline
        {
            let code = std::error::Error::source(error)
                .and_then(|source| source.downcast_ref::<Fault>())
                .map(|fault| fault.code)
                .or_else(|| {
                    (error.category == adk_core::ErrorCategory::Cancelled
                        || context.is_cancelled()
                        || self.stop.is_cancelled())
                    .then_some(sailry_protocol::ErrorCode::Cancelled)
                });
            timeline.fail_tool(
                context.branch(),
                context.function_call_id(),
                self.name(),
                code,
            );
        }
        result
    }
}
