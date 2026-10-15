use serde_json::{Map, Value};
use std::fmt;
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{
    fmt::{FmtContext, FormatEvent, FormatFields, format::Writer},
    registry::LookupSpan,
};

pub(super) struct Safe {
    pub application: &'static str,
}

// Even an accidental payload field on an application target must not be written.
const FIELDS: &[&str] = &[
    "area",
    "phase",
    "node",
    "caller",
    "request",
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
impl<S, N> FormatEvent<S, N> for Safe
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut fields = Fields::default();
        event.record(&mut fields);
        fields
            .0
            .insert("timestamp".into(), chrono::Utc::now().to_rfc3339().into());
        fields
            .0
            .insert("application".into(), self.application.into());
        fields
            .0
            .insert("level".into(), event.metadata().level().as_str().into());
        writeln!(writer, "{}", Value::Object(fields.0))
    }
}
