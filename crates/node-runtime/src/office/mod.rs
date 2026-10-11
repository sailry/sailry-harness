//! Native Office inspection on the execution Node.
use sailry_protocol::knowledge::Position;
use sailry_protocol::{Fault, office::*};
use std::{
    io::{Cursor, Read as _},
    path::Path,
};

mod package;
pub(crate) mod pdf;
#[cfg(test)]
mod tests;
mod workbook;

pub(crate) const LIMIT: usize = 32 * 1024 * 1024;
const TEXT_LIMIT: usize = 128 * 1024;

pub(crate) struct Part {
    pub name: String,
    pub text: String,
    pub position: Position,
}

fn invalid(error: impl std::fmt::Display) -> Fault {
    Fault::invalid(format!("Office: {error}"))
}

fn extension(path: &str) -> Result<&'static str, Fault> {
    let ext = path.rsplit('.').next().unwrap_or_default();
    ["docx", "xlsx", "pptx", "pdf"]
        .into_iter()
        .find(|known| ext.eq_ignore_ascii_case(known))
        .ok_or_else(|| invalid("supported extensions are .docx, .xlsx, .pptx and .pdf"))
}

pub(crate) fn bytes(root: &Path, path: &str) -> Result<Vec<u8>, Fault> {
    let parts = crate::files::path::components(path, false)?;
    let (name, parents) = parts.split_last().unwrap();
    let dir = crate::files::path::descend(crate::files::path::root(root)?, parents)?;
    let file = crate::files::open_regular(&dir, name)?;
    let mut bytes = Vec::new();
    file.take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(invalid)?;
    if bytes.len() > LIMIT {
        return Err(invalid("file exceeds 32 MiB"));
    }
    Ok(bytes)
}

#[cfg(test)]
pub(crate) fn read(root: &Path, options: &Read) -> Result<Inspection, Fault> {
    read_with(root, options, None, &sailry_link::CancellationToken::new())
}

pub(crate) fn read_with(
    root: &Path,
    options: &Read,
    ocr: Option<&crate::ocr::Service>,
    cancel: &sailry_link::CancellationToken,
) -> Result<Inspection, Fault> {
    let Source::Path(path) = &options.source else {
        return Err(Fault::invalid(
            "attachment inspection requires a captured turn",
        ));
    };
    let bytes = bytes(root, path)?;
    inspect(path, &bytes, options.offset, ocr, cancel)
}

pub(crate) fn inspect(
    path: &str,
    bytes: &[u8],
    offset: usize,
    ocr: Option<&crate::ocr::Service>,
    cancel: &sailry_link::CancellationToken,
) -> Result<Inspection, Fault> {
    let revision = blake3::hash(bytes).to_hex().to_string();
    let sections = parts_with(path, bytes, ocr, cancel)?;
    // Long document parts continue in subsequent offsets instead of losing their tail.
    let mut chunks = Vec::new();
    for part in sections {
        let mut remaining = part.text.as_str();
        loop {
            let mut end = remaining.len().min(TEXT_LIMIT);
            while !remaining.is_char_boundary(end) {
                end -= 1;
            }
            chunks.push(Section {
                name: part.name.clone(),
                text: remaining[..end].to_owned(),
                truncated: end < remaining.len(),
            });
            remaining = &remaining[end..];
            if remaining.is_empty() {
                break;
            }
        }
    }
    let total = chunks.len();
    if offset > total {
        return Err(invalid("section offset is out of range"));
    }
    let sections = chunks.into_iter().skip(offset).take(8).collect::<Vec<_>>();
    let next = (offset + sections.len() < total).then_some(offset + sections.len());
    Ok(Inspection {
        path: path.into(),
        revision,
        sections,
        next,
    })
}

pub(crate) fn parts(path: &str, bytes: &[u8]) -> Result<Vec<Part>, Fault> {
    if bytes.len() > LIMIT {
        return Err(invalid("file exceeds 32 MiB"));
    }
    match extension(path)? {
        "pdf" => pdf::parts(bytes),
        "xlsx" => workbook::parts(bytes),
        ext => package::parts(bytes, ext),
    }
}

pub(crate) fn parts_with(
    path: &str,
    bytes: &[u8],
    ocr: Option<&crate::ocr::Service>,
    cancel: &sailry_link::CancellationToken,
) -> Result<Vec<Part>, Fault> {
    if bytes.len() > LIMIT {
        return Err(invalid("file exceeds 32 MiB"));
    }
    if crate::ocr::document::supported(path) {
        return crate::ocr::document::image(
            ocr.ok_or_else(|| {
                Fault::new(
                    sailry_protocol::ErrorCode::NotConfigured,
                    "an OCR model must be installed",
                )
            })?,
            bytes,
            cancel,
        );
    }
    let parts = parts(path, bytes)?;
    if path
        .rsplit('.')
        .next()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
        && let Some(ocr) = ocr
    {
        crate::ocr::document::pdf(ocr, bytes, parts, cancel)
    } else {
        Ok(parts)
    }
}
