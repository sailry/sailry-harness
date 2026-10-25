//! Extraction uses the pinned language pack; source ranges retain UTF-8 coordinates.
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, code_index::*};
use std::{collections::BTreeSet, path::Path};
use tree_sitter_language_pack::{
    DataNode, LanguageRegistry, ProcessConfig, ProcessResult, Span, StructureItem,
    detect_language_from_content, detect_language_from_extension, detect_language_from_path,
};

mod embedded;

const MAX_SYMBOLS: usize = 4096;
const MAX_NAME_BYTES: usize = 512;
const MAX_SIGNATURE_BYTES: usize = 384;
const PARSE_MILLIS: u64 = 1000;

pub(super) struct Parser {
    pub(super) registry: LanguageRegistry,
    #[cfg(test)]
    pub(super) parsed: std::sync::atomic::AtomicUsize,
}

#[derive(Clone)]
pub(super) struct Parsed {
    pub path: String,
    pub language: Option<String>,
    pub hash: Option<String>,
    pub status: Status,
    pub symbols: Vec<Symbol>,
    pub capabilities: Capabilities,
    pub partial: bool,
    pub truncated: bool,
}

impl Parsed {
    pub(super) fn omitted(path: String, status: Status) -> Self {
        Self {
            language: detect_language_from_path(&path).map(str::to_owned),
            path,
            hash: None,
            status,
            symbols: Vec::new(),
            capabilities: Capabilities::default(),
            partial: status != Status::Unsupported,
            truncated: status == Status::TooLarge,
        }
    }
}

impl Parser {
    pub(super) fn open(directory: &Path) -> Self {
        Self {
            registry: LanguageRegistry::with_libs_dir(directory.to_owned()),
            #[cfg(test)]
            parsed: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub(super) fn parse(
        &self,
        path: String,
        source: &str,
        stop: &CancellationToken,
    ) -> Result<Parsed, Fault> {
        check(stop)?;
        let hash = blake3::hash(source.as_bytes()).to_hex().to_string();
        let language = detect(&path, source);
        let mut parsed = Parsed::omitted(path, Status::Unsupported);
        parsed.hash = Some(hash);
        let Some(language) = language else {
            return Ok(parsed);
        };
        parsed.language = Some(language.to_owned());
        // A packaged grammar that cannot load is a resource failure, not an unknown language.
        self.registry.get_language(language).map_err(|_| {
            Fault::new(
                ErrorCode::NotConfigured,
                "bundled code parser is unavailable",
            )
        })?;
        #[cfg(test)]
        self.parsed
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let result = match self.process(source, language) {
            Ok(result) => result,
            Err(_) => {
                parsed.status = Status::Failed;
                parsed.partial = true;
                return Ok(parsed);
            }
        };
        parsed.status = Status::Parsed;
        parsed.partial = result.metrics.error_count != 0;
        extract(&mut parsed, result, language, source, Origin::default());
        embedded::extract(self, &mut parsed, source, language, stop)?;
        parsed.symbols.sort_by(|left, right| {
            left.range
                .start_byte
                .cmp(&right.range.start_byte)
                .then_with(|| right.range.end_byte.cmp(&left.range.end_byte))
                .then_with(|| left.name.cmp(&right.name))
        });
        let mut seen = BTreeSet::new();
        parsed.symbols.retain(|symbol| {
            seen.insert((
                symbol.name.clone(),
                symbol.range.start_byte,
                symbol.range.end_byte,
            ))
        });
        check(stop)?;
        Ok(parsed)
    }

    fn process(
        &self,
        source: &str,
        language: &str,
    ) -> Result<ProcessResult, tree_sitter_language_pack::Error> {
        let mut config = ProcessConfig::new(language.to_owned())
            .with_max_source_bytes(Some(MAX_FILE_BYTES))
            .with_parse_timeout_ms(Some(PARSE_MILLIS))
            .with_data_extraction(true);
        config.imports = false;
        config.exports = false;
        config.symbols = true;
        self.registry.process(source, &config)
    }
}

pub(super) fn candidate(path: &str) -> bool {
    detect_language_from_path(path).is_some() || Path::new(path).extension().is_none()
}

fn detect(path: &str, source: &str) -> Option<&'static str> {
    detect_language_from_path(path)
        .or_else(|| {
            Path::new(path)
                .file_name()?
                .to_str()
                .and_then(detect_language_from_extension)
        })
        .or_else(|| detect_language_from_content(source))
}

#[derive(Clone, Copy, Default)]
struct Origin {
    byte: usize,
    line: usize,
    column: usize,
}

impl Origin {
    fn range(self, span: &Span) -> Option<Range> {
        Some(Range {
            start_byte: self.byte.checked_add(span.start_byte)?.try_into().ok()?,
            end_byte: self.byte.checked_add(span.end_byte)?.try_into().ok()?,
            start_line: self.line.checked_add(span.start_line)?.try_into().ok()?,
            end_line: self.line.checked_add(span.end_line)?.try_into().ok()?,
            start_column: span
                .start_column
                .checked_add(if span.start_line == 0 { self.column } else { 0 })?
                .try_into()
                .ok()?,
            end_column: span
                .end_column
                .checked_add(if span.end_line == 0 { self.column } else { 0 })?
                .try_into()
                .ok()?,
        })
    }
}

fn extract(
    parsed: &mut Parsed,
    result: ProcessResult,
    language: &str,
    source: &str,
    origin: Origin,
) {
    parsed.capabilities.structure |= !result.structure.is_empty();
    parsed.capabilities.symbols |= !result.symbols.is_empty();
    parsed.capabilities.data |= result.data.is_some();
    parsed.partial |= result.metrics.error_count != 0;
    if let Some(data) = &result.data {
        // Data files contribute field declarations, never serialized values or containers.
        data_items(parsed, data, language, source, origin);
        return;
    }
    let mut structure: Vec<(&StructureItem, u16)> = result
        .structure
        .iter()
        .rev()
        .map(|item| (item, 0))
        .collect();
    while let Some((item, depth)) = structure.pop() {
        if let Some(name) = item.name.as_deref() {
            append(
                parsed,
                Item {
                    name,
                    kind: label(&item.kind),
                    span: &item.span,
                    signature: item.signature.as_deref(),
                    depth,
                },
                language,
                source,
                origin,
            );
        }
        if parsed.symbols.len() >= MAX_SYMBOLS {
            parsed.truncated = true;
            break;
        }
        structure.extend(
            item.children
                .iter()
                .rev()
                .map(|child| (child, depth.saturating_add(1))),
        );
    }
    for item in &result.symbols {
        append(
            parsed,
            Item {
                name: &item.name,
                kind: label(&item.kind),
                span: &item.span,
                signature: None,
                depth: 0,
            },
            language,
            source,
            origin,
        );
    }
}

struct Item<'a> {
    name: &'a str,
    kind: String,
    span: &'a Span,
    signature: Option<&'a str>,
    depth: u16,
}

