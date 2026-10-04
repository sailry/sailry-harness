use super::{actions::run, *};

#[tokio::test]
async fn local_and_remote_prune_only_selected_registration() {
    let fixture = Fixture::new().await;
    fixture.stage("base\n");
    fixture.commit("Base", false).await;
    let untouched = fixture.temp.path().join("untouched");
    fixture
        .repository
        .worktree("untouched", &untouched, None)
        .unwrap();
    std::fs::rename(&untouched, fixture.temp.path().join("untouched-retained")).unwrap();
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
    for (index, client) in [&fixture.client, &remote].into_iter().enumerate() {
        let name = format!("stale-{index}");
        let path = fixture.temp.path().join(&name);
        let kept = fixture.temp.path().join(format!("kept-{index}"));
        fixture.repository.worktree(&name, &path, None).unwrap();
        let registered = fixture
            .repository
            .find_worktree(&name)
            .unwrap()
            .path()
            .to_owned();
        std::fs::rename(&path, &kept).unwrap();
        let output = run(
            &fixture,
            client,
            GitAction::PruneWorktree {
                path: registered.to_str().unwrap().into(),
            },
        )
        .await;
        assert_eq!(output, Output::GitActionCompleted);
        assert!(fixture.repository.find_worktree(&name).is_err());
        assert!(fixture.repository.find_worktree("untouched").is_ok());
        assert!(
            fixture
                .repository
                .find_branch(&name, git2::BranchType::Local)
                .is_ok()
        );
        assert_eq!(
            std::fs::read_to_string(kept.join("README.md")).unwrap(),
            "base\n"
        );
    }
    controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}

async fn reject(fixture: &Fixture, path: &Path) {
    let Output::GitStatus(status) = fixture
        .client
        .execute(fixture.client.prepare(Command::InspectGit {
            worktree: fixture.worktree,
        }))
        .await
        .unwrap()
    else {
        panic!("status expected")
    };
    let error = fixture
        .client
        .execute(fixture.client.prepare(Command::RunGitAction {
            worktree: fixture.worktree,
            action: GitAction::PruneWorktree {
                path: path.to_str().unwrap().into(),
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
async fn preserves_existing_locked_foreign_and_symlink_paths() {
    let fixture = Fixture::new().await;
    fixture.stage("base\n");
    fixture.commit("Base", false).await;
    let path = fixture.temp.path().join("linked");
    let worktree = fixture.repository.worktree("linked", &path, None).unwrap();
    let registered = worktree.path().to_owned();
    reject(&fixture, &registered).await;
    reject(&fixture, fixture.repository.workdir().unwrap()).await;
    reject(&fixture, &fixture.temp.path().join("foreign")).await;
    let kept = fixture.temp.path().join("kept");
    std::fs::rename(&path, &kept).unwrap();
    worktree.lock(Some("Keep offline worktree")).unwrap();
    reject(&fixture, &registered).await;
    worktree.unlock().unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(fixture.temp.path().join("missing"), &registered).unwrap();
        reject(&fixture, &registered).await;
        assert!(
            std::fs::symlink_metadata(&registered)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
    assert!(fixture.repository.find_worktree("linked").is_ok());
    assert_eq!(
        std::fs::read_to_string(kept.join("README.md")).unwrap(),
        "base\n"
    );
    fixture.node.shutdown().await.unwrap();
}
