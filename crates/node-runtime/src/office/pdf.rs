use super::*;

pub(crate) fn read(bytes: &[u8]) -> Result<Vec<(String, String)>, Fault> {
    parts(bytes).map(|parts| {
        parts
            .into_iter()
            .map(|part| (part.name, part.text))
            .collect()
    })
}

pub(super) fn parts(bytes: &[u8]) -> Result<Vec<Part>, Fault> {
    let document = lopdf::Document::load_mem(bytes).map_err(invalid)?;
    if document.is_encrypted() {
        return Err(invalid("encrypted PDF is not supported"));
    }
    document
        .get_pages()
        .keys()
        .map(|page| {
            Ok(Part {
                name: format!("Page {page}"),
                text: document.extract_text(&[*page]).map_err(invalid)?,
                position: Position::Page { page: *page },
            })
        })
        .collect()
}
