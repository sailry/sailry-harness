use super::*;

#[test]
#[cfg(unix)]
fn writes_use_captured_checkout_and_receipts() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let root = fixture.directory.path().join("project");
        let manifest_path = root.join("package/plugin.json");
        let mut manifest: Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["actions"]
            .as_array_mut()
            .unwrap()
            .push(json!("git.write"));
        std::fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let package = fixture.install(0);
        let repository = git2::Repository::open(&root).unwrap();
        let mut config = repository.config().unwrap();
        config.set_str("user.name", "SDK fixture").unwrap();
        config.set_str("user.email", "sdk@example.invalid").unwrap();
        config.set_bool("commit.gpgsign", false).unwrap();
        let context = Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: package.summary.reference(),
            worktree: Some(fixture.session.worktree),
            session: Some(fixture.session.id),
        };
        let host = Host::new(
            fixture.binding.client.clone(),
            context.clone(),
            fixture.runtime.handle().clone(),
            false,
            None,
        );
        let module = host.sdk();
        module.validate().unwrap();
        let before = complete(&fixture, &module, "inspectGit", json!([]));
        let index = json!({
            "operation":"stage", "paths":["notes.txt"],
            "expected_index":before["index_revision"], "expected_head":before["head"],
        });
        let id = call(&module, "prepareGitIndex", json!([index]));
        let first = complete(&fixture, &module, "completeRequest", json!([id]));
        assert_eq!(first["Ok"]["kind"], "git_index");
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([id])),
            first
        );
        call(&module, "forgetRequest", json!([id]));
        let id = call(&module, "prepareGitIndex", json!([index]));
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([id]))["Err"]["code"],
            "revision_conflict"
        );
        call(&module, "forgetRequest", json!([id]));
        let current = complete(&fixture, &module, "inspectGit", json!([]));
        let draft = json!({
            "message":"Commit through the public SDK",
            "expected_index":current["index_revision"], "expected_head":current["head"],
            "expected_branch":current["branch"],
        });
        let mut escaped = draft.clone();
        escaped["worktree"] = json!(sailry_protocol::WorktreeId::new());
        assert!(
            module
                .call("prepareGitCommit", &args(json!([escaped])))
                .is_err()
        );
        let id = call(&module, "prepareGitCommit", json!([draft]));
        let first = complete(&fixture, &module, "completeRequest", json!([id]));
        assert_eq!(first["Ok"]["kind"], "git_commit_created");
        assert_eq!(
            complete(&fixture, &module, "completeRequest", json!([id])),
            first
        );
        assert_eq!(
            repository.head().unwrap().target().unwrap().to_string(),
            first["Ok"]["data"]["id"].as_str().unwrap()
        );
        assert!(
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|request| request.plugin.as_ref() == Some(&context))
        );
        host.close();
        assert!(module.begin("inspectGit", &args(json!([]))).is_err());
        assert!(
            module
                .call("prepareGitCommit", &args(json!([draft])))
                .is_err()
        );
        let mut unscoped = context;
        unscoped.worktree = None;
        unscoped.session = None;
        let host = Host::new(
            fixture.binding.client.clone(),
            unscoped,
            fixture.runtime.handle().clone(),
            false,
            None,
        );
        assert!(host.sdk().begin("inspectGit", &args(json!([]))).is_err());
        assert!(
            host.sdk()
                .call("prepareGitIndex", &args(json!([index])))
                .is_err()
        );
        host.close();
        fixture.close();
    }
}
