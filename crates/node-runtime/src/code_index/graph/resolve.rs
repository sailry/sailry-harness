//! Conservative name resolution. A reference prefers definitions in its own
//! file, then in files it imports, then in its directory, then anywhere in the
//! worktree. Several definitions in the chosen tier remain ambiguous.
use sailry_protocol::code_index::{Confidence, Definition, Symbol};
use std::cmp::Reverse;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super) struct Candidate {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub start_line: u32,
    pub end_line: u32,
    pub start: u32,
    pub end: u32,
    pub depth: u16,
    pub signature: Option<String>,
}

impl From<Symbol> for Candidate {
    fn from(symbol: Symbol) -> Self {
        Self {
            path: symbol.path,
            name: symbol.name,
            kind: symbol.kind,
            start_line: symbol.range.start_line,
            end_line: symbol.range.end_line,
            start: symbol.range.start_byte,
            end: symbol.range.end_byte,
            depth: symbol.depth,
            signature: symbol.signature,
        }
    }
}

impl Candidate {
    pub fn definition(&self) -> Definition {
        Definition {
            path: self.path.clone(),
            name: self.name.clone(),
            kind: self.kind.clone(),
            start_line: self.start_line,
            end_line: self.end_line,
        }
    }

    /// Identity shared by indexed declarations and reference scopes.
    pub fn key(&self) -> (&str, &str, u32) {
        (&self.path, &self.name, self.start_line)
    }
}

/// Kinds a reference can resolve to.
pub(in super::super) fn definition(kind: &str) -> bool {
    !matches!(
        kind,
        "variable" | "data" | "impl" | "field" | "property" | "parameter" | "import" | "label"
    )
}

/// Kinds whose members share a lexical owner.
fn owner(kind: &str) -> bool {
    matches!(
        kind,
        "class"
            | "struct"
            | "impl"
            | "trait"
            | "interface"
            | "enum"
            | "module"
            | "namespace"
            | "object"
            | "protocol"
            | "extension"
            | "mixin"
            | "union"
            | "record"
    )
}

pub(in super::super) struct Site<'a> {
    pub path: &'a str,
    pub start: u32,
    pub receiver: bool,
    /// Module keys imported by the referencing file.
    pub imports: &'a [String],
}

/// The strongest tier with candidates, or `None` when no definition has the name.
pub(in super::super) fn resolve<'a>(
    site: &Site<'_>,
    candidates: &'a [Candidate],
) -> Option<(Confidence, Vec<&'a Candidate>)> {
    let local = select(candidates, |candidate| candidate.path == site.path);
    if !local.is_empty() {
        return Some((Confidence::Local, local));
    }
    let imported = select(candidates, |candidate| {
        site.imports.iter().any(|key| imports(key, &candidate.path))
    });
    if !imported.is_empty() {
        return Some((Confidence::Imported, imported));
    }
    let directory = parent(site.path);
    let near = select(candidates, |candidate| parent(&candidate.path) == directory);
    if !near.is_empty() {
        return Some((Confidence::Directory, near));
    }
    (!candidates.is_empty()).then(|| (Confidence::Workspace, candidates.iter().collect()))
}

/// Narrows same-file definitions to those sharing the reference's lexical
/// owner, such as a class or impl block. A `self` or `this` reference requires
/// that owner; other references also accept top-level definitions.
pub(in super::super) fn narrow<'a>(
    site: &Site<'_>,
    targets: Vec<&'a Candidate>,
    outline: &[Candidate],
) -> Vec<&'a Candidate> {
    if targets.len() < 2 {
        return targets;
    }
    let owners: Vec<&Candidate> = outline.iter().filter(|item| owner(&item.kind)).collect();
    let home = around(&owners, site.start, site.start.saturating_add(1));
    let selected: Vec<_> = targets
        .iter()
        .copied()
        .filter(|candidate| {
            let found = around(&owners, candidate.start, candidate.end);
            found == home || (!site.receiver && found.is_none())
        })
        .collect();
    if selected.is_empty() {
        targets
    } else {
        selected
    }
}

/// Whether an import key names this file, its directory or a package path
/// ending in its directory.
pub(in super::super) fn imports(key: &str, path: &str) -> bool {
    let (directory, file) = path.rsplit_once('/').unwrap_or(("", path));
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    let named = match key.rsplit_once('/') {
        Some((prefix, last)) => last == stem && ends(directory, prefix),
        None => key == stem,
    };
    named || ends(directory, key) || (!directory.is_empty() && ends(key, directory))
}

pub(in super::super) fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

fn ends(path: &str, suffix: &str) -> bool {
    !suffix.is_empty()
        && path
            .strip_suffix(suffix)
            .is_some_and(|rest| rest.is_empty() || rest.ends_with('/'))
}

fn select<'a>(
    candidates: &'a [Candidate],
    keep: impl Fn(&Candidate) -> bool,
) -> Vec<&'a Candidate> {
    candidates
        .iter()
        .filter(|candidate| keep(candidate))
        .collect()
}

/// The innermost owner strictly containing a byte range.
fn around<'o>(owners: &[&'o Candidate], start: u32, end: u32) -> Option<&'o str> {
    owners
        .iter()
        .filter(|item| {
            item.start <= start && end <= item.end && (item.start, item.end) != (start, end)
        })
        .max_by_key(|item| (item.start, Reverse(item.end)))
        .map(|item| item.name.as_str())
}
