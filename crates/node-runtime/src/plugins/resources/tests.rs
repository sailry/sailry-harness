use super::*;
use std::fs;

fn fixture() -> (tempfile::TempDir, Resources, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source");
    let profile = directory.path().join("node");
    fs::create_dir_all(source.join("skills/analysis/references")).unwrap();
    fs::create_dir(&profile).unwrap();
    fs::write(source.join("plugin.json"), r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"example"}"#).unwrap();
    fs::write(
        source.join("skills/analysis/SKILL.md"),
        "---\nname: analysis\ndescription: Analyze data\n---\nLiteral {not_state} 中文 🙂\n",
    )
    .unwrap();
    fs::write(
        source.join("skills/analysis/references/guide.md"),
        "Complete guide 中文 🙂",
    )
    .unwrap();
    fs::write(source.join("skills/analysis/references/binary"), [0xff]).unwrap();
    fs::write(
        source.join("skills/analysis/references/large"),
        vec![b'a'; skills::MAX_BYTES + 1],
    )
    .unwrap();
    let host = Host::new(Some(profile.canonicalize().unwrap()));
    let info = host
        .install(&source.canonicalize().unwrap(), "", "example")
        .unwrap();
    let installed = profile.join("plugins/packages").join(&info.summary.digest);
    (
        directory,
        Resources::new(host, vec![info], BTreeMap::new(), CancellationToken::new()),
        installed,
    )
}

#[tokio::test]
async fn reads_complete_text() {
    let (_directory, resources, _) = fixture();
    assert!(
        resources
            .catalog()
            .unwrap()
            .contains("example:analysis — Analyze data")
    );
    let text = resources
        .read("example:analysis", "SKILL.md")
        .await
        .unwrap();
    assert!(text.content.ends_with("Literal {not_state} 中文 🙂\n"));
    assert!(Path::new(&text.directory).is_dir());
    assert_eq!(
        resources
            .read("example:analysis", "references/guide.md")
            .await
            .unwrap()
            .content,
        "Complete guide 中文 🙂"
    );
    for path in [
        "../analysis/SKILL.md",
        "/SKILL.md",
        "references/binary",
        "references/large",
        "references/missing",
    ] {
        assert!(
            resources.read("example:analysis", path).await.is_err(),
            "{path}"
        );
    }
    for key in ["analysis", "other:analysis", "example:missing"] {
        assert!(resources.read(key, "SKILL.md").await.is_err(), "{key}");
    }
}

#[tokio::test]
async fn detects_resource_changes() {
    let (_directory, resources, installed) = fixture();
    resources
        .read("example:analysis", "SKILL.md")
        .await
        .unwrap();
    fs::write(
        installed.join("skills/analysis/references/guide.md"),
        "Changed",
    )
    .unwrap();
    assert_eq!(
        resources
            .read("example:analysis", "references/guide.md")
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::Unavailable
    );
    fs::write(installed.join("skills/analysis/references/new.md"), "New").unwrap();
    assert!(
        resources
            .read("example:analysis", "references/new.md")
            .await
            .is_err()
    );
    let reopened = Resources::new(
        resources.host.clone(),
        resources.packages.clone(),
        BTreeMap::new(),
        CancellationToken::new(),
    );
    assert_eq!(
        reopened
            .read("example:analysis", "SKILL.md")
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::Unavailable
    );
}

#[tokio::test]
async fn cancels_open_and_cached_reads() {
    for opened in [false, true] {
        let (_directory, resources, _) = fixture();
        if opened {
            resources
                .read("example:analysis", "SKILL.md")
                .await
                .unwrap();
        }
        resources.stop.cancel();
        assert_eq!(
            resources
                .read("example:analysis", "SKILL.md")
                .await
                .err()
                .unwrap()
                .code,
            ErrorCode::Cancelled
        );
    }
}
#[tokio::test]
async fn bundled_office_resources_use_the_installed_package() {
    let directory = tempfile::tempdir().unwrap();
    let host = Host::new(Some(directory.path().canonicalize().unwrap()));
    let package = host.install_bundled("office").unwrap();
    let resources = Resources::new(
        host,
        vec![package],
        BTreeMap::new(),
        CancellationToken::new(),
    );
    assert_eq!(resources.skills().len(), 4);
    for name in ["word", "excel", "powerpoint", "pdf"] {
        let key = format!("office:{name}");
        let text = resources.read(&key, "SKILL.md").await.unwrap();
        assert!(text.content.contains("execution Node"));
        assert!(text.content.contains("project-local"));
        assert!(!text.content.contains("get_office_runtime"));
        assert!(Path::new(&text.directory).is_dir());
        for relative in ["scripts/inspect_file.py", "templates/create.py"] {
            let script = resources.read(&key, relative).await.unwrap();
            assert!(script.content.contains("argparse"));
            assert_eq!(script.directory, text.directory);
        }
    }
}
