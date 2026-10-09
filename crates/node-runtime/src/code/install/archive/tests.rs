use super::*;
use std::io::Cursor;

fn file(bytes: &[u8]) -> File {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(bytes).unwrap();
    file
}
fn asset(name: &str) -> Asset {
    Asset {
        name: name.into(),
        url: "https://example.invalid/fixture".into(),
        directory: String::new(),
    }
}

#[test]
fn extracts_supported_release_formats() {
    let root = tempfile::tempdir().unwrap();
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "bin/server",
            zip::write::SimpleFileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
    writer.write_all(b"zip").unwrap();
    let zip = writer.finish().unwrap().into_inner();
    extract(
        file(&zip),
        &asset("server.zip"),
        root.path(),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(root.path().join("bin/server")).unwrap(),
        b"zip"
    );
    for name in ["server.tar.gz", "server.tar.xz"] {
        let root = tempfile::tempdir().unwrap();
        let mut archive = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(&mut header, "bin/server", &b"tar"[..])
            .unwrap();
        let bytes = archive.into_inner().unwrap();
        let encoded = if name.ends_with(".gz") {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(&bytes).unwrap();
            encoder.finish().unwrap()
        } else {
            let mut encoder = xz2::write::XzEncoder::new(Vec::new(), 6);
            encoder.write_all(&bytes).unwrap();
            encoder.finish().unwrap()
        };
        extract(
            file(&encoded),
            &asset(name),
            root.path(),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(root.path().join("bin/server")).unwrap(),
            b"tar"
        );
    }
}

#[test]
fn rejects_escaping_paths_and_cancelled_extraction() {
    let root = tempfile::tempdir().unwrap();
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("../escape", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"escape").unwrap();
    let zip = writer.finish().unwrap().into_inner();
    assert!(
        extract(
            file(&zip),
            &asset("server.zip"),
            root.path(),
            &CancellationToken::new()
        )
        .is_err()
    );
    assert!(!root.path().parent().unwrap().join("escape").exists());
    let stop = CancellationToken::new();
    stop.cancel();
    assert!(matches!(
        extract(file(b"server"), &asset("server"), root.path(), &stop),
        Err(Fault {
            code: ErrorCode::Cancelled,
            ..
        })
    ));
}

#[test]
fn rejects_links_and_expanded_size_overflow() {
    let root = tempfile::tempdir().unwrap();
    let mut archive = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    header.set_mode(0o777);
    header.set_link_name("/outside").unwrap();
    header.set_cksum();
    archive
        .append_data(&mut header, "server", std::io::empty())
        .unwrap();
    let bytes = archive.into_inner().unwrap();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&bytes).unwrap();
    let bytes = encoder.finish().unwrap();
    assert!(
        extract(
            file(&bytes),
            &asset("server.tar.gz"),
            root.path(),
            &CancellationToken::new()
        )
        .is_err()
    );
    let mut size = MAX_BYTES;
    assert!(
        write(
            &mut Cursor::new(b"x"),
            &root.path().join("limit"),
            &mut size,
            &CancellationToken::new()
        )
        .is_err()
    );
}
