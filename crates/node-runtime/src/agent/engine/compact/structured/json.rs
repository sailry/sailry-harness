//! Derived from Goose 14741e9a, goose-context-management/src/structured.rs
//! and goose-provider-types/src/json.rs.
//! Apache-2.0; see third_party_licenses/goose-Apache-2.0.txt.
use serde_json::Value;

pub(super) fn candidates(text: &str) -> Vec<&str> {
    const TERMINATOR: &str = "</analysis>";
    let mut cuts: Vec<_> = text
        .match_indices(TERMINATOR)
        .map(|(index, _)| index + TERMINATOR.len())
        .collect();
    if cuts.is_empty() {
        cuts.push(0);
    }
    let mut candidates = Vec::new();
    for cut in cuts.into_iter().rev() {
        let tail = text.get(cut..).unwrap();
        let later = tail.matches(TERMINATOR).count();
        candidates.extend(
            fences(tail)
                .chain(object(tail))
                .filter(|candidate| candidate.matches(TERMINATOR).count() == later),
        );
    }
    candidates.extend(object(text));
    candidates.dedup();
    candidates
}

fn fences(text: &str) -> impl Iterator<Item = &str> {
    text.match_indices("```json")
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .filter_map(|(index, marker)| object(text.get(index + marker.len()..).unwrap()))
}

fn object(text: &str) -> Option<&str> {
    let text = text.trim_start();
    if !text.starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return text.get(..=index);
                }
            }
            _ => {}
        }
    }
    None
}

pub(super) fn value(text: &str) -> Result<Value, serde_json::Error> {
    if let Ok(value) = serde_json::from_str(text) {
        return Ok(value);
    }
    for candidate in [repair(text), escape(text)] {
        if let Ok(value) = serde_json::from_str(&candidate) {
            return Ok(value);
        }
    }
    serde_json::from_str(&repair(&escape(text)))
}

fn repair(text: &str) -> String {
    let mut repaired = String::with_capacity(text.len() + 8);
    let mut in_string = false;
    let mut escaped = false;
    let mut closers = Vec::new();
    for character in text.chars() {
        repaired.push(character);
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match character {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => closers.push('}'),
            '[' => closers.push(']'),
            '}' | ']' if closers.last() == Some(&character) => {
                closers.pop();
            }
            _ => {}
        }
    }
    if in_string {
        if escaped {
            repaired.push('\\');
        }
        repaired.push('"');
    }
    while let Some(closer) = closers.pop() {
        repaired.push(closer);
    }
    repaired
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\u{0000}'..='\u{001F}' => match character {
                '\u{0008}' => escaped.push_str("\\b"),
                '\u{000C}' => escaped.push_str("\\f"),
                '\n' => escaped.push_str("\\n"),
                '\r' => escaped.push_str("\\r"),
                '\t' => escaped.push_str("\\t"),
                _ => escaped.push_str(&format!("\\u{:04x}", character as u32)),
            },
            _ => escaped.push(character),
        }
    }
    escaped
}
