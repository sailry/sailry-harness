use super::*;

struct Fixture {
    _temporary: tempfile::TempDir,
    workspace: PathBuf,
    plan: Plan,
    verified: VerifiedBundle,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let parent = fs::canonicalize(temporary.path()).unwrap();
        let installed = parent.join("Sailry");
        let workspace = parent.join(".sailry-update-fixture");
        let recovery = parent.join("recovery");
        let staged = workspace.join("staging/Sailry");
        fs::create_dir_all(installed.join("resources")).unwrap();
        fs::write(installed.join("sailry-desktop.exe"), b"old executable").unwrap();
        fs::write(installed.join("loader.dll"), b"old library").unwrap();
        fs::write(installed.join("resources/stale"), b"old resource").unwrap();
        fs::create_dir_all(workspace.join("archive")).unwrap();
        fs::create_dir_all(staged.join("resources")).unwrap();
        fs::create_dir_all(&recovery).unwrap();
        fs::write(staged.join("sailry-desktop.exe"), b"new executable").unwrap();
        fs::write(staged.join("loader.dll"), b"new library").unwrap();
        fs::write(staged.join("resources/new"), b"new resource").unwrap();
        let archive = workspace.join("archive/Sailry-test.zip");
        fs::write(&archive, b"shared verifier fixture input").unwrap();
        let plan = Plan {
            version: 1,
            parent_pid: 42,
            parent_started: 73,
            archive,
            proof: serde_json::json!({"signed_envelope": "fixture-only"}),
            install_root: installed,
            executable: PathBuf::from("sailry-desktop.exe"),
            restart_args: vec![
                OsString::from("--data-dir"),
                OsString::from("profile with spaces"),
            ],
            receipt: recovery.join("receipt.json"),
        };
        Self {
            _temporary: temporary,
            workspace,
            verified: VerifiedBundle {
                root: staged,
                executable: plan.executable.clone(),
            },
            plan,
        }
    }
}

mod plan {
    use super::*;

