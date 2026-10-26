use super::*;

#[path = "../support/environment.rs"]
mod fixture;

#[tokio::test]
async fn preserves_login_paths_local_and_remote() {
    let directory = tempfile::tempdir().unwrap();
    let login = fixture::Login::new(directory.path());
    let native = which::which("git").unwrap();
    for (path, label) in [(&login.first, "first"), (&login.second, "second")] {
        fixture::executable(
            &path.join("git"),
            &format!(
                "#!/bin/sh\nprintf '{label}\\n' >> \"$HOME/git-calls\"\nexec {} \"$@\"\n",
                fixture::quote(native.to_str().unwrap())
            ),
        );
        fixture::executable(
            &path.join("sailry-hook-fixture"),
            &format!(
                "#!/bin/sh\nprintf '{label}:%s:%s' \"$HOME\" \"$SSH_AUTH_SOCK\" > .git/environment-result\n",
            ),
        );
    }
    let mut source = login.source();
    let agent = login.home.join("fixture-agent");
    source
        .environment
        .insert("SSH_AUTH_SOCK".into(), agent.clone().into_os_string());
    let fixture = Fixture::start(Some(source)).await;
    login.select(true);
    fixture::executable(
        &fixture.repository.path().join("hooks/pre-commit"),
        "#!/bin/sh\nsailry-hook-fixture\n",
    );
    let origin = fixture.temp.path().join("origin.git");
    Repository::init_bare(&origin).unwrap();
    fixture
        .repository
        .remote("fixture", origin.to_str().unwrap())
        .unwrap();
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
    for (position, client) in [&fixture.client, &remote].into_iter().enumerate() {
        fixture.stage(&format!("content {position}\n"));
        assert!(matches!(
            client
                .execute(fixture.request(client, "Use captured login", false).await)
                .await
                .unwrap(),
            Output::GitCommitCreated { .. }
        ));
        assert_eq!(
            std::fs::read_to_string(fixture.repository.path().join("environment-result")).unwrap(),
            format!("first:{}:{}", login.home.display(), agent.display())
        );
        assert!(matches!(
            client
                .execute(client.prepare(Command::ListGitRemoteTags {
                    worktree: fixture.worktree,
                    remote: "fixture".into(),
                }))
                .await
                .unwrap(),
            Output::GitRemoteTags(tags) if tags.is_empty()
        ));
    }
    assert_eq!(
        std::fs::read_to_string(login.home.join("git-calls")).unwrap(),
        "first\nfirst\nfirst\nfirst\n"
    );
    controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}
