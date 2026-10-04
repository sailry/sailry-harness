use super::*;

#[test]
fn refreshes_reinstalled_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
    let package = host.install_bundled("code-review").unwrap();
    assert!(!package.skills.is_empty());
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../schema.sql")).unwrap();
    let mut stale = package.clone();
    stale.skills.clear();
    stale.issues.push(plugin::Issue {
        path: "skills/code-review/SKILL.md".into(),
        kind: plugin::IssueKind::InvalidSkill,
    });
    let command = |expected_revision| Command::InstallBundledPlugin {
        name: "code-review".into(),
        expected_revision,
    };
    let mut result = Ok(Output::Plugin(stale));
    finish(&db, &command(0), &mut result).unwrap();
    let mut result = Ok(Output::Plugin(package.clone()));
    finish(&db, &command(1), &mut result).unwrap();
    let installed = required(&db, "code-review").unwrap();
    assert_eq!(installed.summary.revision, 2);
    assert_eq!(installed.summary.digest, package.summary.digest);
    assert_eq!(installed.skills, package.skills);
    let (Output::Plugin(retained), _) = execute(
        &db,
        &Command::ReadPluginVersion {
            package: installed.summary.reference(),
        },
    )
    .unwrap() else {
        panic!("plugin version expected")
    };
    assert_eq!(retained.skills, package.skills);
    assert!(retained.issues.is_empty());
}

#[test]
fn derives_origin_from_admitted_command() {
    let directory = tempfile::tempdir().unwrap();
    let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
    let mut package = host.install_bundled("files").unwrap();
    // Package-supplied metadata must not become inventory provenance.
    package.origin = Some(plugin::Origin::Archive);
    let worktree = sailry_protocol::WorktreeId::new();
    let source = plugin::skills::Resolved {
        repository: "https://github.com/owner/repository.git".into(),
        git_ref: "main".into(),
        commit: "a".repeat(40),
    };
    let online = plugin::Origin::Online {
        source: source.clone(),
        path: "package".into(),
    };
    for (command, origin) in [
        (
            Command::InstallBundledPlugin {
                name: "files".into(),
                expected_revision: 0,
            },
            plugin::Origin::Bundled,
        ),
        (
            Command::InstallPluginUpload {
                stream: sailry_protocol::StreamId::new(),
                source: plugin::UploadSource::Directory,
                name: "files".into(),
                expected_revision: 0,
            },
            plugin::Origin::Directory,
        ),
        (
            Command::InstallPluginUpload {
                stream: sailry_protocol::StreamId::new(),
                source: plugin::UploadSource::Archive,
                name: "files".into(),
                expected_revision: 0,
            },
            plugin::Origin::Archive,
        ),
        (
            Command::InstallPlugin {
                worktree,
                path: "package".into(),
                name: "files".into(),
                expected_revision: 0,
            },
            plugin::Origin::Worktree {
                worktree,
                path: "package".into(),
            },
        ),
        (
            Command::InstallPluginSource {
                source: source.clone(),
                path: "package".into(),
                name: "files".into(),
                expected_revision: 0,
            },
            online.clone(),
        ),
        (
            Command::InstallSkill {
                source,
                path: "package".into(),
                name: "files".into(),
                expected_revision: 0,
            },
            online,
        ),
    ] {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../schema.sql")).unwrap();
        let mut result = Ok(Output::Plugin(package.clone()));
        finish(&db, &command, &mut result).unwrap();
        let installed = required(&db, "files").unwrap();
        assert_eq!(installed.origin, Some(origin));
        assert_eq!(result, Ok(Output::Plugin(installed)));
        let body: Vec<u8> = db
            .query_row(
                "SELECT body FROM plugin_packages WHERE digest=?1",
                [&package.summary.digest],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            serde_json::from_slice::<Info>(&body)
                .unwrap()
                .origin
                .is_none()
        );
        assert!(
            serde_json::from_slice::<serde_json::Value>(&body)
                .unwrap()
                .get("origin")
                .is_none()
        );
        let mut result = Ok(Output::Plugin(package.clone()));
        finish(
            &db,
            &Command::InstallPluginUpload {
                stream: sailry_protocol::StreamId::new(),
                source: plugin::UploadSource::Directory,
                name: "files".into(),
                expected_revision: 1,
            },
            &mut result,
        )
        .unwrap();
        let updated = required(&db, "files").unwrap();
        assert_eq!(updated.origin, Some(plugin::Origin::Directory));
        assert_eq!(updated.summary.digest, package.summary.digest);
        assert_eq!(updated.summary.revision, 2);
        let unchanged: Vec<u8> = db
            .query_row(
                "SELECT body FROM plugin_packages WHERE digest=?1",
                [&package.summary.digest],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unchanged, body);
    }
}
