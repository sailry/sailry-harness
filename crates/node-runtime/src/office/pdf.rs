use super::*;
use office2pdf::config::{ConvertOptions, Format};

pub(super) fn convert(bytes: &[u8], ext: &str) -> Result<(Vec<u8>, Vec<String>), Fault> {
    package::open(bytes)?;
    let format = Format::from_extension(ext).ok_or_else(|| invalid("unsupported conversion"))?;
    let options = ConvertOptions {
        font_bytes: vec![include_bytes!("assets/NotoSansSC-Regular.otf").to_vec()],
        last_resort_font_family: Some("Noto Sans SC".into()),
        ..Default::default()
    };
    let result = office2pdf::convert_bytes(bytes, format, &options).map_err(invalid)?;
    Ok((
        result.pdf,
        result.warnings.iter().map(ToString::to_string).collect(),
    ))
}

pub(super) fn read(bytes: &[u8]) -> Result<Vec<(String, String)>, Fault> {
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
