use super::*;

#[test]
fn reads_four_formats() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures.json")).unwrap();
    for case in cases {
        let name = case["path"].as_str().unwrap();
        let expected = case["text"].as_str().unwrap();
        let source = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/office/fixtures")
                .join(name),
        )
        .unwrap();
        let revision = blake3::hash(&source).to_hex().to_string();
        std::fs::write(root.join(name), &source).unwrap();
        let inspection = read(
            &root,
            &Read {
                path: name.into(),
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(inspection.revision, revision);
        assert!(
            inspection
                .sections
                .iter()
                .any(|section| section.text.contains(expected)),
            "{name}: {:?}",
            inspection.sections
        );
        assert_eq!(std::fs::read(root.join(name)).unwrap(), source);
    }
}

#[test]
fn reads_long_unicode_parts_through_continuations() {
    use std::io::Write as _;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let chunk = "中文长文 ";
    // Cross the eight-section response boundary with one Unicode part.
    let text = chunk.repeat(8 * TEXT_LIMIT / chunk.len() + 1);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "word/document.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(format!("<document><p><t>{text}</t></p></document>").as_bytes())
        .unwrap();
    std::fs::write(root.join("long.docx"), zip.finish().unwrap().into_inner()).unwrap();
    let mut offset = 0;
    let mut content = String::new();
    let mut pages = 0;
    loop {
        let page = read(
            &root,
            &Read {
                path: "long.docx".into(),
                offset,
            },
        )
        .unwrap();
        pages += 1;
        for section in page.sections {
            assert!(section.text.len() <= TEXT_LIMIT);
            content.push_str(&section.text);
        }
        let Some(next) = page.next else { break };
        assert!(next > offset);
        offset = next;
    }
    assert!(pages >= 2);
    assert_eq!(content, text);
}
