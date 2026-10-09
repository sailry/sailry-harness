#![cfg(any(target_os = "macos", target_os = "linux"))]
use super::*;
use adk_sandbox::sandbox::AccessMode;
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

const INJECTIONS: [&str; 7] = [
    "LD_PRELOAD",
    "LD_AUDIT",
    "LD_LIBRARY_PATH",
    "LD_BIND_NOW",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
];

#[path = "tests/repositories.rs"]
mod repositories;

async fn environment(home: &Path) -> Environment {
    Environment::capture(super::super::environment::Source::Fixture(
        super::super::environment::EnvironmentFixture {
            home: home.to_owned(),
            shell: PathBuf::from("/usr/bin/false"),
            environment: BTreeMap::from([("PATH".into(), "/usr/bin:/bin".into())]),
            timeout: Duration::from_millis(100),
        },
    ))
    .await
}

fn launch(root: &Path, profile: &Path) -> Launch {
    Launch {
        pty: false,
        root: root.to_owned(),
        profile: Some(profile.to_owned()),
        command: "true".into(),
        access: Access::Workspace,
        cwd: String::new(),
        timeout_ms: 1000,
        inputs: None,
        scratch: None,
        python: None,
    }
}

fn roots(profile: &Path) -> Vec<(PathBuf, PathBuf)> {
    use sailry_protocol::{DatabaseId, SessionId, SshId, WorktreeId, connection::Resource, plugin};
    let binding = plugin::conversation::Binding {
        package: plugin::Reference {
            name: "fixture".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
        id: "assistant".into(),
    };
    let mut other = binding.clone();
    other.id = "other".into();
    vec![
        (
            profile
                .join("worktrees")
                .join(WorktreeId::new().to_string()),
            profile
                .join("worktrees")
                .join(WorktreeId::new().to_string()),
        ),
        (
            crate::workspaces::session(profile, SessionId::new()),
            crate::workspaces::session(profile, SessionId::new()),
        ),
        (
            crate::workspaces::connection(profile, Resource::Ssh(SshId::new())),
            crate::workspaces::connection(profile, Resource::Ssh(SshId::new())),
        ),
        (
            crate::workspaces::connection(profile, Resource::Database(DatabaseId::new())),
            crate::workspaces::connection(profile, Resource::Database(DatabaseId::new())),
        ),
        (
            crate::workspaces::assistant(profile, &binding).unwrap(),
            crate::workspaces::assistant(profile, &other).unwrap(),
        ),
    ]
}

#[tokio::test]
async fn grants_only_worktree_and_scratch_writes() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let root = directory.path().join("work");
    let profile = directory.path().join("node");
    for path in [&home, &root, &profile] {
        std::fs::create_dir(path).unwrap();
    }
    for relative in [
        ".cargo/bin",
        ".cargo/registry/src",
        ".cargo/registry/cache",
        ".rustup/toolchains",
        ".ssh",
    ] {
        std::fs::create_dir_all(home.join(relative)).unwrap();
    }
    std::fs::write(home.join(".cargo/credentials.toml"), "protected").unwrap();
    std::fs::write(home.join(".ssh/id_ed25519"), "protected").unwrap();
    let environment = environment(&home).await;
    let sandbox = Sandbox::prepare(&launch(&root, &profile), &environment, None)
        .unwrap()
        .unwrap();
    assert!(!sandbox.policy.allow_network);
    assert!(sandbox.policy.allow_process_spawn);
    let writes: Vec<_> = sandbox
        .policy
        .allowed_paths
        .iter()
        .filter(|entry| entry.mode == AccessMode::ReadWrite)
        .map(|entry| entry.path.clone())
        .collect();
    assert_eq!(writes.len(), 2);
    assert!(writes.contains(&root.canonicalize().unwrap()));
    assert!(writes.contains(&sandbox.scratch().canonicalize().unwrap()));
    let home = home.canonicalize().unwrap();
    let scratch = sandbox.scratch().canonicalize().unwrap();
    for entry in &sandbox.policy.allowed_paths {
        assert_ne!(entry.path, home.canonicalize().unwrap());
        assert!(!entry.path.starts_with(profile.canonicalize().unwrap()));
        assert!(
            !home
                .join(".cargo/credentials.toml")
                .starts_with(&entry.path)
        );
        assert!(!home.join(".ssh/id_ed25519").starts_with(&entry.path));
    }
    for name in [
        "HOME",
        "CARGO_HOME",
        "XDG_CONFIG_HOME",
        "npm_config_cache",
        "TMPDIR",
    ] {
        assert!(
            Path::new(sandbox.variables.get(std::ffi::OsStr::new(name)).unwrap())
                .starts_with(&scratch)
        );
    }
    assert_eq!(
        std::fs::canonicalize(
            Path::new(
                sandbox
                    .variables
                    .get(std::ffi::OsStr::new("CARGO_HOME"))
                    .unwrap()
            )
            .join("registry/src")
        )
        .unwrap(),
        home.join(".cargo/registry/src").canonicalize().unwrap()
    );
}

