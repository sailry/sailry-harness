//! Verify pending and persisted turn context against exact adapted text nodes.
use super::Source;
use sailry_protocol::conversation::observation::CategoryKind;
use serde_json::Value;
use std::ops::Range;

#[derive(Clone)]
pub(in crate::agent) struct Context {
    pub(super) text: String,
    pub(super) ranges: Vec<(Range<usize>, CategoryKind)>,
    persisted: bool,
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
            persisted: false,
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

    pub(in crate::agent) fn persisted(text: String, skills: &[String]) -> Self {
        let mut ranges = skills
            .iter()
            .filter(|source| !source.is_empty())
            .flat_map(|source| {
                text.match_indices(source)
                    .map(|(start, value)| start..start + value.len())
            })
            .collect::<Vec<_>>();
        ranges.sort_by_key(|range| (range.start, std::cmp::Reverse(range.end)));
        let mut context = Self {
            text: String::new(),
            ranges: Vec::new(),
            persisted: true,
        };
        let mut end = 0;
        for range in ranges {
            if range.start < end {
                continue;
            }
            context.push(&text[end..range.start], CategoryKind::Injected);
            context.push(&text[range.clone()], CategoryKind::Skills);
            end = range.end;
        }
        context.push(&text[end..], CategoryKind::Injected);
        context
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
        if self.persisted {
            let mut located = None;
            for message in messages.iter().filter(|message| message["role"] == "user") {
                let content = message.get("content").or_else(|| message.get("parts"));
                let texts: Vec<_> = match content {
                    Some(Value::String(text)) => vec![text.as_str()],
                    Some(Value::Array(parts)) => parts
                        .iter()
                        .filter_map(|part| {
                            matches!(part["type"].as_str(), None | Some("text" | "input_text"))
                                .then(|| part["text"].as_str())
                                .flatten()
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                for text in texts {
                    for (start, matched) in text.match_indices(&self.text) {
                        let end = start + matched.len();
                        if (start == 0 || text[..start].ends_with('\n'))
                            && (end == text.len() || text[end..].starts_with('\n'))
                        {
                            // Identical user-authored copies make attribution ambiguous.
                            if located.is_some() {
                                return None;
                            }
                            located = Some(Location {
                                text,
                                start,
                                context: self,
                            });
                        }
                    }
                }
            }
            return located;
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
