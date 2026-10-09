use super::*;

pub(crate) fn read(bytes: &[u8]) -> Result<Vec<(String, String)>, Fault> {
    let document = lopdf::Document::load_mem(bytes).map_err(invalid)?;
    if document.is_encrypted() {
        return Err(invalid("encrypted PDF is not supported"));
    }
    document
        .get_pages()
        .keys()
        .map(|page| {
            Ok((
                format!("Page {page}"),
                document.extract_text(&[*page]).map_err(invalid)?,
            ))
        })
        .collect()
}
