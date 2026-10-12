use super::*;

#[test]
fn observes_only_prepared_language_requests() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let path = fixture.directory.path().join("project/package/plugin.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["actions"] =
            json!(["code.read", "code.manage"]);
        std::fs::write(path, manifest.to_string()).unwrap();
        let package = fixture.install(0);
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let host = Host::new(
            fixture.binding.client.clone(),
            Context {
                invocation: None,
                turn: None,
                surface: Default::default(),
                package: package.summary.reference(),
                worktree: Some(fixture.session.worktree),
                session: Some(fixture.session.id),
            },
            fixture.runtime.handle().clone(),
            false,
            None,
        );
        let sdk = host.sdk();
        let tasks = host.installations_module();
        let install = Command::InstallCodeServer {
            install: sailry_protocol::code::Install {
                package: "fixture-server".into(),
                version: "1".into(),
                purl: "pkg:generic/fixture-server@1".into(),
                urls: Vec::new(),
                licenses: Vec::new(),
                command: None,
                revision: "a".repeat(64),
            },
        };
        let id = call(&sdk, "prepareRequest", json!([install]));
        let request = host
            .prepared(id.as_str().unwrap().parse().unwrap())
            .unwrap();
        assert_eq!(request.command, install);
        assert_eq!(request.plugin, Some(host.context()));
        assert_eq!(complete(&fixture, &tasks, "read", json!([[id]])), json!([]));
        assert!(
            !fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| matches!(request.command, Command::InstallCodeServer { .. }))
        );
        assert!(
            tasks
                .begin("read", &args(json!([[RequestId::new()]])))
                .is_err()
        );
        let unrelated = call(&sdk, "prepareRequest", json!([Command::ReadCodeSettings]));
        assert!(tasks.begin("read", &args(json!([[unrelated]]))).is_err());
        call(&sdk, "forgetRequest", json!([id]));
        assert!(tasks.begin("read", &args(json!([[id]]))).is_err());
        host.close();
        fixture.close();
    }
}
