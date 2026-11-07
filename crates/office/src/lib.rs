//! Native Office and PDF text extraction without file or OCR access.
use sailry_protocol::Fault;
use sailry_protocol::knowledge::Position;
use std::io::{Cursor, Read as _};

mod package;
pub mod pdf;
mod workbook;

pub const LIMIT: usize = 32 * 1024 * 1024;
pub const TEXT_LIMIT: usize = 128 * 1024;

pub struct Part {
    pub name: String,
    pub text: String,
    pub position: Position,
}

pub fn invalid(error: impl std::fmt::Display) -> Fault {
    Fault::invalid(format!("Office: {error}"))
}

fn extension(path: &str) -> Result<&'static str, Fault> {
    let ext = path.rsplit('.').next().unwrap_or_default();
    ["docx", "xlsx", "pptx", "pdf"]
        .into_iter()
        .find(|known| ext.eq_ignore_ascii_case(known))
        .ok_or_else(|| invalid("supported extensions are .docx, .xlsx, .pptx and .pdf"))
}

pub fn format(path: &str, bytes: &[u8]) -> Result<&'static str, Fault> {
    if file_format::FileFormat::from_bytes(bytes) == file_format::FileFormat::PortableDocumentFormat
    {
        Ok("pdf")
    } else {
        extension(path)
    }
}

pub fn parts(path: &str, bytes: &[u8]) -> Result<Vec<Part>, Fault> {
    if bytes.len() > LIMIT {
        return Err(invalid("file exceeds 32 MiB"));
    }
    match format(path, bytes)? {
        "pdf" => pdf::parts(bytes),
        "xlsx" => workbook::parts(bytes),
        ext => package::parts(bytes, ext),
    }
}
