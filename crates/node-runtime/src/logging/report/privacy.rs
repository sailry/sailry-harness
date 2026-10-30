use super::super::format;
use crate::telemetry::Metadata;
use sentry::protocol::{
    AppleDebugImage, Breadcrumb, Context, DebugImage, DebugMeta, Event, Exception, Frame, Level,
    Mechanism, Stacktrace, SymbolicDebugImage,
};
use serde_json::{Map, Value};
use std::{
    borrow::Cow,
    collections::HashMap,
    path::{Component, Path},
    sync::Mutex,
    time::{Duration, Instant},
};

pub(super) fn breadcrumb(breadcrumb: Breadcrumb) -> Option<Breadcrumb> {
    if breadcrumb.level != Level::Warning || breadcrumb.category.as_deref() != Some("sailry") {
        return None;
    }
    let fields = format::remote(&breadcrumb.data.into_iter().collect());
    if fields.is_empty() {
        return None;
    }
    Some(Breadcrumb {
        timestamp: breadcrumb.timestamp,
        category: Some("sailry".into()),
        ty: "log".into(),
        level: Level::Warning,
        data: fields.into_iter().collect(),
        ..Default::default()
    })
}

pub(super) fn event(
    event: Event<'static>,
    metadata: &Metadata,
    explicit: bool,
) -> Option<Event<'static>> {
    let fields: Map<String, Value> = match event.contexts.get("Rust Tracing Fields") {
        Some(Context::Other(fields)) => fields.clone().into_iter().collect(),
        _ => Map::new(),
    };
    let panic = event.exception.iter().any(|exception| {
        exception.ty == "panic"
            && exception
                .mechanism
                .as_ref()
                .is_some_and(|mechanism| mechanism.ty == "panic")
    });
    if !explicit
        && !panic
        && !(event.logger.as_deref() == Some("sailry")
            && event.level == Level::Error
            && fields.get("phase").and_then(Value::as_str) == Some("failed"))
    {
        return None;
    }
    let fields = format::remote(&fields);
    let message = if explicit {
        "runtime.diagnostics:requested".into()
    } else if panic {
        "runtime.panic:internal".into()
    } else {
        format!(
            "{}.{}:{}",
            fields
                .get("area")
                .and_then(Value::as_str)
                .unwrap_or("runtime"),
            fields
                .get("operation")
                .and_then(Value::as_str)
                .unwrap_or("operation"),
            fields
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("internal"),
        )
    };
    let stacktrace = event
        .exception
        .iter()
        .find_map(|exception| exception.stacktrace.as_ref())
        .or_else(|| {
            event
                .threads
                .iter()
                .find_map(|thread| thread.stacktrace.as_ref())
        })
        .or(event.stacktrace.as_ref())
        .map(stack);
    let debug_meta = images(&event.debug_meta);
    let mut fingerprint = blake3::Hasher::new();
    fingerprint.update(if explicit {
        b"diagnostic"
    } else if panic {
        b"panic"
    } else {
        b"operation"
    });
    for key in [
        "area",
        "code",
        "operation",
        "stage",
        "reason",
        "cause_code",
        "package",
        "plugin_version",
    ] {
        if let Some(value) = fields.get(key) {
            fingerprint.update(key.as_bytes());
            fingerprint.update(value.to_string().as_bytes());
        }
    }
    if let Some(stacktrace) = &stacktrace {
        for frame in &stacktrace.frames {
            for value in [&frame.module, &frame.function, &frame.filename]
                .into_iter()
                .flatten()
            {
                fingerprint.update(value.as_bytes());
            }
            fingerprint.update(&frame.lineno.unwrap_or_default().to_le_bytes());
            if frame.function.is_none()
                && let Some((image, offset)) = location(frame, &debug_meta)
            {
                fingerprint.update(b"native\0");
                fingerprint.update(image.as_bytes());
                fingerprint.update(&offset.to_le_bytes());
            }
        }
    }
    let mut safe = Event {
        event_id: event.event_id,
        timestamp: event.timestamp,
        level: if explicit {
            Level::Info
        } else if panic {
            Level::Fatal
        } else {
            Level::Error
        },
        logger: Some("sailry".into()),
        platform: "native".into(),
        message: Some(message),
        release: Some(metadata.release.clone().into()),
        environment: Some(metadata.channel.into()),
        fingerprint: vec![fingerprint.finalize().to_hex().to_string().into()].into(),
        extra: fields.into_iter().collect(),
        breadcrumbs: event
            .breadcrumbs
            .into_iter()
            .filter_map(breadcrumb)
            .take(50)
            .collect::<Vec<_>>()
            .into(),
        debug_meta: Cow::Owned(debug_meta),
        ..Default::default()
    };
    if !explicit {
        safe.exception = vec![Exception {
            ty: if panic { "panic" } else { "SailryError" }.into(),
            stacktrace,
            mechanism: Some(Mechanism {
                ty: if panic { "panic" } else { "tracing" }.into(),
                handled: Some(!panic),
                ..Default::default()
            }),
            ..Default::default()
        }]
        .into();
    }
    safe.tags = [
        ("installation_id", metadata.installation_id.as_str()),
        ("channel", metadata.channel),
        ("application", metadata.application),
        ("os", metadata.os),
        ("arch", metadata.arch),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.into()))
    .collect();
    Some(safe)
}

