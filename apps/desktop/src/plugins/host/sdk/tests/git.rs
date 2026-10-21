use super::*;

#[test]
fn unfiled_reads_keep_the_captured_directory() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let Output::Plugin(package) = fixture.execute(Command::ReadPlugin { name: "git".into() })
        else {
            panic!("plugin expected");
        };
        let create = || Command::CreateSession {
            project: None,
            worktree: None,
            config: Some(fixture.session.config.clone()),
        };
        let Output::Session(session) = fixture.execute(create()) else {
            panic!("session expected");
        };
        let Output::Session(other) = fixture.execute(create()) else {
            panic!("session expected");
        };
        let workspace = |id: sailry_protocol::SessionId| {
            fixture
                .node
                .profile()
                .join("workspaces/sessions")
                .join(id.to_string())
        };
        let root = workspace(session.id);
        assert!(!root.exists());
        assert!(!workspace(other.id).exists());
        let hosts: Vec<_> = [Some(session.id), None]
            .into_iter()
            .map(|session_id| {
                Host::new(
                    fixture.binding.client.clone(),
                    Context {
                        invocation: None,
                        turn: None,
                        surface: Default::default(),
                        package: package.summary.reference(),
                        worktree: Some(session.worktree),
                        session: session_id,
                    },
                    fixture.runtime.handle().clone(),
                    false,
                    None,
                )
            })
            .collect();
        let modules: Vec<_> = hosts.iter().map(|host| host.sdk()).collect();
        let reads: Vec<_> = modules
            .iter()
            .flat_map(|module| {
                [
                    ("inspectGit", json!([])),
                    ("listGitBranches", json!([])),
                    ("readGitLog", json!([100, null])),
                ]
                .into_iter()
                .map(move |(name, values)| module.begin(name, &args(values)).unwrap())
            })
            .collect();
        let results = fixture.runtime.block_on(futures::future::join_all(reads));
        for result in results {
            let value = decode(&result.unwrap()).unwrap();
            assert_eq!(value["kind"], "directory", "remote={remote}: {value}");
            assert_eq!(value["entries"], json!([]));
        }
        assert!(root.is_dir());
        assert!(!root.join(".git").exists());
        assert!(!workspace(other.id).exists());
        {
            let requests = fixture.transport.requests.lock().unwrap();
            let contexts: Vec<_> = requests
                .iter()
                .filter_map(|request| request.plugin.as_ref())
                .filter(|context| context.package == package.summary.reference())
                .collect();
            assert_eq!(contexts.len(), 6);
            assert!(contexts.iter().all(|context| {
                context.worktree == Some(session.worktree)
                    && [Some(session.id), None].contains(&context.session)
            }));
        }
        std::fs::write(root.join(".git"), "invalid repository metadata\n").unwrap();
        for module in &modules {
            for (name, values) in [
                ("inspectGit", json!([])),
                ("listGitBranches", json!([])),
                ("readGitLog", json!([100, null])),
            ] {
                assert!(
                    fixture
                        .runtime
                        .block_on(module.begin(name, &args(values)).unwrap())
                        .is_err(),
                    "damaged metadata must fail: {name}, remote={remote}"
                );
            }
        }
        for host in hosts {
            host.close();
        }
        fixture.close();
    }
}

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
