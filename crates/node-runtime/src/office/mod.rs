//! Native Office inspection and PDF conversion on the execution Node.
use sailry_protocol::{ErrorCode, Fault, office::*};
use std::{
    io::{Cursor, Read as _},
    path::Path,
};

mod package;
mod pdf;
#[cfg(test)]
mod tests;
mod workbook;

pub(crate) const LIMIT: usize = 32 * 1024 * 1024;
const TEXT_LIMIT: usize = 128 * 1024;

fn invalid(error: impl std::fmt::Display) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, format!("Office: {error}"))
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

pub(crate) fn read(root: &Path, options: &Read) -> Result<Inspection, Fault> {
    let ext = extension(&options.path)?;
    let bytes = bytes(root, &options.path)?;
    let revision = blake3::hash(&bytes).to_hex().to_string();
    let sections = match ext {
        "pdf" => pdf::read(&bytes)?,
        "xlsx" => workbook::read(&bytes)?,
        _ => package::read(&bytes, ext)?,
    };
    // Long document parts continue in subsequent offsets instead of losing their tail.
    let mut chunks = Vec::new();
    for (name, text) in sections {
        let mut remaining = text.as_str();
        loop {
            let mut end = remaining.len().min(TEXT_LIMIT);
            while !remaining.is_char_boundary(end) {
                end -= 1;
            }
            chunks.push(Section {
                name: name.clone(),
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
    if options.offset > total {
        return Err(invalid("section offset is out of range"));
    }
    let sections = chunks
        .into_iter()
        .skip(options.offset)
        .take(8)
        .collect::<Vec<_>>();
    let next = (options.offset + sections.len() < total).then_some(options.offset + sections.len());
    Ok(Inspection {
        path: options.path.clone(),
        revision,
        sections,
        next,
    })
}

pub(crate) fn preview(root: &Path, path: &str) -> Result<(Vec<u8>, String, Vec<String>), Fault> {
    let ext = extension(path)?;
    let bytes = bytes(root, path)?;
    let revision = blake3::hash(&bytes).to_hex().to_string();
    let (pdf, warnings) = if ext == "pdf" {
        (bytes, Vec::new())
    } else {
        pdf::convert(&bytes, ext)?
    };
    if pdf.len() > LIMIT {
        return Err(invalid("PDF exceeds 32 MiB"));
    }
    Ok((pdf, revision, warnings))
}

pub(crate) fn export(root: &Path, options: &Export) -> Result<Written, Fault> {
    if extension(&options.path)? != "pdf" || options.path == options.source {
        return Err(invalid("PDF export needs a separate .pdf destination"));
    }
    let (bytes, _, warnings) = preview(root, &options.source)?;
    let file = crate::files::write_binary(
        root,
        &options.path,
        &bytes,
        options.expected_revision.as_deref(),
        LIMIT,
    )?;
    Ok(Written { file, warnings })
}