fn stack(trace: &Stacktrace) -> Stacktrace {
    Stacktrace {
        frames: trace
            .frames
            .iter()
            .skip(trace.frames.len().saturating_sub(128))
            .map(|frame| Frame {
                module: frame
                    .module
                    .as_deref()
                    .filter(|value| symbol(value))
                    .map(str::to_owned),
                function: frame
                    .function
                    .as_deref()
                    .filter(|value| symbol(value) && *value != "<unknown>")
                    .map(str::to_owned),
                filename: frame
                    .abs_path
                    .as_deref()
                    .and_then(filename)
                    .or_else(|| frame.filename.as_deref().and_then(filename)),
                lineno: frame.lineno,
                instruction_addr: frame.instruction_addr,
                image_addr: frame.image_addr,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

fn images(metadata: &DebugMeta) -> DebugMeta {
    DebugMeta {
        // Sentry's project symbol index uses debug and code IDs. Image paths
        // are only needed by external symbol servers and are never reported.
        images: metadata
            .images
            .iter()
            .filter_map(|image| match image {
                DebugImage::Symbolic(image) => Some(DebugImage::Symbolic(SymbolicDebugImage {
                    name: "native".into(),
                    arch: None,
                    image_addr: image.image_addr,
                    image_size: image.image_size,
                    image_vmaddr: image.image_vmaddr,
                    id: image.id,
                    code_id: image
                        .code_id
                        .clone()
                        .filter(|id| !id.is_nil() && id.as_str().len() <= 128),
                    debug_file: None,
                })),
                DebugImage::Apple(image) => Some(DebugImage::Apple(AppleDebugImage {
                    name: "native".into(),
                    arch: None,
                    cpu_type: None,
                    cpu_subtype: None,
                    image_addr: image.image_addr,
                    image_size: image.image_size,
                    image_vmaddr: image.image_vmaddr,
                    uuid: image.uuid,
                })),
                DebugImage::Proguard(_) | DebugImage::Wasm(_) => None,
            })
            .collect(),
        ..Default::default()
    }
}

fn location(frame: &Frame, metadata: &DebugMeta) -> Option<(String, u64)> {
    let instruction = frame.instruction_addr?.0;
    metadata.images.iter().find_map(|image| {
        let (start, size, identity) = match image {
            DebugImage::Symbolic(image) => (
                image.image_addr,
                image.image_size,
                if !image.id.is_nil() {
                    format!("debug:{}", image.id)
                } else {
                    format!("code:{}", image.code_id.as_ref()?)
                },
            ),
            DebugImage::Apple(image) if !image.uuid.is_nil() => (
                image.image_addr,
                image.image_size,
                format!("debug:{}", image.uuid),
            ),
            _ => return None,
        };
        if frame.image_addr.is_some_and(|address| address != start) {
            return None;
        }
        let offset = instruction.checked_sub(start.0)?;
        (offset < size).then_some((identity, offset))
    })
}

fn symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b" _.:$<>{},()&;[]?!'*+-".contains(&byte))
}

fn filename(value: &str) -> Option<String> {
    let path = Path::new(value);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2)?;
    let path = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    if value.len() > 512
        || value.contains('\\')
        || value.contains(':')
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let path = path.to_str()?;
    let path = if path.starts_with("crates/") || path.starts_with("apps/") {
        path.splitn(3, '/').nth(2)?
    } else {
        path
    };
    if path.is_empty() || path.chars().any(char::is_control) {
        return None;
    }
    Some(path.into())
}

#[derive(Default)]
pub(super) struct Limiter(Mutex<HashMap<String, (Instant, u8)>>);

impl Limiter {
    pub(super) fn admit(&self, event: &Event<'_>) -> bool {
        self.at(event, Instant::now())
    }

    fn at(&self, event: &Event<'_>, now: Instant) -> bool {
        let Some(fingerprint) = event.fingerprint.first() else {
            return false;
        };
        let Ok(mut entries) = self.0.lock() else {
            return false;
        };
        entries.retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(3600));
        if entries.len() >= 1024 && !entries.contains_key(fingerprint.as_ref()) {
            return false;
        }
        let (_, count) = entries.entry(fingerprint.to_string()).or_insert((now, 0));
        if *count >= 10 {
            return false;
        }
        *count += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::Control;
    use serde_json::json;

    fn unresolved(base: u64, offset: u64, known_image: bool) -> Event<'static> {
        serde_json::from_value(json!({
            "exception":{"values":[{
                "type":"panic", "mechanism":{"type":"panic"},
                "stacktrace":{"frames":[{
                    "instruction_addr":format!("0x{:x}", base + offset),
                    "image_addr":known_image.then(|| format!("0x{base:x}"))
                }]}
            }]},
            "debug_meta":{"images":[{
                "type":"symbolic", "name":"/Users/PRIVATE_USER/application.exe",
                "image_addr":format!("0x{base:x}"), "image_size":4096,
                "id":"4c181d40-80d9-489d-a61b-c8770a510cc1-3",
                "code_id":"b81f58452fc52cf6fb163353f727c31b5de732d4"
            }]}
        }))
        .unwrap()
    }

    #[test]
    fn distinguishes_native_instruction_offsets() {
        let metadata = Control::fixture(true, true).metadata();
        for known_image in [true, false] {
            let first =
                super::event(unresolved(0x1000, 0x10, known_image), &metadata, false).unwrap();
            let second =
                super::event(unresolved(0x1000, 0x20, known_image), &metadata, false).unwrap();
            assert_ne!(first.fingerprint, second.fingerprint);
        }
    }

    #[test]
    fn preserves_grouping_after_image_relocation() {
        let metadata = Control::fixture(true, true).metadata();
        for known_image in [true, false] {
            let first =
                super::event(unresolved(0x1000, 0x10, known_image), &metadata, false).unwrap();
            let relocated =
                super::event(unresolved(0x80000000, 0x10, known_image), &metadata, false).unwrap();
            assert_eq!(first.fingerprint, relocated.fingerprint);
        }
    }

    #[test]
    fn distinguishes_outer_native_callers() {
        let metadata = Control::fixture(true, true).metadata();
        for placeholder in [None, Some("<unknown>")] {
            let mut first = unresolved(0x1000, 0x10, false);
            let mut second = unresolved(0x1000, 0x20, false);
            for event in [&mut first, &mut second] {
                let frames = &mut event.exception[0].stacktrace.as_mut().unwrap().frames;
                frames[0].function = placeholder.map(str::to_owned);
                frames.extend(vec![
                    Frame {
                        function: placeholder.map(str::to_owned),
                        instruction_addr: Some(0x1180_u64.into()),
                        ..Default::default()
                    };
                    12
                ]);
            }
            let first = super::event(first, &metadata, false).unwrap();
            let second = super::event(second, &metadata, false).unwrap();
            assert_ne!(first.fingerprint, second.fingerprint);
        }
    }

    #[test]
    fn keeps_newest_frames_without_private_fields() {
        let trace = Stacktrace {
            frames: (0..140)
                .map(|line| Frame {
                    function: Some(format!("sailry_node_runtime::frame_{line}")),
                    filename: Some("src/worker.rs".into()),
                    lineno: Some(line),
                    instruction_addr: Some((0x1000 + line).into()),
                    abs_path: Some("/Users/PRIVATE_USER/project/src/worker.rs".into()),
                    context_line: Some("PRIVATE_SOURCE".into()),
                    package: Some("C:\\Users\\PRIVATE_USER\\project.exe".into()),
                    vars: [("PRIVATE_VARIABLE".into(), json!("PRIVATE_VALUE"))]
                        .into_iter()
                        .collect(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let safe = stack(&trace);
        assert_eq!(safe.frames.len(), 128);
        assert_eq!(safe.frames[0].lineno, Some(12));
        assert_eq!(safe.frames[127].lineno, Some(139));
        assert_eq!(
            safe.frames[127].function.as_deref(),
            Some("sailry_node_runtime::frame_139")
        );
        let serialized = serde_json::to_string(&safe).unwrap();
        assert!(!serialized.contains("PRIVATE_"));
        assert!(!serialized.contains("/Users/"));
        assert!(!serialized.contains("C:\\"));
    }

    #[test]
    fn retains_native_identifiers_without_paths() {
        let id = "4c181d40-80d9-489d-a61b-c8770a510cc1-3";
        let uuid = "8b9964e7-9eaf-4aac-8bca-3b656a4bd355";
        let code = "b81f58452fc52cf6fb163353f727c31b5de732d4";
        let event: Event<'static> = serde_json::from_value(json!({
            "platform": "PRIVATE_PLATFORM",
            "message": "PRIVATE_MESSAGE",
            "exception": {"values":[{
                "type":"panic", "value":"PRIVATE_SECRET",
                "mechanism":{"type":"panic"},
                "stacktrace":{"frames":[{
                    "instruction_addr":"0x1010", "image_addr":"0x1000",
                    "symbol_addr":"0x1004", "addr_mode":"PRIVATE_MODE",
                    "abs_path":"/Users/PRIVATE_USER/private.rs",
                    "context_line":"PRIVATE_SOURCE"
                }]}
            }]},
            "debug_meta": {
                "sdk_info":{"sdk_name":"PRIVATE_SDK","version_major":1,"version_minor":2,"version_patchlevel":3},
                "images":[
                    {"type":"symbolic", "name":"C:\\Users\\PRIVATE_USER\\private.exe",
                     "arch":"PRIVATE_ARCH", "image_addr":"0x1000", "image_size":4096,
                     "image_vmaddr":"0x100000000", "id":id, "code_id":code,
                     "debug_file":"https://PRIVATE_HOST/private.pdb?token=PRIVATE_KEY"},
                    {"type":"apple", "name":"/Users/PRIVATE_USER/private.dylib",
                     "arch":"PRIVATE_ARCH", "cpu_type":123, "cpu_subtype":456,
                     "image_addr":"0x2000", "image_size":8192, "uuid":uuid},
                    {"type":"wasm", "name":"PRIVATE_WASM", "debug_id":uuid,
                     "code_file":"https://PRIVATE_HOST/private.wasm?token=PRIVATE_KEY"},
                    {"type":"proguard", "uuid":uuid}
                ]
            }
        })).unwrap();
        let safe = serde_json::to_value(
            super::event(event, &Control::fixture(true, true).metadata(), false).unwrap(),
        )
        .unwrap();
        let serialized = safe.to_string();
        for forbidden in [
            "PRIVATE_",
            "C:\\",
            "/Users/",
            "https://",
            "debug_file",
            "sdk_info",
        ] {
            assert!(!serialized.contains(forbidden), "{serialized}");
        }
        assert_eq!(safe["platform"], "native");
        assert_eq!(
            safe["exception"]["values"][0]["stacktrace"]["frames"][0],
            json!({"instruction_addr":"0x1010", "image_addr":"0x1000"})
        );
        let images = safe["debug_meta"]["images"].as_array().unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0]["name"], "native");
        assert_eq!(images[0]["id"], id);
        assert_eq!(images[0]["code_id"], code);
        assert_eq!(images[0]["image_addr"], "0x1000");
        assert_eq!(images[0]["image_size"], 4096);
        assert_eq!(images[0]["image_vmaddr"], "0x100000000");
        assert!(images[0]["arch"].is_null());
        assert_eq!(images[1]["name"], "native");
        assert_eq!(images[1]["uuid"], uuid);
        assert_eq!(images[1]["image_addr"], "0x2000");
        assert!(images[1]["cpu_type"].is_null());
        assert!(images[1]["cpu_subtype"].is_null());
    }

    #[test]
    fn expires_hourly_limits() {
        let limiter = Limiter::default();
        let event = Event {
            fingerprint: vec!["synthetic".into()].into(),
            ..Default::default()
        };
        let started = Instant::now();
        for _ in 0..10 {
            assert!(limiter.at(&event, started));
        }
        assert!(!limiter.at(&event, started + Duration::from_secs(3599)));
        assert!(limiter.at(&event, started + Duration::from_secs(3600)));
    }

    #[test]
    fn keeps_crate_relative_locations() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/logging.rs");
        assert_eq!(
            filename(path.to_str().unwrap()).as_deref(),
            Some("src/logging.rs")
        );
        for path in [
            "/Users/private/src/main.rs",
            "../private.rs",
            "C:\\Users\\private.rs",
        ] {
            assert!(filename(path).is_none());
        }
    }
}
