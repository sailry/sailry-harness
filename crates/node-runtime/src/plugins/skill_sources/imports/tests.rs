use super::*;
use std::io::Write;

fn tar(link: bool) -> Vec<u8> {
    let mut archive = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    let body = b"---\nname: analysis\ndescription: Inspect a sample\n---\nOriginal body\n";
    header.set_mode(0o644);
    if link {
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_path("SKILL.md").unwrap();
        header.set_link_name("../private").unwrap();
        header.set_size(0);
        header.set_cksum();
        archive.append(&header, std::io::empty()).unwrap();
    } else {
        header.set_size(body.len() as u64);
        header.set_cksum();
        archive
            .append_data(&mut header, "bundle/analysis/SKILL.md", &body[..])
            .unwrap();
    }
    archive.into_inner().unwrap()
}

#[test]
fn discovers_safe_tar_and_gzip_roots() {
    let host = Host::new(None);
    for format in [ArchiveFormat::Tar, ArchiveFormat::TarGz] {
        let bytes = tar(false);
        let bytes = if format == ArchiveFormat::TarGz {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(&bytes).unwrap();
            encoder.finish().unwrap()
        } else {
            bytes
        };
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(&bytes).unwrap();
        let discovery = host
            .discover_skill_import(
                file,
                sailry_protocol::StreamId::new(),
                blake3::hash(&bytes).to_hex().to_string(),
                format,
                UploadSource::Archive,
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(discovery.skills.len(), 1);
        assert_eq!(discovery.skills[0].skill.name, "analysis");
        assert_eq!(discovery.skills[0].path, "bundle/analysis");
    }
}

#[test]
fn rejects_links_and_confined_path_escapes() {
    let bytes = tar(true);
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&bytes).unwrap();
    assert!(archive::zip(file, ArchiveFormat::Tar, &CancellationToken::new()).is_err());
    let root = tempfile::tempdir().unwrap();
    for path in [
        "../private",
        ".git/config",
        ".sailry/worktrees/another/SKILL.md",
    ] {
        assert!(
            Host::snapshot_skills(
                root.path(),
                path,
                tempfile::tempfile().unwrap(),
                &CancellationToken::new()
            )
            .is_err()
        );
    }
}

#[test]
fn keeps_reimport_identity_independent_of_content() {
    assert_ne!(
        identity("analysis", "analysis"),
        identity("other", "analysis")
    );
    let root = tempfile::tempdir().unwrap();
    let base = root.path().canonicalize().unwrap();
    let directory = base.join("analysis");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(
        directory.join("SKILL.md"),
        "---\nname: analysis\ndescription: Inspect a sample\n---\nFirst body\n",
    )
    .unwrap();
    let stop = CancellationToken::new();
    let (mut file, format, kind) =
        Host::snapshot_skills(&base, "analysis", tempfile::tempfile().unwrap(), &stop).unwrap();
    let first = Host::skill_import_digest(&mut file, &stop).unwrap();
    let initial = Host::new(None)
        .discover_skill_import(
            file,
            sailry_protocol::StreamId::new(),
            first.clone(),
            format,
            kind,
            &stop,
        )
        .unwrap();
    std::fs::write(
        directory.join("SKILL.md"),
        "---\nname: analysis\ndescription: Inspect a sample\n---\nSecond body\n",
    )
    .unwrap();
    let (mut file, format, kind) = Host::snapshot_skills(
        &base,
        "analysis/SKILL.md",
        tempfile::tempfile().unwrap(),
        &stop,
    )
    .unwrap();
    let second = Host::skill_import_digest(&mut file, &stop).unwrap();
    let updated = Host::new(None)
        .discover_skill_import(
            file,
            sailry_protocol::StreamId::new(),
            second.clone(),
            format,
            kind,
            &stop,
        )
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(initial.skills[0].name, updated.skills[0].name);
}
