use super::*;

fn git(root: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("/usr/bin/git")
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn protects_metadata_across_both_paths() {
    for remote in [false, true] {
        let server = Server::tools(vec![
            (plugin_tool("commands", "run_command"), json!({"command":
                "git --version > version.txt || exit 90; git status --short > status.txt || exit 91; \
                 printf changed > tracked.txt || exit 92; \
                 if printf changed > .git/config; then exit 93; fi; \
                 if printf changed > .git/hooks/pre-commit; then exit 94; fi; \
                 if printf changed > .git/index; then exit 95; fi; \
                 if mv .git renamed-metadata; then exit 96; fi; printf protected"})),
            (plugin_tool("commands", "run_command"), json!({"access":"unrestricted", "command":
                "git -c user.name=Fixture -c user.email=fixture@example.invalid add tracked.txt && \
                 git -c user.name=Fixture -c user.email=fixture@example.invalid commit --quiet -m Changed && \
                 git init --quiet newly-created && \
                 git log -1 --format=%s"})),
            (plugin_tool("commands", "run_command"), json!({"command":
                "if printf changed > newly-created/.git/config; then exit 90; fi; \
                 printf cached-protection"})),
        ]).await;
        let mut fixture = Fixture::packages(remote, &server, &["commands"]).await;
        permission(&mut fixture, Permission::Full).await;
        git(&fixture.root, &["init", "--quiet"]);
        std::fs::write(fixture.root.join("tracked.txt"), "original").unwrap();
        git(&fixture.root, &["add", "tracked.txt"]);
        git(&fixture.root, &["commit", "--quiet", "-m", "Fixture"]);
        let config = std::fs::read(fixture.root.join(".git/config")).unwrap();
        let turn = approvals::rules::submit(&fixture.client, &fixture.session).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        let outputs = process::results(&fixture.client, &page).await;
        assert_eq!(outputs.len(), 3, "{page:?}");
        assert!(
            outputs
                .iter()
                .all(|output| output.outcome == Outcome::Exited(0)),
            "{outputs:?}"
        );
        assert_eq!(outputs[0].stdout.text, "protected");
        assert_eq!(outputs[1].stdout.text.trim(), "Changed");
        assert_eq!(outputs[2].stdout.text, "cached-protection");
        assert!(fixture.root.join("newly-created/.git/HEAD").is_file());
        assert!(
            !std::fs::read_to_string(fixture.root.join("newly-created/.git/config"))
                .unwrap()
                .contains("changed")
        );
        assert_eq!(
            std::fs::read(fixture.root.join(".git/config")).unwrap(),
            config
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("tracked.txt")).unwrap(),
            "changed"
        );
        assert!(!fixture.root.join(".git/hooks/pre-commit").exists());
        assert!(!fixture.root.join("renamed-metadata").exists());
        assert_eq!(page.approvals.len(), 3);
        assert_eq!(page.approvals[0].source, ApprovalSource::Sandbox);
        assert_eq!(page.approvals[0].access, Some(Access::Workspace));
        assert_eq!(page.approvals[1].source, ApprovalSource::Full);
        assert_eq!(page.approvals[1].access, Some(Access::Unrestricted));
        assert_eq!(page.approvals[2].source, ApprovalSource::Sandbox);
        assert_eq!(page.approvals[2].access, Some(Access::Workspace));
        assert!(
            page.approvals
                .iter()
                .all(|approval| approval.state == ApprovalState::Approved)
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
