//! Request-only provenance is limited to a verified terminal text node.
use super::Source;
use sailry_protocol::conversation::observation::CategoryKind;
use serde_json::Value;
use std::ops::Range;

#[derive(Clone)]
pub(in crate::agent) struct Context {
    pub(super) text: String,
    pub(super) ranges: Vec<(Range<usize>, CategoryKind)>,
}

pub(super) struct Location<'a> {
    pub text: &'a str,
    pub start: usize,
    pub context: &'a Context,
}

impl Context {
    pub(in crate::agent) fn new(parts: Vec<Source>) -> Option<Self> {
        if parts.is_empty() {
            return None;
        }
        let mut context = Self {
            text: String::new(),
            ranges: Vec::new(),
        };
        context.push("<turn-context>\n", CategoryKind::Injected);
        for (index, part) in parts.into_iter().enumerate() {
            if index != 0 {
                context.push("\n\n", CategoryKind::Injected);
            }
            context.push(&part.text, part.kind);
        }
        context.push("\n</turn-context>", CategoryKind::Injected);
        Some(context)
    }

    pub(in crate::agent) fn text(&self) -> &str {
        &self.text
    }

    fn push(&mut self, text: &str, kind: CategoryKind) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        if let Some((range, previous)) = self.ranges.last_mut()
            && *previous == kind
        {
            range.end = self.text.len();
        } else {
            self.ranges.push((start..self.text.len(), kind));
        }
    }

    pub(super) fn contains(&self, kind: CategoryKind) -> bool {
        self.ranges.iter().any(|(_, source)| *source == kind)
    }

    pub(super) fn locate<'a>(&'a self, body: &'a Value) -> Option<Location<'a>> {
        let mut collections = ["messages", "contents", "input"]
            .iter()
            .filter_map(|field| body.get(field).and_then(Value::as_array));
        let messages = collections.next()?;
        if collections.next().is_some() {
            return None;
        }
        let last = messages
            .last()
            .filter(|message| message["role"] == "user")?;
        let content = last.get("content").or_else(|| last.get("parts"))?;
        let text = match content {
            Value::String(text) => text.as_str(),
            Value::Array(parts) => {
                let last = parts.last()?;
                if !matches!(last["type"].as_str(), None | Some("text" | "input_text")) {
                    return None;
                }
                last.get("text")?.as_str()?
            }
            _ => return None,
        };
        let prefix = text.strip_suffix(&self.text)?;
        if !prefix.is_empty() && !prefix.ends_with('\n') {
            return None;
        }
        Some(Location {
            text,
            start: prefix.len(),
            context: self,
        })
    }
}
