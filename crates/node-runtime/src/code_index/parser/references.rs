//! The bundled tags query finds references on the shared syntax tree. Each
//! reference records its innermost enclosing indexed declaration.
use super::{imports, *};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tree_sitter::{Query, QueryCursor, StreamingIterator, Tree};
use tree_sitter_language_pack::ImportInfo;

const MAX_REFERENCES: usize = 8192;
const MAX_ITEMS: usize = 64;
const MAX_REFERENCE_NAME: usize = 256;
/// Calls the bundled queries omit. An addition that does not compile against
/// the shipped grammar is skipped; the bundled query still applies.
const ADDITIONS: &[(&str, &[&str])] = &[
    (
        "rust",
        &[
            "(call_expression function: (scoped_identifier name: (identifier) @name)) @reference.call",
            "(call_expression function: (generic_function function: (identifier) @name)) @reference.call",
        ],
    ),
    (
        "c",
        &[
            "(call_expression function: (identifier) @name) @reference.call",
            "(call_expression function: (field_expression field: (field_identifier) @name)) @reference.call",
        ],
    ),
    (
        "cpp",
        &[
            "(call_expression function: (identifier) @name) @reference.call",
            "(call_expression function: (field_expression field: (field_identifier) @name)) @reference.call",
            "(call_expression function: (qualified_identifier name: (identifier) @name)) @reference.call",
        ],
    ),
];

/// One reference occurrence. Bytes and columns are UTF-8 offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Usage {
    pub name: String,
    pub kind: String,
    pub line: u32,
    pub column: u32,
    pub start: u32,
    /// Written through `self` or `this`.
    pub receiver: bool,
    pub scope: Option<Scope>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Scope {
    pub name: String,
    pub kind: String,
    pub start_line: u32,
    pub end_line: u32,
}

/// Compiled tags queries per language, shared by every parse of this Node.
#[derive(Default)]
pub(crate) struct Tags(Mutex<BTreeMap<String, Option<Arc<Query>>>>);

impl Tags {
    fn query(&self, parser: &Parser, language: &str) -> Result<Option<Arc<Query>>, Fault> {
        if let Some(query) = self.0.lock().unwrap().get(language) {
            return Ok(query.clone());
        }
        let compiled = compile(parser, language)?.map(Arc::new);
        self.0
            .lock()
            .unwrap()
            .insert(language.to_owned(), compiled.clone());
        Ok(compiled)
    }
}

pub(super) fn supported(language: &str) -> bool {
    get_tags_query(language).is_some()
}

pub(super) fn extract(
    parser: &Parser,
    parsed: &mut Parsed,
    source: &str,
    language: &str,
    tree: Option<&Tree>,
    imported: &[ImportInfo],
    stop: &CancellationToken,
) -> Result<(), Fault> {
    parsed.imports = imports::modules(language, &parsed.path, imported);
    let aliases = imports::aliases(imported);
    let mut found = items(imported);
    if let Some(query) = parser.tags.query(parser, language)? {
        match tree {
            Some(tree) => collect(&query, tree, source, &aliases, &mut found, stop)?,
            None => parsed.partial = true,
        }
    }
    let mut seen = BTreeSet::new();
    found.retain(|usage| seen.insert((usage.start, usage.name.clone())));
    found.truncate(MAX_REFERENCES);
    enclose(&parsed.symbols, &mut found);
    parsed.references = found;
    Ok(())
}

fn compile(parser: &Parser, language: &str) -> Result<Option<Query>, Fault> {
    let Some(own) = get_tags_query(language) else {
        return Ok(None);
    };
    let grammar = parser.registry.get_language(language).map_err(|_| {
        Fault::new(
            ErrorCode::NotConfigured,
            "bundled code parser is unavailable",
        )
    })?;
    // TypeScript grammars extend JavaScript; their bundled tags omit its calls.
    let inherited = matches!(language, "typescript" | "tsx")
        .then(|| get_tags_query("javascript"))
        .flatten();
    let additions = ADDITIONS
        .iter()
        .filter(|(owner, _)| *owner == language)
        .flat_map(|(_, patterns)| patterns.iter().copied());
    let mut text = String::new();
    let mut query = None;
    for source in inherited.into_iter().chain([own]).chain(additions) {
        // Keep whole sources when they compile; otherwise keep each pattern
        // the shipped grammar accepts.
        let whole = [source];
        let parts = patterns(source);
        let units = if Query::new(&grammar, source).is_ok() {
            &whole[..]
        } else {
            &parts[..]
        };
        for unit in units {
            let extended = format!("{text}\n{unit}");
            if let Ok(compiled) = Query::new(&grammar, &extended) {
                text = extended;
                query = Some(compiled);
            }
        }
    }
    Ok(query)
}

/// Top-level patterns of a query source, each with its captures and predicates.
fn patterns(source: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let (mut depth, mut start, mut closed) = (0usize, None, false);
    let (mut string, mut escaped, mut comment) = (false, false, false);
    for (index, character) in source.char_indices() {
        if comment {
            comment = character != '\n';
            continue;
        }
        if string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                string = false;
            }
            continue;
        }
        match character {
            ';' => comment = true,
            '"' => string = true,
            '(' | '[' => {
                if depth == 0 && closed {
                    found.extend(start.map(|begin: usize| source[begin..index].trim()));
                    start = None;
                    closed = false;
                }
                start = start.or(Some(index));
                depth += 1;
            }
            ')' | ']' => {
                depth = depth.saturating_sub(1);
                closed |= depth == 0;
            }
            _ => {}
        }
    }
    found.extend(start.map(|begin| source[begin..].trim()));
    found
}

