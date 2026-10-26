use super::*;

#[tokio::test]
async fn retains_compatible_and_blocked_releases() {
    let directory = tempfile::tempdir().unwrap();
    let host = Host::new(Some(directory.path().canonicalize().unwrap()));
    let mut current = host.install_bundled("reminders").unwrap();
    current.origin = Some(Origin::Bundled);
    current.summary.version = Some("0.0.1".into());
    let mut entry = bundled::entries()
        .unwrap()
        .into_iter()
        .find(|entry| entry.id == "reminders")
        .unwrap();
    entry.version = Some("1.0.0".into());
    entry.versions = vec![sailry_protocol::plugin::catalog::Version {
        version: "2.0.0".into(),
        commit: "a".repeat(40),
        min_sailry_version: Some("99.0.0".into()),
        release_notes: Some(sailry_protocol::plugin::catalog::ReleaseNotes {
            text: "Future notes".into(),
            locales: Default::default(),
        }),
        compatibility: sailry_protocol::plugin::Compatibility::RequiresNewer {
            required: "99.0.0".into(),
        },
    }];
    let catalog = Ok(Catalog {
        entries: vec![entry],
    });
    let report = host
        .check_update(current, None, CancellationToken::new(), Some(&catalog))
        .await
        .unwrap();
    assert!(matches!(report.available, Some(Candidate::Official(_))));
    let blocked = report.blocked.unwrap();
    assert_eq!(blocked.version, "2.0.0");
    assert_eq!(blocked.required, "99.0.0");
    assert_eq!(blocked.release_notes.unwrap().text, "Future notes");
}

#[allow(dead_code)]
#[path = "../../../tests/support/skills_git.rs"]
mod fixture;

#[test]
fn compares_semantic_precedence() {
    for (current, available, newer) in [
        ("0.1.9", "0.1.10", true),
        ("0.1.10", "0.1.9", false),
        ("0.1.0", "0.1.0", false),
        ("0.1.0+old", "0.1.0+new", false),
        ("0.1.0-rc.2", "0.1.0-rc.10", true),
        ("0.1.0-rc.1", "0.1.0", true),
        ("0.1.0", "0.1.0-rc.1", false),
    ] {
        assert_eq!(
            version(Some(available))
                .unwrap()
                .cmp_precedence(&version(Some(current)).unwrap())
                .is_gt(),
            newer
        );
    }
    for invalid in [None, Some(""), Some("invalid"), Some("0.01.0")] {
        assert_eq!(
            version(invalid).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
}

#[test]
fn skips_assets_without_a_newer_version() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let host = Host::new(None);
    let manifest = directory.path().join("plugin.json");
    std::fs::write(
        &manifest,
        serde_json::json!({
            "$schema":manifest::SCHEMA,"name":"example","version":"1.0.0",
        })
        .to_string(),
    )
    .unwrap();
    std::fs::File::create(directory.path().join("oversized.bin"))
        .unwrap()
        .set_len(sailry_protocol::plugin::MAX_PACKAGE_BYTES + 1)
        .unwrap();
    for current in ["1.0.0", "1.0.0+retained", "2.0.0"] {
        assert!(
            host.inspect_newer(
                &root,
                "",
                "example",
                Some(current),
                CancellationToken::new()
            )
            .unwrap()
            .is_none()
        );
    }
    std::fs::write(
        &manifest,
        serde_json::json!({
            "$schema":manifest::SCHEMA,"name":"example","version":"2.0.0",
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(
        host.inspect_newer(
            &root,
            "",
            "example",
            Some("1.0.0"),
            CancellationToken::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidRequest
    );
}

#[tokio::test]
async fn selects_the_highest_newer_package() {
    let directory = tempfile::tempdir().unwrap();
    let host = Host::new(Some(directory.path().canonicalize().unwrap()));
    let mut current = host.install_bundled("reminders").unwrap();
    current.origin = Some(Origin::Bundled);
    current.summary.version = Some("0.0.1".into());
    let bundled = Host::bundled_version("reminders").unwrap();
    for (available, official) in [
        ("0.0.2", false),
        (bundled.as_deref().unwrap(), true),
        ("0.2.0", true),
    ] {
        let mut entry = bundled::entries()
            .unwrap()
            .into_iter()
            .find(|entry| entry.id == "reminders")
            .unwrap();
        entry.version = Some(available.into());
        let catalog = Ok(Catalog {
            entries: vec![entry],
        });
        let report = host
            .check_update(
                current.clone(),
                None,
                CancellationToken::new(),
                Some(&catalog),
            )
            .await
            .unwrap();
        assert_eq!(
            matches!(&report.available, Some(Candidate::Official(_))),
            official
        );
        assert!(report.available.is_some());
    }
    for invalid in [None, Some("invalid".into())] {
        current.summary.version = invalid;
        let report = host
            .check_update(current.clone(), None, CancellationToken::new(), None)
            .await
            .unwrap();
        assert!(report.manual && report.available.is_none());
    }
}

#[tokio::test]
async fn pins_manifest_inspection_to_its_commit() {
    use std::io::{Read, Seek};
    let git = fixture::Git::new();
    let selected = repository::source::Selection::parse(&Source {
        repository: git.url.clone(),
        git_ref: Some("release/v1".into()),
        path: Some("plugin".into()),
    })
    .unwrap();
    let host = Host::new(None);
    let stop = CancellationToken::new();
    let (bytes, captured) = host.source_manifest(&selected, stop.clone()).await.unwrap();
    assert_eq!(captured.commit, git.first);
    assert_eq!(
        manifest::parse(&bytes).unwrap().version.as_deref(),
        Some("1.0.0")
    );
    let changed = git.update();
    assert_ne!(changed, captured.commit);
    let (mut file, resolved) = host
        .source_archive(&selected, Some(&captured.commit), stop, None)
        .await
        .unwrap();
    assert_eq!(resolved, captured);
    file.rewind().unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut stored = Vec::new();
    archive
        .by_name("source/plugin/plugin.json")
        .unwrap()
        .read_to_end(&mut stored)
        .unwrap();
    assert_eq!(stored, bytes);
}
