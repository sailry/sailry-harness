use super::*;

#[tokio::test]
async fn refreshes_reinstalled_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
    let package = host.install_bundled("code-review").unwrap();
    assert!(!package.skills.is_empty());
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../schema.sql"))
        .await
        .unwrap();
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
    finish(&db, None, &command(0), &mut result).await.unwrap();
    let mut result = Ok(Output::Plugin(package.clone()));
    finish(&db, None, &command(1), &mut result).await.unwrap();
    let installed = required(&db, "code-review").await.unwrap();
    assert_eq!(installed.summary.revision, 2);
    assert_eq!(installed.summary.digest, package.summary.digest);
    assert_eq!(installed.skills, package.skills);
    let (Output::Plugin(retained), _) = execute(
        &db,
        None,
        &Command::ReadPluginVersion {
            package: installed.summary.reference(),
        },
    )
    .await
    .unwrap() else {
        panic!("plugin version expected")
    };
    assert_eq!(retained.skills, package.skills);
    assert!(retained.issues.is_empty());
}

#[tokio::test]
async fn derives_origin_from_admitted_command() {
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
                expected_digest: None,
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
        let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let db = crate::store::database::connect(&engine).await.unwrap();
        db.execute_batch(include_str!("../schema.sql"))
            .await
            .unwrap();
        let mut result = Ok(Output::Plugin(package.clone()));
        finish(&db, None, &command, &mut result).await.unwrap();
        let installed = required(&db, "files").await.unwrap();
        assert_eq!(installed.origin, Some(origin));
        assert_eq!(result, Ok(Output::Plugin(installed)));
        let body: Vec<u8> = (async {
            let mut statement = db
                .prepare("SELECT body FROM plugin_packages WHERE digest=?1")
                .await?;
            let row = statement
                .query_row([package.summary.digest.as_str()])
                .await?;
            let mapped: turso::Result<_> = row.get(0);
            mapped
        })
        .await
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
            None,
            &Command::InstallPluginUpload {
                stream: sailry_protocol::StreamId::new(),
                source: plugin::UploadSource::Directory,
                name: "files".into(),
                expected_revision: 1,
            },
            &mut result,
        )
        .await
        .unwrap();
        let updated = required(&db, "files").await.unwrap();
        assert_eq!(updated.origin, Some(plugin::Origin::Directory));
        assert_eq!(updated.summary.digest, package.summary.digest);
        assert_eq!(updated.summary.revision, 2);
        let unchanged: Vec<u8> = (async {
            let mut statement = db
                .prepare("SELECT body FROM plugin_packages WHERE digest=?1")
                .await?;
            let row = statement
                .query_row([package.summary.digest.as_str()])
                .await?;
            let mapped: turso::Result<_> = row.get(0);
            mapped
        })
        .await
        .unwrap();
        assert_eq!(unchanged, body);
    }
}
