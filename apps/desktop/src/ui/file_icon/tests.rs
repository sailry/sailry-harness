use super::*;
use core::prelude::v1::test;
use std::sync::Arc;

#[test]
fn formats_and_metadata() {
    for (name, mime, expected) in [
        ("报告.DOCX", "application/octet-stream", "docx"),
        ("budget.xlsm", "", "xlsm"),
        ("deck.PPTX", "", "pptx"),
        ("report.pdf", "", "pdf"),
        ("archive.zip", "", "zip"),
        ("picture.PNG", "", "png"),
        ("download", "application/msword", "doc"),
        (
            "download",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "docx",
        ),
        (
            "download",
            "application/vnd.ms-excel.sheet.macroEnabled.12",
            "xlsm",
        ),
        (
            "download",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            "pptx",
        ),
        ("report.docx", "application/pdf; charset=utf-8", "pdf"),
        ("data.csv", "text/plain", "csv"),
        ("src/main.rs", "text/plain", "rs"),
        ("download", "image/png", "png"),
        ("download", "image/jpeg", "jpg"),
        ("download", "audio/wav", "wav"),
        ("download", "video/mp4", "mp4"),
        ("download", "image/unknown", "image"),
        ("download", "font/woff2", "woff2"),
        ("download", "audio/webm", "audio"),
        (
            "download",
            "application/vnd.ms-word.document.macroenabled.12",
            "document",
        ),
        ("preview", "application/xhtml+xml", "html"),
        ("readme", "text/plain", "txt"),
        ("file.unknown", "application/octet-stream", "file"),
        ("file.unknown", "application/x-unknown", "file"),
    ] {
        assert_eq!(resolution::resolve(name, mime), expected, "{name}: {mime}");
    }
}

#[test]
fn paths_and_fallbacks() {
    for (name, expected) in [
        ("README", "empty"),
        (".env", "empty"),
        ("", "empty"),
        ("pdf", "pdf"),
        (".PDF", "pdf"),
        ("font", "font"),
        ("image", "image"),
        ("project/", "folder"),
        ("C:\\work\\", "folder"),
        ("C:\\work\\预算.XLSX", "xlsx"),
        ("/資料/报告.PDF", "pdf"),
        ("https://example.com/download/PAPER.PDF?token=1#view", "pdf"),
        ("https://example.com", "empty"),
        ("https://example.com/", "empty"),
        ("https://example.com/folder/?sort=date", "folder"),
        ("report.pdf?v=1.2", "pdf"),
        ("invoice #12.pdf", "pdf"),
        ("note?draft.md", "md"),
        ("image/a.png", "png"),
        ("application/pdf", "pdf"),
        ("audio/webm", "audio"),
        ("application/octet-stream", "empty"),
        ("note.unknown", "file"),
        ("report.", "empty"),
    ] {
        assert_eq!(resolution::resolve(name, ""), expected, "{name}");
    }
}

#[test]
fn catalog_covers_all_types() {
    let catalog = crate::assets::file_types::catalog();
    assert_eq!(catalog.types.len(), 113);
    assert_eq!(
        catalog
            .types
            .iter()
            .filter(|(name, artwork)| name.as_str() != artwork.category)
            .count(),
        99
    );
    let names = crate::assets::Assets.list("icons/reui-file-type/").unwrap();
    assert_eq!(names.len(), 226);
    for (name, artwork) in &catalog.types {
        for edition in ["compact", "regular"] {
            let path = format!("icons/reui-file-type/{edition}/{name}.svg");
            let bytes = crate::assets::Assets.load(&path).unwrap().unwrap();
            let svg = std::str::from_utf8(&bytes).unwrap();
            let document = roxmltree::Document::parse(svg).unwrap();
            assert_eq!(
                document.root_element().attribute("viewBox"),
                Some("0 0 24 24")
            );
            assert!(
                document.descendants().all(|node| {
                    !node.has_tag_name("script")
                        && !node.has_tag_name("image")
                        && node.attribute("href").is_none()
                }),
                "{name}"
            );
            let label = document
                .descendants()
                .find(|node| node.has_tag_name("text"));
            if name == &artwork.category {
                assert!(label.is_none(), "{name}");
            } else {
                assert_eq!(label.unwrap().text().unwrap(), name.to_ascii_uppercase());
            }
        }
    }
    for path in [
        "icons/reui-file-type/regular/../pdf.svg",
        "icons/reui-file-type/unknown/pdf.svg",
        "icons/reui-file-type/regular/missing.svg",
        "icons/reui-file-type/regular/pdf.png",
    ] {
        assert!(crate::assets::Assets.load(path).is_err(), "{path}");
    }
}

#[gpui_kit::test]
fn native_renderer_accepts_catalog(cx: &mut TestAppContext) {
    cx.update(|_| {
        let renderer = SvgRenderer::new(Arc::new(crate::assets::Assets));
        for path in crate::assets::Assets.list("icons/reui-file-type/").unwrap() {
            let bytes = crate::assets::Assets.load(path.as_ref()).unwrap().unwrap();
            let document = renderer.parse_svg(&bytes).unwrap();
            let image = renderer
                .render_parsed(
                    &document,
                    SvgSize::ExactSize(size(DevicePixels(56), DevicePixels(56))),
                )
                .unwrap();
            assert_eq!(image.size(0), size(DevicePixels(56), DevicePixels(56)));
            assert_eq!(image.frame_count(), 1);
        }
    });
}

#[test]
fn artwork_keeps_source_colors() {
    for (kind, fill, foreground) in [
        ("pdf", "#f54900", "#ffffff"),
        ("docx", "#155dfc", "#ffffff"),
        ("xlsx", "#00a63e", "#ffffff"),
        ("png", "#f0b100", "#432004"),
        ("zip", "#bcbdc2", "#52525a"),
    ] {
        for edition in ["compact", "regular"] {
            let bytes = crate::assets::Assets
                .load(&format!("icons/reui-file-type/{edition}/{kind}.svg"))
                .unwrap()
                .unwrap();
            let svg = std::str::from_utf8(&bytes).unwrap();
            let document = roxmltree::Document::parse(svg).unwrap();
            let page = document
                .descendants()
                .find(|node| node.has_tag_name("path"))
                .unwrap();
            let label = document
                .descendants()
                .find(|node| node.has_tag_name("text"))
                .unwrap();
            assert_eq!(page.attribute("fill"), Some(fill), "{kind}: {edition}");
            assert_eq!(
                label.attribute("fill"),
                Some(foreground),
                "{kind}: {edition}"
            );
        }
    }
}