#[tokio::test]
async fn rejects_protected_workspace_roots() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let profile = directory.path().join("node");
    std::fs::create_dir_all(home.join(".ssh/project")).unwrap();
    std::fs::create_dir(&profile).unwrap();
    let environment = environment(&home).await;
    for root in [
        directory.path(),
        home.as_path(),
        profile.as_path(),
        Path::new("/"),
    ] {
        let error = Sandbox::prepare(&launch(root, &profile), &environment, None)
            .err()
            .expect("protected root accepted");
        assert_eq!(error.code, sailry_protocol::ErrorCode::PermissionDenied);
    }
    let registered = home.join(".ssh/project");
    assert!(
        Sandbox::prepare(&launch(&registered, &profile), &environment, None)
            .unwrap()
            .is_some()
    );
    let mut request = launch(&home, &profile);
    request.access = Access::Unrestricted;
    assert!(
        Sandbox::prepare(&request, &environment, None)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn confines_managed_workspace_grants() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let profile = directory.path().join("node");
    for (root, sibling) in roots(&profile) {
        for path in [&home, &root, &sibling] {
            std::fs::create_dir_all(path).unwrap();
        }
        let environment = environment(&home).await;
        let sandbox = Sandbox::prepare(&launch(&root, &profile), &environment, None)
            .unwrap()
            .unwrap();
        let root = root.canonicalize().unwrap();
        let profile = profile.canonicalize().unwrap();
        let sibling = sibling.canonicalize().unwrap();
        assert!(!sandbox.policy.allow_network);
        let writes = sandbox
            .policy
            .allowed_paths
            .iter()
            .filter(|entry| entry.mode == AccessMode::ReadWrite)
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        assert_eq!(writes.len(), 2);
        assert!(writes.contains(&root));
        assert!(writes.contains(&sandbox.scratch().canonicalize().unwrap()));
        for entry in &sandbox.policy.allowed_paths {
            assert!(!profile.starts_with(&entry.path));
            assert!(!sibling.starts_with(&entry.path));
            assert!(!profile.join("storage/private.txt").starts_with(&entry.path));
        }
        for rejected in [
            profile.clone(),
            profile.join("worktrees"),
            profile.join("workspaces"),
            profile.join("workspaces/sessions"),
            root.parent().unwrap().to_owned(),
            profile.join("worktrees/not-an-id"),
            profile.join("worktrees/00000000-0000-0000-0000-000000000000"),
            profile.join("workspaces/sessions/00000000-0000-0000-0000-000000000000"),
            profile.join("connections/ssh/00000000-0000-0000-0000-000000000000/workspace"),
            profile.join("connections/database/00000000-0000-0000-0000-000000000000/workspace"),
            profile.join("assistants/not-a-hash/workspace"),
            profile
                .join("storage")
                .join(sailry_protocol::WorktreeId::new().to_string()),
            profile
                .join("tools")
                .join(sailry_protocol::WorktreeId::new().to_string()),
            root.join("nested"),
        ] {
            std::fs::create_dir_all(&rejected).unwrap();
            let error = Sandbox::prepare(&launch(&rejected, &profile), &environment, None)
                .err()
                .expect("protected Node path accepted");
            assert_eq!(error.code, sailry_protocol::ErrorCode::PermissionDenied);
        }
        std::fs::remove_dir(&sibling).unwrap();
        std::os::unix::fs::symlink(profile.join("storage"), &sibling).unwrap();
        let error = Sandbox::prepare(&launch(&sibling, &profile), &environment, None)
            .err()
            .expect("protected symlink target accepted");
        assert_eq!(error.code, sailry_protocol::ErrorCode::PermissionDenied);
    }
}

