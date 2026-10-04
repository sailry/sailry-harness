use super::actions::run;
use super::*;

#[tokio::test]
async fn local_and_remote() {
    let fixture = Fixture::new().await;
    fixture.stage("base\n");
    fixture.commit("Base", false).await;
    let controller = Link::controller(
        fixture.temp.path().join("controller"),
        NetworkScope::default(),
    )
    .await
    .unwrap();
    let address = controller
        .handle()
        .pair(fixture.node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    let root = fixture.repository.workdir().unwrap();
    std::fs::write(
        root.join(".gitignore"),
        "# Keep existing rules\r\n/existing\r\n",
    )
    .unwrap();
    let index = std::fs::read(root.join(".git/index")).unwrap();
    for (position, client) in [&fixture.client, &remote].into_iter().enumerate() {
        let directory = format!("output-{position}");
        std::fs::create_dir_all(root.join(&directory)).unwrap();
        std::fs::write(root.join(&directory).join("artifact.bin"), "artifact").unwrap();
        std::fs::create_dir_all(root.join(format!("{directory}-sibling"))).unwrap();
        std::fs::write(root.join(format!("{directory}-sibling/keep")), "keep").unwrap();
        let literal = format!("literal[1]-{position}.txt");
        let sibling = format!("literal1-{position}.txt");
        std::fs::write(root.join(&literal), "literal").unwrap();
        std::fs::write(root.join(&sibling), "sibling").unwrap();
        for action in [
            GitAction::Ignore {
                paths: vec![directory.clone()],
                directories: true,
            },
            GitAction::Ignore {
                paths: vec![literal.clone(), literal.clone()],
                directories: false,
            },
        ] {
            run(&fixture, client, action.clone()).await;
            let before = std::fs::read(root.join(".gitignore")).unwrap();
            run(&fixture, client, action).await;
            assert_eq!(std::fs::read(root.join(".gitignore")).unwrap(), before);
        }
        let repo = Repository::open(root).unwrap();
        assert!(
            repo.status_should_ignore(Path::new(&format!("{directory}/artifact.bin")))
                .unwrap()
        );
        assert!(repo.status_should_ignore(Path::new(&literal)).unwrap());
        assert!(!repo.status_should_ignore(Path::new(&sibling)).unwrap());
        assert!(
            !repo
                .status_should_ignore(Path::new(&format!("{directory}-sibling/keep")))
                .unwrap()
        );
        let text = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert!(text.starts_with("# Keep existing rules\r\n/existing\r\n"));
        assert_eq!(text.matches(&format!("/{directory}/\r\n")).count(), 1);
        assert_eq!(
            text.matches(&format!("/literal\\[1\\]-{position}.txt\r\n"))
                .count(),
            1
        );
        assert!(root.join(&literal).exists());
        assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
    }
    controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}

async fn reject(fixture: &Fixture, paths: &[&str], directories: bool) {
    let Output::GitStatus(status) = fixture
        .client
        .execute(fixture.client.prepare(Command::InspectGit {
            worktree: fixture.worktree,
        }))
        .await
        .unwrap()
    else {
        panic!()
    };
    let error = fixture
        .client
        .execute(fixture.client.prepare(Command::RunGitAction {
            worktree: fixture.worktree,
            action: GitAction::Ignore {
                paths: paths.iter().map(|path| (*path).into()).collect(),
                directories,
            },
            expected_index: status.index_revision.unwrap(),
            expected_head: status.head,
            expected_branch: status.branch,
        }))
        .await
        .unwrap_err();
    assert_ne!(error.code, ErrorCode::OutcomeUnknown);
}

#[tokio::test]
async fn preserves_files_on_rejection() {
    let fixture = Fixture::new().await;
    fixture.stage("base\n");
    fixture.commit("Base", false).await;
    let root = fixture.repository.workdir().unwrap();
    std::fs::write(root.join(".gitignore"), "# preserved").unwrap();
    for (paths, directories) in [
        (vec!["README.md"], false),
        (vec!["../outside"], false),
        (vec![".git/config"], false),
        (vec!["README.md"], true),
    ] {
        reject(&fixture, &paths, directories).await;
        assert_eq!(
            std::fs::read_to_string(root.join(".gitignore")).unwrap(),
            "# preserved"
        );
    }
    #[cfg(unix)]
    {
        std::fs::remove_file(root.join(".gitignore")).unwrap();
        let outside = fixture.temp.path().join("outside");
        std::fs::write(&outside, "outside").unwrap();
        std::os::unix::fs::symlink(&outside, root.join(".gitignore")).unwrap();
        std::fs::write(root.join("new.txt"), "new").unwrap();
        reject(&fixture, &["new.txt"], false).await;
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "outside");
    }
    fixture.node.shutdown().await.unwrap();
}
