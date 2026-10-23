use serde_json::{Map, Value};
use std::fmt::{self, Write as _};
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{
    fmt::{FmtContext, FormatEvent, FormatFields, format::Writer},
    registry::LookupSpan,
};

pub(super) struct Safe<T = super::native::Native> {
    pub application: &'static str,
    pub sink: T,
}

impl Safe {
    pub fn new(application: super::Application) -> Self {
        Self {
            application: application.name(),
            sink: super::native::Native::new(application),
        }
    }
}

pub(super) trait Sink: Send + Sync {
    fn emit(&self, level: tracing::Level, area: &str, line: &str);
}

// Even an accidental payload field on an application target must not be written.
const FIELDS: &[&str] = &[
    "area",
    "phase",
    "node",
    "caller",
    "request",
    "incident",
    "plugin_version",
    "app_version",
    "platform",
    "exception",
    "session",
    "turn",
    "worktree",
    "provider",
    "provider_name",
    "model",
    "package",
    "digest",
    "settings_revision",
    "handler",
    "invocation",
    "call",
    "artifact_id",
    "revision",
    "command",
    "operation",
    "stage",
    "source_kind",
    "reason",
    "cause_code",
    "cause_component",
    "cause_category",
    "upstream_status",
    "attempt",
    "limit",
    "request_mode",
    "transport",
    "connection_reused",
    "communication_outcome",
    "execution_outcome",
    "recovery_decision",
    "wait_ms",
    "end_to_end_us",
    "rtt_ms",
    "generation",
    "sequence",
    "fallback_reason",
    "continuation",
    "stream",
    "budget",
    "parse_layer",
    "parse_category",
    "parse_cause",
    "parse_field",
    "parse_event",
    "parse_line",
    "parse_column",
    "code",
    "status",
    "duration_us",
    "first_token_us",
    "bytes",
    "count",
    "durable",
    "elapsed_us",
    "last_content_us",
];

#[derive(Default)]
struct Fields(Map<String, Value>);
impl Fields {
    fn set(&mut self, field: &Field, value: Value) {
        if FIELDS.contains(&field.name()) {
            self.0.insert(field.name().into(), value);
        }
    }

    fn format(&self, include_area: bool) -> Result<String, fmt::Error> {
        let mut text = String::new();
        for key in FIELDS {
            if !include_area && *key == "area" {
                continue;
            }
            let Some(value) = self.0.get(*key) else {
                continue;
            };
            if !text.is_empty() {
                text.push(' ');
            }
            write!(text, "{key}=")?;
            match value {
                Value::String(value)
                    if !value.chars().any(|character| {
                        character.is_whitespace() || matches!(character, '"' | '\\' | '=')
                    }) && serde_json::from_str::<Value>(value).is_err() =>
                {
                    text.push_str(value);
                }
                value => write!(text, "{value}")?,
            }
        }
        Ok(text)
    }
}
impl Visit for Fields {
    fn record_debug(&mut self, _: &Field, _: &dyn fmt::Debug) {}
    fn record_str(&mut self, field: &Field, value: &str) {
        let value: String = value
            .chars()
            .filter(|character| !character.is_control())
            .take(256)
            .collect();
        if !value.is_empty() {
            self.set(field, Value::String(value));
        }
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.set(field, value.into());
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.set(field, value.into());
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.set(field, value.into());
    }
}
impl<S, N, T> FormatEvent<S, N> for Safe<T>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
    T: Sink,
{
    fn format_event(
        &self,
        _: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let level = *event.metadata().level();
        let mut line = format!(
            "{} {level} {}",
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            self.application,
        );
        let body = fields.format(true)?;
        if !body.is_empty() {
            write!(line, " {body}")?;
        }
        // Native time, severity, subsystem and category already have dedicated metadata.
        // Both bodies still come from the same privacy allowlist and value escaping.
        let area = fields.0.get("area").and_then(Value::as_str).unwrap_or("");
        self.sink.emit(level, area, &fields.format(false)?);
        writeln!(writer, "{line}")
    }
}
