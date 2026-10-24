//! Inspect controlled CREATE TABLE clauses; Turso remains the SQL parser.
use crate::Error;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Table {
    pub(super) header: String,
    pub(super) columns: BTreeMap<String, String>,
    pub(super) constraints: BTreeSet<String>,
    pub(super) options: String,
}

impl Table {
    pub(super) fn parse(sql: &str) -> Result<Self, Error> {
        let sql = strip_comments(sql);
        let start = sql.find('(').ok_or_else(invalid)?;
        let end = sql.rfind(')').ok_or_else(invalid)?;
        let mut columns = BTreeMap::new();
        let mut constraints = BTreeSet::new();
        let mut references = Vec::new();
        for clause in clauses(&sql[start + 1..end])? {
            let first = clause.split_whitespace().next().ok_or_else(invalid)?;
            let keyword = first.split('(').next().unwrap().to_ascii_uppercase();
            if matches!(
                keyword.as_str(),
                "PRIMARY" | "FOREIGN" | "UNIQUE" | "CHECK" | "CONSTRAINT"
            ) {
                if keyword == "FOREIGN" {
                    references.push(normalize(&clause));
                } else {
                    constraints.insert(normalize(&clause));
                }
            } else {
                let name = first.trim_matches(['"', '\x60', '[', ']']).to_owned();
                if columns.insert(name, clause).is_some() {
                    return Err(invalid());
                }
            }
        }
        // Native ALTER moves inline foreign keys to table constraints.
        for reference in references {
            if let Some((field, clause)) = reference
                .strip_prefix("FOREIGN KEY(")
                .and_then(|reference| reference.split_once(')'))
                && !field.contains(',')
                && clause.starts_with("REFERENCES ")
                && let Some((_, declaration)) = columns.iter_mut().find(|(name, _)| {
                    normalize(name) == normalize(field.trim_matches(['"', '\x60', '[', ']']))
                })
            {
                declaration.push(' ');
                declaration.push_str(clause);
            } else {
                constraints.insert(reference);
            }
        }
        Ok(Self {
            header: normalize(&sql[..start]),
            columns,
            constraints,
            options: normalize(&sql[end + 1..]),
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
    for (conditional, canonical) in [
        ("CREATE TABLE IF NOT EXISTS", "CREATE TABLE"),
        ("CREATE INDEX IF NOT EXISTS", "CREATE INDEX"),
        ("CREATE UNIQUE INDEX IF NOT EXISTS", "CREATE UNIQUE INDEX"),
    ] {
        if let Some(rest) = result.strip_prefix(conditional) {
            return format!("{canonical}{rest}");
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

/// Names come only from the application-owned DDL, never an untrusted SQL fragment.
pub(super) fn table_names(ddl: &str) -> Result<Vec<String>, Error> {
    let mut names = Vec::new();
    for statement in strip_comments(ddl).split(';') {
        let statement = statement.trim();
        if !statement.to_ascii_uppercase().starts_with("CREATE TABLE ") {
            continue;
        }
        let table = Table::parse(statement)?;
        let name = table.header.split_whitespace().last().ok_or_else(invalid)?;
        names.push(
            name.trim_matches(['"', '\x60', '[', ']'])
                .to_ascii_lowercase(),
        );
    }
    Ok(names)
}
