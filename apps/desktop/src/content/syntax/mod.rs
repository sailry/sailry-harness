//! One Kit syntax backend for Markdown fences, document views and diffs.
use gpui_kit::base::input::Rope;
use gpui_kit::component::{
    ActiveTheme,
    highlighter::{LanguageRegistry, SyntaxHighlighter},
};
use gpui_kit::{App, HighlightStyle};
use std::{cell::RefCell, collections::VecDeque, ops::Range};

const CACHE_ENTRIES: usize = 64;
const CACHE_BYTES: usize = 4 * 1024 * 1024;

struct Entry {
    language: String,
    source: String,
    highlighter: SyntaxHighlighter,
}
thread_local! { static CACHE: RefCell<VecDeque<Entry>> = const { RefCell::new(VecDeque::new()) }; }

/// UTF-8 byte ranges. Unknown languages remain ordinary selectable text.
/// Cached parses are independent of theme colors, so a theme change only repaints.
pub(crate) fn highlight(
    language: &str,
    text: &str,
    cx: &App,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let language = language
        .split([' ', ','])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let Some(config) = LanguageRegistry::singleton().language(&language) else {
        return Vec::new();
    };
    if !config.has_grammar() {
        return Vec::new();
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let found = cache
            .iter()
            .position(|entry| entry.language == config.name.as_ref() && entry.source == text);
        let entry = found
            .and_then(|index| cache.remove(index))
            .unwrap_or_else(|| {
                let mut highlighter = SyntaxHighlighter::new(config.name.as_ref());
                highlighter.update(None, &Rope::from_str(text), None);
                Entry {
                    language: config.name.to_string(),
                    source: text.to_owned(),
                    highlighter,
                }
            });
        let spans = entry
            .highlighter
            .styles(&(0..text.len()), cx.theme().highlight_theme.as_ref());
        if entry.source.len() <= CACHE_BYTES {
            let mut size = cache.iter().map(|entry| entry.source.len()).sum::<usize>();
            while cache.len() >= CACHE_ENTRIES || size + entry.source.len() > CACHE_BYTES {
                let Some(old) = cache.pop_front() else { break };
                size -= old.source.len();
            }
            cache.push_back(entry);
        }
        spans
    })
}
