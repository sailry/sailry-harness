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

// File, native, remote and diagnostic records share this single field policy.
// Local identifiers remain useful on disk but never leave the process.
#[derive(Clone, Copy)]
enum Disclosure {
    Local,
    Public,
    Version,
}
const FIELDS: &[(&str, Disclosure)] = &[
    ("area", Disclosure::Public),
    ("phase", Disclosure::Public),
    ("node", Disclosure::Local),
    ("caller", Disclosure::Local),
    ("request", Disclosure::Local),
    ("incident", Disclosure::Local),
    ("plugin_version", Disclosure::Local),
    ("app_version", Disclosure::Version),
    ("platform", Disclosure::Local),
    ("exception", Disclosure::Local),
    ("session", Disclosure::Local),
    ("turn", Disclosure::Local),
    ("worktree", Disclosure::Local),
    ("provider", Disclosure::Local),
    ("provider_name", Disclosure::Local),
    ("model", Disclosure::Local),
    ("package", Disclosure::Local),
    ("digest", Disclosure::Local),
    ("settings_revision", Disclosure::Local),
    ("handler", Disclosure::Local),
    ("invocation", Disclosure::Local),
    ("call", Disclosure::Local),
    ("artifact_id", Disclosure::Local),
    ("revision", Disclosure::Local),
    ("command", Disclosure::Local),
    ("operation", Disclosure::Public),
    ("stage", Disclosure::Local),
    ("source_kind", Disclosure::Local),
    ("reason", Disclosure::Local),
    ("cause_code", Disclosure::Public),
    ("cause_component", Disclosure::Local),
    ("cause_category", Disclosure::Local),
    ("upstream_status", Disclosure::Local),
    ("websocket_close_code", Disclosure::Local),
    ("attempt", Disclosure::Local),
    ("limit", Disclosure::Local),
    ("request_mode", Disclosure::Local),
    ("transport", Disclosure::Local),
    ("connection_reused", Disclosure::Local),
    ("communication_outcome", Disclosure::Local),
    ("execution_outcome", Disclosure::Local),
    ("recovery_decision", Disclosure::Local),
    ("wait_ms", Disclosure::Local),
    ("end_to_end_us", Disclosure::Local),
    ("rtt_ms", Disclosure::Local),
    ("generation", Disclosure::Local),
    ("sequence", Disclosure::Local),
    ("fallback_reason", Disclosure::Local),
    ("continuation", Disclosure::Local),
    ("stream", Disclosure::Local),
    ("budget", Disclosure::Local),
    ("parse_layer", Disclosure::Local),
    ("parse_category", Disclosure::Local),
    ("parse_cause", Disclosure::Local),
    ("parse_field", Disclosure::Local),
    ("parse_event", Disclosure::Local),
    ("parse_line", Disclosure::Local),
    ("parse_column", Disclosure::Local),
    ("code", Disclosure::Public),
    ("status", Disclosure::Local),
    ("duration_us", Disclosure::Public),
    ("first_token_us", Disclosure::Local),
    ("bytes", Disclosure::Local),
    ("count", Disclosure::Public),
    ("durable", Disclosure::Local),
    ("elapsed_us", Disclosure::Public),
    ("last_content_us", Disclosure::Local),
];

pub(super) fn remote(fields: &Map<String, Value>) -> Map<String, Value> {
    let mut safe: Map<String, Value> = FIELDS
        .iter()
        .filter(|(_, disclosure)| !matches!(disclosure, Disclosure::Local))
        .filter_map(|(key, disclosure)| {
            let value = fields.get(*key)?;
            match value {
                Value::String(value)
                    if match disclosure {
                        Disclosure::Version => {
                            value.len() <= 64 && semver::Version::parse(value).is_ok()
                        }
                        _ => token(value),
                    } =>
                {
                    Some(((*key).into(), value.clone().into()))
                }
                Value::Number(value)
                    if matches!(disclosure, Disclosure::Public)
                        && (value.is_u64() || value.is_i64()) =>
                {
                    Some(((*key).into(), value.clone().into()))
                }
                Value::Bool(value) if matches!(disclosure, Disclosure::Public) => {
                    Some(((*key).into(), (*value).into()))
                }
                _ => None,
            }
        })
        .collect();
    if let Some(name) = fields.get("package").and_then(Value::as_str)
        && !name.is_empty()
    {
        let version = fields.get("plugin_version").and_then(Value::as_str);
        let public = version.is_some_and(|version| {
            fields
                .get("digest")
                .and_then(Value::as_str)
                .is_some_and(|digest| crate::plugins::Host::bundled_identity(name, version, digest))
        });
        safe.insert(
            "package".into(),
            if public { name } else { "third_party" }.into(),
        );
        if public {
            safe.insert("plugin_version".into(), version.unwrap().into());
        }
    }
    safe
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.')
        })
        && !value.contains("..")
}

#[derive(Default)]
struct Fields(Map<String, Value>);
impl Fields {
    fn set(&mut self, field: &Field, value: Value) {
        if FIELDS.iter().any(|(name, _)| *name == field.name()) {
            self.0.insert(field.name().into(), value);
        }
    }

    fn format(&self, include_area: bool) -> Result<String, fmt::Error> {
        let mut text = String::new();
        for (key, _) in FIELDS {
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
                        character.is_whitespace()
                            || character.is_control()
                            || matches!(character, '"' | '\\' | '=')
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
