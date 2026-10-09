use sailry_protocol::{ErrorCode, Fault, web};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

type Slot = Arc<tokio::sync::Mutex<Option<Document>>>;

#[derive(Default)]
pub(super) struct Cache {
    entries: BTreeMap<String, (Instant, Slot)>,
}

impl Cache {
    pub(super) fn slot(&mut self, url: &str, offset: usize) -> Result<Slot, Fault> {
        if let Some((used, slot)) = self.entries.get_mut(url) {
            *used = Instant::now();
            return Ok(slot.clone());
        }
        if offset != 0 {
            return Err(restart());
        }
        if self.entries.len() >= web::CACHE_ENTRIES {
            let oldest = self
                .entries
                .iter()
                .filter(|(_, (_, slot))| Arc::strong_count(slot) == 1)
                .min_by_key(|(_, (used, _))| *used)
                .map(|(url, _)| url.clone())
                .ok_or_else(|| Fault::new(ErrorCode::Busy, "web cache capacity is exhausted"))?;
            self.entries.remove(&oldest);
        }
        let slot = Arc::new(tokio::sync::Mutex::new(None));
        self.entries
            .insert(url.into(), (Instant::now(), slot.clone()));
        Ok(slot)
    }
}

pub(super) fn restart() -> Fault {
    Fault::new(
        ErrorCode::NotFound,
        "web page is no longer cached; restart with offset 0",
    )
}

pub(super) struct Document {
    pub url: String,
    pub status: u16,
    pub content_type: String,
    pub title: Option<String>,
    pub content: String,
    pub fetched: Instant,
}

impl Document {
    pub(super) fn expired(&self) -> bool {
        self.fetched.elapsed() >= Duration::from_secs(web::CACHE_SECS)
    }

    pub(super) fn page(&self, offset: usize) -> Result<web::Page, Fault> {
        let mut characters = self.content.char_indices();
        let start = if offset == 0 {
            0
        } else {
            characters
                .nth(offset - 1)
                .map(|(byte, character)| byte + character.len_utf8())
                .ok_or_else(|| Fault::invalid("web character offset is out of range"))?
        };
        let end = self.content[start..]
            .char_indices()
            .nth(web::PAGE_CHARS)
            .map(|(byte, _)| start + byte)
            .unwrap_or(self.content.len());
        let truncated = end < self.content.len();
        Ok(web::Page {
            url: self.url.clone(),
            status: self.status,
            content_type: self.content_type.clone(),
            title: self.title.clone(),
            content: self.content[start..end].into(),
            next_offset: truncated.then_some(offset + web::PAGE_CHARS),
            truncated,
            untrusted: true,
        })
    }
}

#[cfg(test)]
mod tests;
