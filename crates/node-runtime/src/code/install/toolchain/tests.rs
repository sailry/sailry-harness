use super::*;
use crate::code::{
    install::plan,
    rules::{Package, Rules},
    tests::Fixture,
};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn relocates_all_supported_toolchain_launchers() {
    let fixture = Fixture::new().await;
    let python = which::which("python3").unwrap();
    let script = format!(
        "#!{}\n{}",
        python.display(),
        include_str!("tests/manager.py")
    );
    for executable in ["npm", "python3", "cargo", "go"] {
        let path = fixture.bin.join(executable);
        std::fs::write(&path, &script).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    for (manager, namespace) in [
        ("npm", "fixture"),
        ("pypi", "fixture"),
        ("cargo", "fixture"),
        ("golang", "example.invalid/fixture"),
    ] {
        let entry = Package {
            name: format!("fixture-{manager}"),
            source: json!({"id":format!("pkg:{manager}/{namespace}@1.0")}),
            bin: BTreeMap::from([("fixture".into(), format!("{manager}:fixture"))]),
            homepage: None,
            licenses: vec!["MIT".into()],
        };
        let consent = plan::package(&entry, &fixture.environment).install.unwrap();
        let rules = Rules {
            languages: vec![],
            servers: BTreeMap::new(),
            packages: vec![entry],
        };
        let plan = plan::prepare(&rules, &consent, &fixture.environment).unwrap();
        let stage = fixture._directory.path().join(format!("stage-{manager}"));
        let destination = fixture
            ._directory
            .path()
            .join(format!("installed-{manager}"));
        std::fs::create_dir(&stage).unwrap();
        install(
            &plan,
            &stage,
            &destination,
            &fixture.environment,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        std::fs::rename(&stage, &destination).unwrap();
        let output = tokio::process::Command::new(destination.join("bin/fixture"))
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "ready");
        assert!(!stage.exists());
    }
}

#[tokio::test]
async fn terminates_the_installer_process_group_on_cancel() {
    let fixture = Fixture::new().await;
    let python = which::which("python3").unwrap();
    let executable = fixture.bin.join("hang");
    std::fs::write(&executable,format!("#!{}\nimport subprocess,sys,pathlib,time\nchild=subprocess.Popen([sys.executable,'-c','import time;time.sleep(3600)'])\npathlib.Path('child.pid').write_text(str(child.pid))\ntime.sleep(3600)\n",python.display())).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let stop = CancellationToken::new();
    let cancelled = stop.clone();
    let environment = fixture.environment.clone();
    let root = fixture.root.clone();
    let task = tokio::spawn(async move {
        run(
            &executable,
            &[],
            &root,
            &environment,
            &BTreeMap::new(),
            &cancelled,
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !fixture.root.join("child.pid").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let pid = std::fs::read_to_string(fixture.root.join("child.pid"))
        .unwrap()
        .parse::<i32>()
        .unwrap();
    stop.cancel();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = tokio::process::Command::new("ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()
                .await
                .unwrap();
            if !status.status.success()
                || String::from_utf8_lossy(&status.stdout)
                    .trim_start()
                    .starts_with('Z')
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
