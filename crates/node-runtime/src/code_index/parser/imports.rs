//! Import statements become slash-separated module keys. Resolution prefers
//! definitions whose file or directory path ends with a key; keys are hints,
//! not a module resolver.
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter_language_pack::ImportInfo;

const MAX_KEYS: usize = 256;
const MAX_KEY_BYTES: usize = 512;
const EXTENSIONS: &[&str] = &[".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts"];

/// Module keys named by the file's imports.
pub(super) fn modules(language: &str, path: &str, imports: &[ImportInfo]) -> Vec<String> {
    let directory = path.rsplit_once('/').map_or("", |(directory, _)| directory);
    let mut keys = BTreeSet::new();
    for import in imports {
        let text = import.source.trim();
        let found = match language {
            "python" => python(text, directory, &import.items),
            "javascript" | "typescript" | "tsx" => quoted(text)
                .map(|specifier| script(specifier, directory))
                .into_iter()
                .collect(),
            "rust" => rust(text),
            "go" => quoted(text)
                .map(|specifier| specifier.trim_matches('/').to_owned())
                .into_iter()
                .collect(),
            "java" | "kotlin" => dotted(text),
            _ => Vec::new(),
        };
        keys.extend(
            found
                .into_iter()
                .filter(|key| !key.is_empty() && key.len() <= MAX_KEY_BYTES),
        );
    }
    keys.into_iter().take(MAX_KEYS).collect()
}

/// Local names bound to one differently named imported item.
pub(super) fn aliases(imports: &[ImportInfo]) -> BTreeMap<String, String> {
    imports
        .iter()
        .filter_map(|import| match (import.items.as_slice(), &import.alias) {
            ([item], Some(alias)) if item != alias => Some((alias.clone(), last(item).to_owned())),
            _ => None,
        })
        .collect()
}

/// The final segment of a qualified name.
pub(super) fn last(name: &str) -> &str {
    name.rsplit(['.', ':', '\\', '/'])
        .find(|segment| !segment.is_empty())
        .unwrap_or(name)
}

fn python(text: &str, directory: &str, items: &[String]) -> Vec<String> {
    if let Some(rest) = text.strip_prefix("from ") {
        let Some((module, _)) = rest.split_once(" import") else {
            return Vec::new();
        };
        let module = module.trim();
        let dots = module
            .chars()
            .take_while(|character| *character == '.')
            .count();
        let mut base = if dots == 0 {
            Vec::new()
        } else {
            let mut parents = segments(directory, '/');
            parents.truncate(parents.len().saturating_sub(dots - 1));
            parents
        };
        base.extend(segments(&module[dots..], '.'));
        let mut keys = vec![base.join("/")];
        for item in items.iter().take(64) {
            let mut nested = base.clone();
            nested.extend(segments(item, '.'));
            keys.push(nested.join("/"));
        }
        return keys;
    }
    let Some(rest) = text.strip_prefix("import ") else {
        return Vec::new();
    };
    rest.split(',')
        .map(|entry| {
            let name = entry.split(" as ").next().unwrap_or_default();
            segments(name.trim_matches(['(', ')', ' ']), '.').join("/")
        })
        .collect()
}

fn script(specifier: &str, directory: &str) -> String {
    let specifier = EXTENSIONS
        .iter()
        .find_map(|extension| specifier.strip_suffix(*extension))
        .unwrap_or(specifier);
    if !specifier.starts_with('.') {
        let specifier = specifier
            .strip_prefix("@/")
            .or_else(|| specifier.strip_prefix("~/"))
            .unwrap_or(specifier);
        return specifier.trim_matches('/').to_owned();
    }
    let mut resolved = segments(directory, '/');
    for segment in specifier.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                resolved.pop();
            }
            segment => resolved.push(segment),
        }
    }
    resolved.join("/")
}