/// Explicitly imported names refer to their definitions from the import site.
fn items(imported: &[ImportInfo]) -> Vec<Usage> {
    let mut found = Vec::new();
    for import in imported {
        let (Ok(line), Ok(column), Ok(start)) = (
            u32::try_from(import.span.start_line),
            u32::try_from(import.span.start_column),
            u32::try_from(import.span.start_byte),
        ) else {
            continue;
        };
        for item in import.items.iter().take(MAX_ITEMS) {
            let name = imports::last(item);
            if identifier(name) {
                found.push(Usage {
                    name: name.to_owned(),
                    kind: "import".into(),
                    line,
                    column,
                    start,
                    receiver: false,
                    scope: None,
                });
            }
        }
    }
    found
}

fn collect(
    query: &Query,
    tree: &Tree,
    source: &str,
    aliases: &BTreeMap<String, String>,
    found: &mut Vec<Usage>,
    stop: &CancellationToken,
) -> Result<(), Fault> {
    let Some(name) = query.capture_index_for_name("name") else {
        return Ok(());
    };
    let names = query.capture_names();
    let bytes = source.as_bytes();
    let deadline = Instant::now() + Duration::from_millis(PARSE_MILLIS);
    let mut definitions = BTreeSet::new();
    let mut references = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), bytes);
    while let Some(matched) = matches.next() {
        check(stop)?;
        if Instant::now() >= deadline || references.len() >= MAX_REFERENCES {
            break;
        }
        let Some(node) = matched
            .captures
            .iter()
            .find(|capture| capture.index == name)
            .map(|capture| capture.node)
        else {
            continue;
        };
        let role = matched.captures.iter().find_map(|capture| {
            let role = names[capture.index as usize];
            (role.starts_with("reference.") || role.starts_with("definition.")).then_some(role)
        });
        match role.map(|role| role.split_once('.')) {
            Some(Some(("definition", _))) => {
                definitions.insert(node.start_byte());
            }
            Some(Some(("reference", kind))) => references.push((node, kind)),
            _ => {}
        }
    }
    for (node, kind) in references {
        // A definition's own name is not a reference to itself.
        if definitions.contains(&node.start_byte()) {
            continue;
        }
        let Ok(text) = node.utf8_text(bytes) else {
            continue;
        };
        let text = imports::last(text.trim());
        if !identifier(text) {
            continue;
        }
        let position = node.start_position();
        let (Ok(line), Ok(column), Ok(start)) = (
            u32::try_from(position.row),
            u32::try_from(position.column),
            u32::try_from(node.start_byte()),
        ) else {
            continue;
        };
        found.push(Usage {
            name: aliases
                .get(text)
                .cloned()
                .unwrap_or_else(|| text.to_owned()),
            kind: kind.to_owned(),
            line,
            column,
            start,
            receiver: receiver(source, node.start_byte()),
            scope: None,
        });
    }
    Ok(())
}

fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_REFERENCE_NAME
        && name.chars().all(|character| {
            character.is_alphanumeric() || matches!(character, '_' | '$' | '!' | '?' | '-')
        })
}

/// Whether the name follows a `self` or `this` member access.
fn receiver(source: &str, start: usize) -> bool {
    let Some(before) = source.get(..start) else {
        return false;
    };
    let before = before.trim_end();
    ["self.", "this.", "self::", "Self::", "self->", "this->"]
        .iter()
        .filter_map(|prefix| before.strip_suffix(*prefix))
        .any(|rest| {
            !rest
                .chars()
                .next_back()
                .is_some_and(|character| character.is_alphanumeric() || character == '_')
        })
}

/// Assigns each reference the innermost containing declaration. Declarations
/// and bindings are tried in that order, so a call in `let x = f()` belongs to
/// its function rather than to `x`.
fn enclose(symbols: &[Symbol], found: &mut [Usage]) {
    found.sort_by_key(|usage| usage.start);
    for declarations in [true, false] {
        let candidates: Vec<&Symbol> = symbols
            .iter()
            .filter(|symbol| {
                if declarations {
                    container(&symbol.kind)
                } else {
                    matches!(symbol.kind.as_str(), "variable" | "constant")
                }
            })
            .collect();
        let mut open: Vec<&Symbol> = Vec::new();
        let mut next = 0;
        for usage in found.iter_mut() {
            while let Some(&symbol) = candidates.get(next)
                && symbol.range.start_byte <= usage.start
            {
                while open
                    .last()
                    .is_some_and(|top| top.range.end_byte <= symbol.range.start_byte)
                {
                    open.pop();
                }
                open.push(symbol);
                next += 1;
            }
            while open
                .last()
                .is_some_and(|top| top.range.end_byte <= usage.start)
            {
                open.pop();
            }
            if usage.scope.is_none()
                && let Some(symbol) = open.last()
            {
                usage.scope = Some(Scope {
                    name: symbol.name.clone(),
                    kind: symbol.kind.clone(),
                    start_line: symbol.range.start_line,
                    end_line: symbol.range.end_line,
                });
            }
        }
    }
}

/// Declarations that can contain executable references.
fn container(kind: &str) -> bool {
    !matches!(
        kind,
        "variable" | "constant" | "data" | "field" | "property" | "parameter" | "import" | "label"
    )
}

#[cfg(test)]
mod tests;
