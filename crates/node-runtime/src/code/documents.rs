use sailry_protocol::{ErrorCode, Fault};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    time::Instant,
};

pub(super) const MAX_TEXT: usize = 4 * 1024 * 1024;
const MAX_DOCUMENTS: usize = 50;

#[derive(Clone)]
pub(super) struct Text {
    pub text: String,
    pub revision: String,
}

pub(super) fn read(root: &Path, path: &str) -> Result<Text, Fault> {
    let parts = crate::files::path::components(path, false)?;
    let directory =
        crate::files::path::descend(crate::files::path::root(root)?, &parts[..parts.len() - 1])?;
    let file = crate::files::open_regular(&directory, parts[parts.len() - 1])?;
    let mut bytes = Vec::new();
    file.take((MAX_TEXT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() > MAX_TEXT {
        return Err(Fault::invalid("code document exceeds the text limit"));
    }
    let revision = blake3::hash(&bytes).to_hex().to_string();
    let text =
        String::from_utf8(bytes).map_err(|_| Fault::invalid("code document is not UTF-8 text"))?;
    if text.contains('\0') {
        return Err(Fault::invalid("code document contains binary data"));
    }
    Ok(Text { text, revision })
}

pub(super) fn uri(root: &Path, path: &str) -> Result<String, Fault> {
    crate::files::path::components(path, false)?;
    url::Url::from_file_path(root.join(path))
        .map(|value| value.to_string())
        .map_err(|_| Fault::invalid("code document URI is unavailable"))
}

pub(super) fn relative(root: &Path, uri: &str) -> Option<String> {
    let url = url::Url::parse(uri).ok()?;
    if url.scheme() != "file" || url.query().is_some() || url.fragment().is_some() {
        return None;
    }
    let path = url.to_file_path().ok()?.canonicalize().ok()?;
    let relative = path.strip_prefix(root).ok()?.to_str()?.replace('\\', "/");
    crate::files::path::components(&relative, false).ok()?;
    // Re-open through the retained root; an ambient canonicalization is not read authority.
    read(root, &relative).ok()?;
    Some(relative)
}

pub(super) struct Document {
    pub text: Text,
    pub saved_revision: String,
    pub version: i32,
    pub used: Instant,
}

#[derive(Default)]
pub(super) struct Documents(pub BTreeMap<String, Document>);

impl Documents {
    pub fn put(
        &mut self,
        path: String,
        text: Text,
        saved_revision: String,
    ) -> (bool, bool, i32, Option<String>) {
        let mut evicted = None;
        if !self.0.contains_key(&path) && self.0.len() >= MAX_DOCUMENTS {
            evicted = self
                .0
                .iter()
                .min_by_key(|(_, document)| document.used)
                .map(|(path, _)| path.clone());
            if let Some(path) = &evicted {
                self.0.remove(path);
            }
        }
        let open = !self.0.contains_key(&path);
        let changed = self
            .0
            .get(&path)
            .is_none_or(|document| document.text.revision != text.revision);
        let version = self.0.get(&path).map_or(1, |document| {
            document.version.saturating_add(i32::from(changed))
        });
        self.0.insert(
            path,
            Document {
                text,
                saved_revision,
                version,
                used: Instant::now(),
            },
        );
        (open, changed, version, evicted)
    }
}

pub(super) fn directory(root: &Path, relative: &str) -> Result<PathBuf, Fault> {
    let parts = crate::files::path::components(relative, true)?;
    crate::files::path::descend(crate::files::path::root(root)?, &parts)?;
    Ok(root.join(relative))
}

fn io(error: std::io::Error) -> Fault {
    Fault::new(
        ErrorCode::InvalidRequest,
        format!("code document is unavailable: {error}"),
    )
}

#[cfg(test)]
mod tests;
