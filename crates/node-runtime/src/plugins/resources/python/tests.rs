use super::*;

#[tokio::test]
async fn only_required_dependencies_need_a_runtime() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("profile");
    std::fs::create_dir(&profile).unwrap();
    let host = Host::new(Some(profile.canonicalize().unwrap()));
    host.set_python_runtime_fixture(&directory.path().join("missing-python"))
        .unwrap();
    let mut packages = Vec::new();
    for (name, required) in [("optional", false), ("required", true)] {
        let source = directory.path().join(name);
        std::fs::create_dir_all(source.join("skills/review")).unwrap();
        std::fs::write(
            source.join("plugin.json"),
            serde_json::json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":name,"version":"1.0.0"}).to_string(),
        ).unwrap();
        std::fs::write(
            source.join("skills/review/SKILL.md"),
            "---\nname: review\ndescription: Review source\n---\nReview project code\n",
        )
        .unwrap();
        if required {
            std::fs::write(
                source.join("requirements.txt"),
                "example==1.0 --hash=sha256:abcd\n",
            )
            .unwrap();
        }
        packages.push(
            host.install(&source.canonicalize().unwrap(), "", name)
                .unwrap(),
        );
    }
    let resources = Resources::new(host, packages, Default::default(), CancellationToken::new());
    let environment = Environment::ambient();
    assert!(
        resources
            .python("optional:review", &environment)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        resources
            .python("required:review", &environment)
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::NotConfigured
    );
}