fn rust(text: &str) -> Vec<String> {
    let Some((_, rest)) = text.split_once("use ") else {
        return Vec::new();
    };
    let rest = rest.trim().trim_end_matches(';').trim();
    let (prefix, group) = match rest.split_once('{') {
        Some((prefix, group)) => (prefix, Some(group.trim_end_matches('}'))),
        None => (rest, None),
    };
    let base: Vec<&str> = module(prefix.split(" as ").next().unwrap_or_default());
    let mut keys = Vec::new();
    match group {
        Some(group) => {
            keys.push(base.join("/"));
            for entry in group.split(',').take(64) {
                let entry = entry.trim_matches(|character: char| {
                    character.is_whitespace() || character == '{' || character == '}'
                });
                let mut nested = base.clone();
                nested.extend(module(entry.split(" as ").next().unwrap_or_default()));
                keys.push(nested.join("/"));
            }
        }
        None => {
            keys.push(base.join("/"));
            if base.len() > 1 {
                keys.push(base[..base.len() - 1].join("/"));
            }
        }
    }
    keys
}

/// Rust path segments without crate-relative markers or globs.
fn module(path: &str) -> Vec<&str> {
    path.split("::")
        .map(str::trim)
        .filter(|segment| {
            !segment.is_empty() && !matches!(*segment, "crate" | "self" | "super" | "*")
        })
        .collect()
}

fn dotted(text: &str) -> Vec<String> {
    let text = text.trim_end_matches(';');
    let path = text
        .split_whitespace()
        .find(|word| !matches!(*word, "import" | "static"))
        .unwrap_or_default();
    let path = path.trim_end_matches(".*");
    let parts = segments(path, '.');
    let mut keys = vec![parts.join("/")];
    if parts.len() > 1 {
        keys.push(parts[..parts.len() - 1].join("/"));
    }
    keys
}

fn quoted(text: &str) -> Option<&str> {
    let start = text.find(['"', '\'', '`'])?;
    let quote = text[start..].chars().next()?;
    let rest = &text[start + quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

fn segments(text: &str, separator: char) -> Vec<&str> {
    text.split(separator)
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter_language_pack::Span;

    fn import(source: &str, items: &[&str], alias: Option<&str>) -> ImportInfo {
        ImportInfo {
            source: source.into(),
            items: items.iter().map(|item| (*item).to_owned()).collect(),
            alias: alias.map(str::to_owned),
            is_wildcard: false,
            span: Span::default(),
        }
    }

    #[test]
    fn derives_language_module_keys() {
        for (language, path, statement, items, expected) in [
            (
                "python",
                "app/views/home.py",
                "from ..models import user",
                &["user"][..],
                &["app/models", "app/models/user"][..],
            ),
            (
                "python",
                "main.py",
                "import os.path, numpy as np",
                &[][..],
                &["numpy", "os/path"][..],
            ),
            (
                "typescript",
                "src/views/home.ts",
                "import { render } from '../lib/render.js';",
                &["render"][..],
                &["src/lib/render"][..],
            ),
            (
                "typescript",
                "src/main.ts",
                "import api from '@/services/api'",
                &[][..],
                &["services/api"][..],
            ),
            (
                "rust",
                "src/main.rs",
                "use crate::code_index::{index::Index, parser};",
                &[][..],
                &["code_index", "code_index/index/Index", "code_index/parser"][..],
            ),
            (
                "rust",
                "src/main.rs",
                "pub(crate) use super::store::Handle;",
                &[][..],
                &["store", "store/Handle"][..],
            ),
            (
                "go",
                "cmd/main.go",
                "\"example.com/project/internal/store\"",
                &[][..],
                &["example.com/project/internal/store"][..],
            ),
            (
                "java",
                "src/App.java",
                "import static com.example.util.Strings.trim;",
                &[][..],
                &["com/example/util/Strings", "com/example/util/Strings/trim"][..],
            ),
        ] {
            assert_eq!(
                modules(language, path, &[import(statement, items, None)]),
                expected,
                "{language}: {statement}"
            );
        }
    }

    #[test]
    fn maps_single_aliases_to_their_items() {
        let found = aliases(&[
            import(
                "import { render as draw } from './view'",
                &["render"],
                Some("draw"),
            ),
            import("import * as views from './view'", &[], Some("views")),
            import("from pkg.mod import helper", &["helper"], None),
        ]);
        assert_eq!(found, BTreeMap::from([("draw".into(), "render".into())]));
        assert_eq!(last("crate::index::Index"), "Index");
        assert_eq!(last("App\\Models\\User"), "User");
        assert_eq!(last("plain"), "plain");
    }
}