#[tokio::test]
async fn runs_inside_managed_workspaces() {
    use sailry_link::CancellationToken;
    use sailry_protocol::process::Outcome;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let profile = directory.path().join("node");
    for (root, sibling) in roots(&profile) {
        for path in [&home, &root, &sibling, &profile.join("storage")] {
            std::fs::create_dir_all(path).unwrap();
        }
        let profile = profile.canonicalize().unwrap();
        let root = root.canonicalize().unwrap();
        let sibling = sibling.canonicalize().unwrap();
        let private = [
            profile.join("private.txt"),
            profile.join("storage/private.txt"),
            sibling.join("private.txt"),
        ];
        for path in &private {
            std::fs::write(path, "protected").unwrap();
        }
        let environment = environment(&home).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let serving = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            assert!(stream.read(&mut request).await.unwrap() > 0);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\nreachable",
                )
                .await
                .unwrap();
            listener
        });
        let curl = which::which_in("curl", environment.path(), &root)
            .expect("sandbox acceptance requires system curl");
        let curl = format!("'{}'", curl.to_str().unwrap().replace('\'', "'\\''"));
        let quote = |path: &Path| format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"));
        let targets = private
            .iter()
            .map(|path| quote(path))
            .collect::<Vec<_>>()
            .join(" ");
        let escaped = quote(&profile.join("escaped.txt"));
        let mut baseline = launch(&root, &profile);
        baseline.access = Access::Unrestricted;
        baseline.timeout_ms = 5000;
        baseline.command = format!(
            "cat {} && {curl} --silent --max-time 2 http://{address}/",
            quote(&private[0])
        );
        let output = super::super::execute(
            baseline,
            &environment,
            &CancellationToken::new(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(output.outcome, Outcome::Exited(0), "{output:?}");
        assert_eq!(output.stdout.text, "protectedreachable");
        let listener = tokio::time::timeout(Duration::from_secs(5), serving)
            .await
            .unwrap()
            .unwrap();
        let mut request = launch(&root, &profile);
        request.timeout_ms = 5000;
        request.command = format!(
            "printf managed > marker.txt || exit 90; \
         printf '%s' \"$PWD\" > cwd.txt || exit 91; \
         for target in {targets}; do \
           if cat \"$target\" > blocked-read.txt 2>/dev/null; then exit 92; fi; \
           if (printf changed > \"$target\") 2>/dev/null; then exit 93; fi; \
         done; \
         if (printf escaped > {escaped}) 2>/dev/null; then exit 94; fi; \
         if {curl} --silent --max-time 1 http://{address}/ > network.txt; then exit 95; fi; \
         printf confined"
        );
        let output = super::super::execute(
            request,
            &environment,
            &CancellationToken::new(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(output.outcome, Outcome::Exited(0), "{output:?}");
        assert_eq!(output.stdout.text, "confined");
        assert_eq!(
            std::fs::read_to_string(root.join("marker.txt")).unwrap(),
            "managed"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("cwd.txt")).unwrap(),
            root.to_str().unwrap()
        );
        for path in private {
            assert_eq!(std::fs::read_to_string(path).unwrap(), "protected");
        }
        assert!(!profile.join("escaped.txt").exists());
        assert_eq!(root.join("blocked-read.txt").metadata().unwrap().len(), 0);
        assert_eq!(root.join("network.txt").metadata().unwrap().len(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn preserves_login_arguments_through_wrapper() {
    let pair = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let device = pair.master.tty_name();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("work");
    let profile = directory.path().join("node");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&profile).unwrap();
    let environment = environment(directory.path()).await;
    let sandbox = Sandbox::prepare(&launch(&root, &profile), &environment, None)
        .unwrap()
        .unwrap();
    let mut command = portable_pty::CommandBuilder::new_default_prog();
    command.env("SHELL", "/bin/sh");
    sandbox.terminal(&mut command, device.as_deref()).unwrap();
    assert_eq!(command.get_argv()[0], sandbox.wrapper.as_os_str());
    assert!(
        command
            .get_argv()
            .ends_with(&["/bin/sh".into(), "-l".into()])
    );
    let mut command = portable_pty::CommandBuilder::new("/bin/bash");
    command.args(["--posix", "--login", "-i"]);
    sandbox.terminal(&mut command, device.as_deref()).unwrap();
    assert!(command.get_argv().ends_with(&[
        "/bin/bash".into(),
        "--posix".into(),
        "--login".into(),
        "-i".into()
    ]));
}

#[tokio::test]
async fn confines_terminal_loader_environment() {
    let pair = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let device = pair.master.tty_name();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("work");
    let profile = directory.path().join("node");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&profile).unwrap();
    let environment = environment(directory.path()).await;
    for access in [Access::Workspace, Access::Unrestricted] {
        let mut request = launch(&root, &profile);
        request.access = access;
        let sandbox = Sandbox::prepare(&request, &environment, None).unwrap();
        let mut command = portable_pty::CommandBuilder::new_default_prog();
        environment.apply_terminal(&mut command);
        command.env("SHELL", "/bin/sh");
        command.env("SAILRY_FIXTURE_KEEP", "ordinary value");
        command.env("MY_LD_PRELOAD", "ordinary name");
        for key in INJECTIONS {
            command.env(key, "/workspace/fixture-library");
        }
        if let Some(sandbox) = &sandbox {
            sandbox.apply_terminal(&mut command);
            sandbox.terminal(&mut command, device.as_deref()).unwrap();
            assert_eq!(command.get_argv()[0], sandbox.wrapper.as_os_str());
            for key in INJECTIONS {
                assert!(command.get_env(key).is_none(), "{key}");
            }
        } else {
            assert_eq!(access, Access::Unrestricted);
            assert!(command.is_default_prog());
            for key in INJECTIONS {
                assert_eq!(
                    command.get_env(key),
                    Some(std::ffi::OsStr::new("/workspace/fixture-library"))
                );
            }
        }
        assert_eq!(
            command.get_env("SAILRY_FIXTURE_KEEP"),
            Some(std::ffi::OsStr::new("ordinary value"))
        );
        assert_eq!(
            command.get_env("MY_LD_PRELOAD"),
            Some(std::ffi::OsStr::new("ordinary name"))
        );
    }
}

#[tokio::test]
async fn confines_command_loader_environment() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("work");
    let profile = directory.path().join("node");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&profile).unwrap();
    let environment = environment(directory.path()).await;
    for access in [Access::Workspace, Access::Unrestricted] {
        let mut request = launch(&root, &profile);
        request.access = access;
        let sandbox = Sandbox::prepare(&request, &environment, None).unwrap();
        let mut command = environment.command(&request, &root, None);
        command.env("SAILRY_FIXTURE_KEEP", "ordinary value");
        command.env("MY_LD_PRELOAD", "ordinary name");
        for key in INJECTIONS {
            command.env(key, "/workspace/fixture-library");
        }
        let command = match &sandbox {
            Some(sandbox) => sandbox.command(command).unwrap(),
            None => command,
        };
        let source = command.as_std();
        let values = source.get_envs().collect::<BTreeMap<_, _>>();
        if let Some(sandbox) = &sandbox {
            assert_eq!(source.get_program(), sandbox.wrapper.as_os_str());
            for key in INJECTIONS {
                assert!(!values.contains_key(std::ffi::OsStr::new(key)), "{key}");
            }
            assert!(
                source
                    .get_args()
                    .map(std::ffi::OsString::from)
                    .collect::<Vec<_>>()
                    .ends_with(&[
                        environment.shell().into(),
                        "-c".into(),
                        request.command.clone().into(),
                    ])
            );
        } else {
            assert_eq!(access, Access::Unrestricted);
            assert_eq!(source.get_program(), environment.shell());
            for key in INJECTIONS {
                assert_eq!(
                    values.get(std::ffi::OsStr::new(key)),
                    Some(&Some(std::ffi::OsStr::new("/workspace/fixture-library"))),
                );
            }
        }
        assert_eq!(source.get_current_dir(), Some(root.as_path()));
        for (key, value) in [
            ("SAILRY_FIXTURE_KEEP", "ordinary value"),
            ("MY_LD_PRELOAD", "ordinary name"),
        ] {
            assert_eq!(
                values.get(std::ffi::OsStr::new(key)),
                Some(&Some(std::ffi::OsStr::new(value))),
            );
        }
    }
}
