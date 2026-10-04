//! File artwork belongs to the desktop, independent of the producing plugin.
use gpui_kit::{component::Icon, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Artwork {
    Color(&'static str),
    Glyph(&'static str),
}

pub(crate) fn render(path: &str, mime: &str, size: Rems) -> AnyElement {
    match artwork(path, mime) {
        // Kit Icon applies a single foreground color. GPUI images preserve the
        // embedded SVG's format colors without fading the surrounding controls.
        Artwork::Color(asset) => img(asset).size(size).flex_shrink_0().into_any_element(),
        Artwork::Glyph(asset) => Icon::default().path(asset).size(size).into_any_element(),
    }
}

fn artwork(path: &str, mime: &str) -> Artwork {
    use Artwork::{Color, Glyph};
    let mime = mime
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    // Specific MIME metadata wins; generic types still allow extension lookup.
    match mime.as_str() {
        "application/msword" => return Color("icons/files/word.svg"),
        "application/vnd.ms-excel" => return Color("icons/files/excel.svg"),
        "application/vnd.ms-powerpoint" => return Color("icons/files/powerpoint.svg"),
        "application/pdf" => return Color("icons/files/pdf.svg"),
        "text/html" | "application/xhtml+xml" => return Glyph("icons/globe.svg"),
        value
            if value
                .starts_with("application/vnd.openxmlformats-officedocument.wordprocessingml.")
                || value.starts_with("application/vnd.ms-word.") =>
        {
            return Color("icons/files/word.svg");
        }
        value
            if value
                .starts_with("application/vnd.openxmlformats-officedocument.spreadsheetml.")
                || value.starts_with("application/vnd.ms-excel.") =>
        {
            return Color("icons/files/excel.svg");
        }
        value
            if value
                .starts_with("application/vnd.openxmlformats-officedocument.presentationml.")
                || value.starts_with("application/vnd.ms-powerpoint.") =>
        {
            return Color("icons/files/powerpoint.svg");
        }
        _ => (),
    }
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "doc" | "docx" | "docm" | "dot" | "dotx" | "dotm" => Color("icons/files/word.svg"),
        "xls" | "xlsx" | "xlsm" | "xlsb" | "xlt" | "xltx" | "xltm" => {
            Color("icons/files/excel.svg")
        }
        "ppt" | "pptx" | "pptm" | "pot" | "potx" | "potm" | "pps" | "ppsx" | "ppsm" => {
            Color("icons/files/powerpoint.svg")
        }
        "pdf" => Color("icons/files/pdf.svg"),
        "html" | "htm" | "xhtml" => Glyph("icons/globe.svg"),
        "csv" | "tsv" | "ods" => Color("icons/fluent/table.svg"),
        "odp" | "key" => Color("icons/fluent/slide_text_sparkle.svg"),
        "rs" | "js" | "ts" | "tsx" | "jsx" | "py" | "json" | "toml" | "yaml" | "yml" | "css"
        | "sh" | "go" | "java" | "c" | "cpp" | "h" | "sql" | "vue" => {
            Color("icons/fluent/code_block.svg")
        }
        "txt" | "md" | "rtf" | "odt" => Color("icons/fluent/document_text.svg"),
        "zip" | "gz" | "tar" | "7z" | "rar" => Glyph("reicon:files/file-zip"),
        _ if mime.starts_with("image/") => Glyph("reicon:video/image"),
        _ if mime.starts_with("audio/") => Glyph("reicon:video/music-note"),
        _ if mime.starts_with("video/") => Glyph("reicon:video/video"),
        _ if mime.starts_with("text/") => Color("icons/fluent/document_text.svg"),
        _ => Color("icons/fluent/document.svg"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn office_vectors_are_self_contained() {
        for name in ["word", "excel", "powerpoint", "pdf"] {
            let bytes = crate::assets::Assets
                .load(&format!("icons/files/{name}.svg"))
                .unwrap()
                .unwrap();
            let svg = std::str::from_utf8(&bytes).unwrap().to_owned();
            assert!(
                sailry_protocol::plugin::desktop::Icon::Svg { svg }.valid(),
                "{name}"
            );
        }
    }

    #[test]
    fn formats_and_fallbacks() {
        for (path, mime, expected) in [
            (
                "报告.DOCX",
                "application/octet-stream",
                "icons/files/word.svg",
            ),
            ("budget.xlsm", "", "icons/files/excel.svg"),
            ("deck.PPTX", "", "icons/files/powerpoint.svg"),
            ("report.pdf", "", "icons/files/pdf.svg"),
            (
                "download",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "icons/files/word.svg",
            ),
            (
                "download",
                "application/vnd.ms-excel.sheet.macroEnabled.12",
                "icons/files/excel.svg",
            ),
            (
                "download",
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
                "icons/files/powerpoint.svg",
            ),
            ("report.docx", "application/pdf", "icons/files/pdf.svg"),
            ("data.csv", "text/plain", "icons/fluent/table.svg"),
            ("src/main.rs", "text/plain", "icons/fluent/code_block.svg"),
            (
                "file.unknown",
                "application/octet-stream",
                "icons/fluent/document.svg",
            ),
        ] {
            assert_eq!(
                artwork(path, mime),
                Artwork::Color(expected),
                "{path}: {mime}"
            );
            assert!(
                crate::assets::Assets.load(expected).unwrap().is_some(),
                "{expected}"
            );
        }
        for (path, mime, expected) in [
            ("preview.html", "text/html", "icons/globe.svg"),
            ("archive.zip", "", "reicon:files/file-zip"),
            ("download", "image/png", "reicon:video/image"),
            ("download", "audio/wav", "reicon:video/music-note"),
            ("download", "video/mp4", "reicon:video/video"),
        ] {
            assert_eq!(artwork(path, mime), Artwork::Glyph(expected));
            assert!(
                crate::assets::Assets.load(expected).unwrap().is_some(),
                "{expected}"
            );
        }
    }
}