fn append(parsed: &mut Parsed, item: Item<'_>, language: &str, source: &str, origin: Origin) {
    if parsed.symbols.len() >= MAX_SYMBOLS || item.name.len() > MAX_NAME_BYTES {
        parsed.truncated = true;
        return;
    }
    let Some(range) = origin.range(item.span) else {
        parsed.partial = true;
        return;
    };
    if item.span.start_byte > item.span.end_byte
        || source
            .get(item.span.start_byte..item.span.end_byte)
            .is_none()
        || item.name.is_empty()
    {
        parsed.partial = true;
        return;
    }
    let signature = item
        .signature
        .or_else(|| {
            source
                .get(item.span.start_byte..item.span.end_byte)?
                .lines()
                .next()
        })
        .map(|signature| short(signature, MAX_SIGNATURE_BYTES));
    parsed.symbols.push(Symbol {
        path: parsed.path.clone(),
        language: language.to_owned(),
        name: item.name.to_owned(),
        kind: item.kind,
        range,
        signature,
        file_revision: parsed.hash.clone().unwrap_or_default(),
        depth: item.depth,
    });
}

fn data_items(parsed: &mut Parsed, root: &DataNode, language: &str, source: &str, origin: Origin) {
    let mut nodes = vec![(root, 0u16)];
    while let Some((node, depth)) = nodes.pop() {
        if let Some(name) = node.key.as_deref() {
            let signature = format!("{name}: {}", label(&node.kind));
            append(
                parsed,
                Item {
                    name,
                    kind: "data".into(),
                    span: &node.span,
                    signature: Some(&signature),
                    depth,
                },
                language,
                source,
                origin,
            );
        }
        if parsed.symbols.len() >= MAX_SYMBOLS {
            parsed.truncated = true;
            break;
        }
        nodes.extend(
            node.children
                .iter()
                .rev()
                .map(|child| (child, depth.saturating_add(1))),
        );
    }
}

fn label(kind: &impl serde::Serialize) -> String {
    match serde_json::to_value(kind) {
        Ok(serde_json::Value::String(name)) => name.to_lowercase(),
        Ok(serde_json::Value::Object(value)) => value
            .values()
            .find_map(serde_json::Value::as_str)
            .unwrap_or("other")
            .to_lowercase(),
        _ => "other".into(),
    }
}

fn short(text: &str, limit: usize) -> String {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn check(stop: &CancellationToken) -> Result<(), Fault> {
    if stop.is_cancelled() {
        Err(Fault::new(
            ErrorCode::Cancelled,
            "code indexing was cancelled",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
