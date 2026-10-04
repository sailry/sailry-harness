//! Bounded, ephemeral Git diagnostics owned by each Node. Never part of durable receipts.
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const LIMIT: usize = 64 * 1024;
thread_local! { static CAPTURE: RefCell<Option<String>> = const { RefCell::new(None) }; }

#[derive(Clone, Default)]
pub(super) struct Journal(Arc<Mutex<BTreeMap<PathBuf, VecDeque<String>>>>);

impl Journal {
    pub(super) fn capture<T>(&self, root: &Path, run: impl FnOnce() -> T) -> T {
        CAPTURE.with(|capture| *capture.borrow_mut() = Some(String::new()));
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                CAPTURE.with(|c| {
                    c.borrow_mut().take();
                });
            }
        }
        let _reset = Reset;
        let result = run();
        let output = CAPTURE.with(|capture| capture.borrow_mut().take().unwrap_or_default());
        if !output.is_empty() {
            let mut journal = self.0.lock().unwrap_or_else(|error| error.into_inner());
            if !journal.contains_key(root) && journal.len() >= 32 {
                journal.pop_first();
            }
            let entries = journal.entry(root.to_owned()).or_default();
            entries.push_back(output);
            while entries.iter().map(String::len).sum::<usize>() > LIMIT {
                entries.pop_front();
            }
        }
        result
    }
    pub(super) fn read(&self, root: &Path) -> String {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(root)
            .map(|entries| entries.iter().cloned().collect::<Vec<_>>().join("\n"))
            .unwrap_or_default()
    }
}

pub(super) fn record(text: &str) {
    CAPTURE.with(|capture| {
        if let Some(output) = capture.borrow_mut().as_mut() {
            let text = redact(text);
            let remaining = LIMIT.saturating_sub(output.len());
            let mut end = remaining.min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            output.push_str(&text[..end]);
        }
    });
}

pub(super) fn command(args: &[String]) {
    let mut skip = false;
    let args: Vec<_> = args
        .iter()
        .map(|arg| {
            if skip {
                skip = false;
                return "<message>".to_owned();
            }
            if matches!(arg.as_str(), "--message" | "-m") {
                skip = true;
            }
            redact(arg)
        })
        .collect();
    record(&format!("> git {}\n", args.join(" ")));
}

pub(super) fn redact(text: &str) -> String {
    let mut result = String::new();
    for token in text.split_inclusive(char::is_whitespace) {
        let mut token = token.to_owned();
        if let Some(start) = token.find("://") {
            let authority_start = start + 3;
            let authority_end = token[authority_start..]
                .find(['/', '?', '#', ' ', '\n', '\r'])
                .map_or(token.len(), |i| authority_start + i);
            if let Some(at) = token[authority_start..authority_end].rfind('@') {
                token.replace_range(authority_start..authority_start + at, "<redacted>");
            }
            if let Some(query) = token.find('?') {
                let end = token.trim_end().len();
                token.replace_range(query..end, "?<redacted>");
            }
        }
        let lower = token.to_ascii_lowercase();
        if ["password=", "token=", "authorization=", "secret="]
            .iter()
            .any(|key| lower.contains(key))
            && let Some(eq) = token.find('=')
        {
            token.truncate(eq + 1);
            token.push_str("<redacted> ");
        }
        result.push_str(&token);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_credentials() {
        let output =
            redact("fatal: https://user:secret@example.com/repo?token=hidden\npassword=hidden\n");
        assert!(!output.contains("secret"));
        assert!(!output.contains("hidden"));
        assert!(output.contains("example.com"));
    }
}