    #[test]
    fn authenticated_replacement() {
        for (scenario, check) in [
            (
                "preserves_explicit_relaunch_arguments",
                (|_scenario: &str| {
                    let fixture = Fixture::new();
                    let path = fixture.workspace.join(PLAN_NAME);
                    write_json(&path, &fixture.plan).unwrap();
                    let (workspace, decoded) = read_plan(&path).unwrap();
                    assert_eq!(
                        workspace, fixture.workspace,
                        "preserves explicit relaunch arguments"
                    );
                    assert_eq!(
                        decoded.restart_args, fixture.plan.restart_args,
                        "preserves explicit relaunch arguments"
                    );
                    assert_eq!(
                        decoded.proof, fixture.plan.proof,
                        "preserves explicit relaunch arguments"
                    );
                }) as fn(&str),
            ),
            (
                "rejects_other_definitions_and_escaped_inputs",
                (|_scenario: &str| {
                    let mut fixture = Fixture::new();
                    fixture.plan.version = 2;
                    assert!(
                        validate_plan(&fixture.workspace, &fixture.plan).is_err(),
                        "rejects other definitions and escaped inputs"
                    );
                    fixture.plan.version = 1;
                    fixture.plan.executable = PathBuf::from("../sailry-desktop.exe");
                    assert!(
                        validate_plan(&fixture.workspace, &fixture.plan).is_err(),
                        "rejects other definitions and escaped inputs"
                    );
                    fixture.plan.executable = PathBuf::from("sailry-desktop.exe");
                    fixture.plan.archive = fixture.plan.install_root.join("loader.dll");
                    assert!(
                        validate_plan(&fixture.workspace, &fixture.plan).is_err(),
                        "rejects other definitions and escaped inputs"
                    );
                }),
            ),
            (
                "refuses_existing_recovery_files",
                (|_scenario: &str| {
                    let fixture = Fixture::new();
                    fs::write(&fixture.plan.receipt, b"user content").unwrap();
                    assert!(
                        validate_plan(&fixture.workspace, &fixture.plan).is_err(),
                        "refuses existing recovery files"
                    );
                    assert_eq!(
                        fs::read(&fixture.plan.receipt).unwrap(),
                        b"user content",
                        "refuses existing recovery files"
                    );
                }),
            ),
            (
                "helper_dispatch_does_not_consume_normal_launches",
                (|_scenario: &str| {
                    assert!(
                        helper_request(&[OsString::from("--preview")]).is_none(),
                        "helper dispatch does not consume normal launches"
                    );
                    assert!(
                        helper_request(&[OsString::from(HELPER_FLAG)])
                            .unwrap()
                            .is_err(),
                        "helper dispatch does not consume normal launches"
                    );
                    let request = helper_request(&[
                        OsString::from(HELPER_FLAG),
                        OsString::from("private plan"),
                    ]);
                    assert_eq!(
                        request.unwrap().unwrap(),
                        PathBuf::from("private plan"),
                        "helper dispatch does not consume normal launches"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}

mod installation {
    use super::*;

    #[test]
    fn authenticated_replacement() {
        for (scenario, check) in [
            (
                "replaces_bundle_and_retains_original",
                (|_scenario: &str| {
                    let fixture = Fixture::new();
                    install(&fixture.workspace, &fixture.plan, &fixture.verified).unwrap();
                    let installed = &fixture.plan.install_root;
                    assert_eq!(
                        fs::read(installed.join("sailry-desktop.exe")).unwrap(),
                        b"new executable",
                        "replaces bundle and retains original"
                    );
                    assert_eq!(
                        fs::read(installed.join("loader.dll")).unwrap(),
                        b"new library",
                        "replaces bundle and retains original"
                    );
                    assert_eq!(
                        fs::read(installed.join("resources/new")).unwrap(),
                        b"new resource",
                        "replaces bundle and retains original"
                    );
                    assert!(
                        !installed.join("resources/stale").exists(),
                        "replaces bundle and retains original"
                    );
                    let original = fixture.workspace.join("previous");
                    assert_eq!(
                        fs::read(original.join("sailry-desktop.exe")).unwrap(),
                        b"old executable",
                        "replaces bundle and retains original"
                    );
                    assert_eq!(
                        fs::read(original.join("loader.dll")).unwrap(),
                        b"old library",
                        "replaces bundle and retains original"
                    );
                    assert_eq!(
                        fs::read(original.join("resources/stale")).unwrap(),
                        b"old resource",
                        "replaces bundle and retains original"
                    );
                    assert!(
                        !fixture.plan.receipt.exists(),
                        "replaces bundle and retains original"
                    );
                }) as fn(&str),
            ),
            (
                "failed_move_restores_the_original_directory",
                (|_scenario: &str| {
                    let fixture = Fixture::new();
                    let backup = fixture.workspace.join("previous");
                    let error = bundle::replace(
                        &fixture.workspace.join("absent"),
                        &fixture.plan.install_root,
                        &backup,
                    )
                    .unwrap_err();
                    assert!(
                        error.contains("restored"),
                        "failed move restores the original directory"
                    );
                    assert_eq!(
                        fs::read(fixture.plan.install_root.join("loader.dll")).unwrap(),
                        b"old library",
                        "failed move restores the original directory"
                    );
                    assert!(
                        !backup.exists(),
                        "failed move restores the original directory"
                    );
                }),
            ),
            (
                "invalid_bundle_does_not_change_the_installation",
                (|_scenario: &str| {
                    let mut fixture = Fixture::new();
                    fixture.verified.root = fixture.plan.install_root.clone();
                    assert!(
                        install(&fixture.workspace, &fixture.plan, &fixture.verified).is_err(),
                        "invalid bundle does not change the installation"
                    );
                    assert_eq!(
                        fs::read(fixture.plan.install_root.join("loader.dll")).unwrap(),
                        b"old library",
                        "invalid bundle does not change the installation"
                    );
                    assert!(
                        !fixture.workspace.join("previous").exists(),
                        "invalid bundle does not change the installation"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}

mod helper_copy {
    use super::*;

    #[test]
    fn keeps_loader_libraries_and_nested_resources() {
        let fixture = Fixture::new();
        let target = fixture.workspace.join("helper");
        bundle::copy_tree(&fixture.plan.install_root, &target).unwrap();
        assert_eq!(fs::read(target.join("loader.dll")).unwrap(), b"old library");
        assert_eq!(
            fs::read(target.join("resources/stale")).unwrap(),
            b"old resource"
        );
        assert_eq!(
            fs::read(fixture.plan.install_root.join("loader.dll")).unwrap(),
            b"old library"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_links_in_a_portable_bundle() {
        let fixture = Fixture::new();
        std::os::unix::fs::symlink("loader.dll", fixture.plan.install_root.join("linked.dll"))
            .unwrap();
        assert!(
            bundle::copy_tree(
                &fixture.plan.install_root,
                &fixture.workspace.join("helper")
            )
            .is_err()
        );
        assert!(bundle::validate_tree(&fixture.plan.install_root).is_err());
        assert!(fixture.plan.install_root.join("loader.dll").is_file());
    }
}

mod receipt {
    use super::*;

    #[test]
    fn authenticated_replacement() {
        for (scenario, check) in [
            (
                "records_authenticated_application_version",
                (|_scenario: &str| {
                    let fixture = Fixture::new();
                    write_receipt(
                        &fixture.plan.receipt,
                        "success",
                        None,
                        Some("9.9.9-alpha.2"),
                    )
                    .unwrap();
                    let bytes = fs::read(&fixture.plan.receipt).unwrap();
                    let receipt: Value = serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(
                        receipt["application_version"], "9.9.9-alpha.2",
                        "records authenticated application version"
                    );
                    assert!(
                        receipt["installed"].as_bool().unwrap(),
                        "records authenticated application version"
                    );
                }) as fn(&str),
            ),
            (
                "reports_outcomes_without_overwriting_state",
                (|_scenario: &str| {
                    let fixture = Fixture::new();
                    write_receipt(
                        &fixture.plan.receipt,
                        "failure",
                        Some("installation is uncertain"),
                        None,
                    )
                    .unwrap();
                    let bytes = fs::read(&fixture.plan.receipt).unwrap();
                    let receipt: Value = serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(
                        receipt["version"], 1,
                        "reports outcomes without overwriting state"
                    );
                    assert_eq!(
                        receipt["result"], "failure",
                        "reports outcomes without overwriting state"
                    );
                    assert_eq!(
                        receipt["message"], "installation is uncertain",
                        "reports outcomes without overwriting state"
                    );
                    assert!(
                        write_receipt(
                            &fixture.plan.receipt,
                            "success",
                            None,
                            Some("9.9.9-alpha.2")
                        )
                        .is_err(),
                        "reports outcomes without overwriting state"
                    );
                    assert_eq!(
                        fs::read(&fixture.plan.receipt).unwrap(),
                        bytes,
                        "reports outcomes without overwriting state"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
