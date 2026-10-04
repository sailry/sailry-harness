//! Inspect controlled CREATE TABLE clauses; SQLite remains the SQL parser.
use crate::Error;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Table {
    pub(super) columns: BTreeMap<String, String>,
    pub(super) constraints: BTreeSet<String>,
}

impl Table {
    pub(super) fn parse(sql: &str) -> Result<Self, Error> {
        let sql = strip_comments(sql);
        let start = sql.find('(').ok_or_else(invalid)?;
        let end = sql.rfind(')').ok_or_else(invalid)?;
        let mut columns = BTreeMap::new();
        let mut constraints = BTreeSet::new();
        for clause in clauses(&sql[start + 1..end])? {
            let first = clause.split_whitespace().next().ok_or_else(invalid)?;
            let keyword = first.split('(').next().unwrap().to_ascii_uppercase();
            if matches!(
                keyword.as_str(),
                "PRIMARY" | "FOREIGN" | "UNIQUE" | "CHECK" | "CONSTRAINT"
            ) {
                constraints.insert(normalize(&clause));
            } else {
                let name = first.trim_matches(['"', '\x60', '[', ']']).to_owned();
                if columns.insert(name, clause).is_some() {
                    return Err(invalid());
                }
            }
        }
        Ok(Self {
            columns,
            constraints,
        })
    }
}

fn clauses(body: &str) -> Result<Vec<String>, Error> {
    let mut result = vec![];
    let mut depth = 0usize;
    let mut quote = None;
    let mut start = 0;
    let mut chars = body.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if let Some(delimiter) = quote {
            if ch == delimiter {
                if chars.peek().is_some_and(|(_, next)| *next == delimiter) {
                    chars.next();
                } else {
                    quote = None;
                }
            }
            continue;
        }
        match ch {
            '\'' | '"' | '\x60' => quote = Some(ch),
            '[' => quote = Some(']'),
            '(' => depth += 1,
            ')' => depth = depth.checked_sub(1).ok_or_else(invalid)?,
            ',' if depth == 0 => {
                result.push(body[start..index].trim().to_owned());
                start = index + 1;
            }
            _ => {}
        }
    }
    if depth != 0 || quote.is_some() {
        return Err(invalid());
    }
    result.push(body[start..].trim().to_owned());
    Ok(result)
}

pub(super) fn normalize(sql: &str) -> String {
    let sql = strip_comments(sql);
    let mut result = String::new();
    let mut quote = None;
    let mut space = false;
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(delimiter) = quote {
            result.push(ch);
            if ch == delimiter {
                if chars.peek() == Some(&delimiter) {
                    result.push(chars.next().unwrap());
                } else {
                    quote = None;
                }
            }
        } else if matches!(ch, '\'' | '"' | '\x60' | '[') {
            quote = Some(if ch == '[' { ']' } else { ch });
            result.push(ch);
        } else if ch.is_whitespace() {
            space = true;
        } else if ch != ';' {
            if space
                && ch.is_alphanumeric()
                && result
                    .chars()
                    .last()
                    .is_some_and(|last| last.is_alphanumeric() || last == '_')
            {
                result.push(' ');
            }
            result.extend(ch.to_uppercase());
            space = false;
        }
    }
    result
}

fn strip_comments(sql: &str) -> String {
    let mut result = String::new();
    let mut quote = None;
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(delimiter) = quote {
            result.push(ch);
            if ch == delimiter {
                if chars.peek() == Some(&delimiter) {
                    result.push(chars.next().unwrap());
                } else {
                    quote = None;
                }
            }
        } else if matches!(ch, '\'' | '"' | '\x60' | '[') {
            quote = Some(if ch == '[' { ']' } else { ch });
            result.push(ch);
        } else if ch == '-' && chars.peek() == Some(&'-') {
            chars.next();
            for next in chars.by_ref() {
                if next == '\n' {
                    break;
                }
            }
            result.push(' ');
        } else if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(next) = chars.next() {
                if next == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
            result.push(' ');
        } else {
            result.push(ch);
        }
    }
    result
}

fn invalid() -> Error {
    Error::Worker(
        "database table declaration could not be inspected; existing data was not changed".into(),
    )
}
