//! Native Office inspection on the execution Node.
pub(crate) use sailry_office::{LIMIT, Part, parts, pdf};
use sailry_office::{TEXT_LIMIT, format, invalid};
use sailry_protocol::{Fault, office::*};
use std::io::Read as _;
#[cfg(test)]
use std::{io::Cursor, path::Path};

#[cfg(test)]
mod tests;

pub(crate) fn bytes<'a>(
    root: impl Into<crate::files::Scope<'a>>,
    path: &str,
) -> Result<Vec<u8>, Fault> {
    let root = root.into();
    let file = crate::files::path::file(root, path)?;
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

pub(crate) fn read_with<'a>(
    root: impl Into<crate::files::Scope<'a>>,
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
    let sections = parts_with(
        path,
        bytes,
        ocr,
        crate::models::runtime::Lane::Interactive,
        cancel,
    )?;
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

pub(crate) fn parts_with(
    path: &str,
    bytes: &[u8],
    ocr: Option<&crate::ocr::Service>,
    lane: crate::models::runtime::Lane,
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
            lane,
            cancel,
        );
    }
    let parts = parts(path, bytes)?;
    if format(path, bytes)? == "pdf"
        && let Some(ocr) = ocr
    {
        crate::ocr::document::pdf(ocr, bytes, parts, lane, cancel)
    } else {
        Ok(parts)
    }
}
