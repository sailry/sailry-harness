use super::*;

#[test]
fn normalizes_windows_names() {
    for (key, expected) in [
        ("Path", "PATH"),
        ("ComSpec", "COMSPEC"),
        ("pathext", "PATHEXT"),
        ("SystemRoot", "SystemRoot"),
    ] {
        assert_eq!(canonical(OsStr::new(key), true), Some(expected));
    }
    for key in [
        "OPENAI_API_KEY",
        "openai_api_key",
        "SAILRY_FIXTURE_SECRET",
        "node_options",
    ] {
        assert_eq!(canonical(OsStr::new(key), true), None);
    }
}

#[test]
fn preserves_unix_name_case() {
    for key in ["PATH", "COMSPEC", "PATHEXT", "SystemRoot"] {
        assert_eq!(canonical(OsStr::new(key), false), Some(key));
    }
    for key in ["Path", "ComSpec", "pathext", "systemroot", "OPENAI_API_KEY"] {
        assert_eq!(canonical(OsStr::new(key), false), None);
    }
}

#[test]
fn binds_filtered_launch_values() {
    let initial = Environment::filtered([("PATH".into(), "/fixture/bin".into())]);
    let changed = Environment::filtered([("PATH".into(), "/other/bin".into())]);
    let ignored = Environment::filtered([
        ("PATH".into(), "/fixture/bin".into()),
        ("OPENAI_API_KEY".into(), "private".into()),
        ("SSH_AUTH_SOCK".into(), "/fixture/agent".into()),
    ]);
    assert_ne!(initial.identity(), changed.identity());
    assert_eq!(initial.identity(), ignored.identity());
}

#[test]
fn filters_sensitive_values() {
    let environment = Environment::filtered(
        [
            ("PATH", "/fixture/bin"),
            ("LANG", "en_US.UTF-8"),
            ("SSH_AUTH_SOCK", "/fixture/agent"),
            ("LC_MESSAGES", "en_US.UTF-8"),
            ("SAILRY_FIXTURE_SECRET", "private"),
            ("OPENAI_API_KEY", "private"),
            ("NODE_OPTIONS", "--require /parent/integration.js"),
        ]
        .map(|(key, value)| (key.into(), value.into())),
    );
    for key in ["SAILRY_FIXTURE_SECRET", "OPENAI_API_KEY", "NODE_OPTIONS"] {
        assert!(!environment.values.contains_key(OsStr::new(key)));
        assert!(!environment.terminal.contains_key(OsStr::new(key)));
    }
    for key in ["SSH_AUTH_SOCK", "LC_MESSAGES"] {
        assert!(!environment.values.contains_key(OsStr::new(key)));
        assert!(environment.terminal.contains_key(OsStr::new(key)));
    }
    assert_eq!(environment.path(), Some(OsStr::new("/fixture/bin")));
    assert_eq!(environment.values[OsStr::new("LANG")], "en_US.UTF-8");
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::{ffi::OsStringExt, fs::PermissionsExt};

    fn shell(directory: &Path, script: &str) -> PathBuf {
        let path = directory.join("fixture-shell");
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn source(directory: &Path, shell: PathBuf, timeout: Duration) -> Source {
        Source::Fixture(EnvironmentFixture {
            home: directory.into(),
            shell,
            environment: [
                ("PATH", "/ambient/bin"),
                ("HOME", "/parent/home"),
                ("LANG", "en_US.UTF-8"),
                ("SSH_AUTH_SOCK", "/fixture/agent"),
                ("SAILRY_FIXTURE_SECRET", "private"),
            ]
            .map(|(key, value)| (key.into(), value.into()))
            .into(),
            timeout,
        })
    }

    async fn pid(path: &Path) -> i32 {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(text) = std::fs::read_to_string(path)
                    && let Ok(pid) = text.parse()
                {
                    return pid;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture probe did not start")
    }

    async fn gone(pid: i32) {
        tokio::time::timeout(Duration::from_secs(3), async {
            // SAFETY: signal 0 only observes this fixture's exact process ID.
            while unsafe { libc::kill(pid, 0) } == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture probe process was not reaped")
    }

    #[test]
    fn isolates_parent_integration() {
        let account = Account {
            name: "fixture".into(),
            home: "/fixture/home".into(),
            shell: "/bin/zsh".into(),
        };
        let inherited = [
            ("HOME", "/other/home"),
            ("SHELL", "/bin/sh"),
            ("ZDOTDIR", "/parent/integration"),
            ("STARSHIP_SHELL", "zsh"),
            ("DMUX_TERMINAL_ID", "parent"),
            ("GHOSTTY_RESOURCES_DIR", "/parent/ghostty"),
            ("SSH_AUTH_SOCK", "/session/agent"),
            ("LANG", "en_US.UTF-8"),
        ];
        let mut environment =
            Environment::filtered(inherited.map(|(key, value)| (key.into(), value.into())));
        account.apply(&mut environment.terminal);
        let values = &environment.terminal;
        for key in [
            "ZDOTDIR",
            "STARSHIP_SHELL",
            "DMUX_TERMINAL_ID",
            "GHOSTTY_RESOURCES_DIR",
        ] {
            assert!(!values.contains_key(OsStr::new(key)));
        }
        for (key, expected) in [
            ("HOME", "/fixture/home"),
            ("SHELL", "/bin/zsh"),
            ("SSH_AUTH_SOCK", "/session/agent"),
            ("LANG", "en_US.UTF-8"),
        ] {
            assert_eq!(values.get(OsStr::new(key)).unwrap(), expected);
        }
        for key in ["USER", "LOGNAME"] {
            assert_eq!(values.get(OsStr::new(key)).unwrap(), "fixture");
        }
    }

    #[test]
    fn parses_framed_values() {
        let bytes = b"Startup banner\n\0fixture\0PATH=/fixture/\xff/bin\0LANG=en_US.UTF-8\0VALUE=first=second\nthird\0EMPTY=\0\0fixture-end\0Trailing banner\n";
        let values = login::parse(bytes, b"fixture").unwrap();
        assert_eq!(values.len(), 4);
        assert_eq!(
            values[OsStr::new("PATH")],
            OsString::from_vec(b"/fixture/\xff/bin".to_vec())
        );
        assert_eq!(values[OsStr::new("LANG")], "en_US.UTF-8");
        assert_eq!(values[OsStr::new("VALUE")], "first=second\nthird");
        assert_eq!(values[OsStr::new("EMPTY")], "");
    }

    #[test]
    fn rejects_invalid_frames() {
        for bytes in [
            b"PATH=/fixture/bin\0\0fixture-end\0".as_slice(),
            b"\0fixture\0PATH=/fixture/bin\0".as_slice(),
            b"\0fixture\0PATH=/fixture/bin\0\0different-end\0".as_slice(),
            b"\0fixture\0BROKEN\0\0fixture-end\0".as_slice(),
            b"\0fixture\0=value\0\0fixture-end\0".as_slice(),
        ] {
            assert!(login::parse(bytes, b"fixture").is_none(), "{bytes:?}");
        }
    }

    #[tokio::test]
    async fn shares_captured_path() {
        let directory = tempfile::tempdir().unwrap();
        let executable = shell(
            directory.path(),
            r#"
printf 'Startup banner\n'
printf x >> captured.count
export PATH=/fixture/login/bin:/usr/bin:/bin
export SAILRY_FIXTURE_SECRET=private
export GHOSTTY_RESOURCES_DIR=/parent/integration
for argument do payload=$argument; done
eval "$payload"
"#,
        );
        let environment = Environment::capture(source(
            directory.path(),
            executable.clone(),
            Duration::from_secs(2),
        ))
        .await;
        let expected = OsStr::new("/fixture/login/bin:/usr/bin:/bin");
        assert_eq!(environment.path(), Some(expected));
        assert_eq!(environment.login_shell(), Some(executable.as_os_str()));
        assert_eq!(
            environment.terminal[OsStr::new("HOME")],
            directory.path().as_os_str()
        );
        let mut command = tokio::process::Command::new("/bin/sh");
        command.env("SAILRY_FIXTURE_SECRET", "parent");
        environment.apply(&mut command);
        let inputs: BTreeMap<_, _> = command.as_std().get_envs().collect();
        assert_eq!(inputs.get(OsStr::new("PATH")), Some(&Some(expected)));
        assert!(!inputs.contains_key(OsStr::new("SAILRY_FIXTURE_SECRET")));
        let mut terminal = portable_pty::CommandBuilder::new("/bin/sh");
        terminal.env("SAILRY_FIXTURE_SECRET", "parent");
        environment.apply_terminal(&mut terminal);
        assert_eq!(terminal.get_env("PATH"), Some(expected));
        for key in ["SAILRY_FIXTURE_SECRET", "GHOSTTY_RESOURCES_DIR"] {
            assert!(!environment.values.contains_key(OsStr::new(key)));
            assert!(terminal.get_env(key).is_none());
        }
        assert_eq!(
            std::fs::read_to_string(directory.path().join("captured.count")).unwrap(),
            "x"
        );
    }

    #[tokio::test]
    async fn falls_back_on_probe_failure() {
        for script in ["exit 9", "printf 'Unframed output\\n'"] {
            let directory = tempfile::tempdir().unwrap();
            let executable = shell(directory.path(), script);
            let environment = Environment::capture(source(
                directory.path(),
                executable.clone(),
                Duration::from_secs(2),
            ))
            .await;
            assert_eq!(environment.path(), Some(OsStr::new("/ambient/bin")));
            assert_eq!(environment.terminal[OsStr::new("PATH")], "/ambient/bin");
            assert_eq!(
                environment.values[OsStr::new("HOME")],
                directory.path().as_os_str()
            );
            assert_eq!(environment.login_shell(), Some(executable.as_os_str()));
            assert!(
                !environment
                    .values
                    .contains_key(OsStr::new("SAILRY_FIXTURE_SECRET"))
            );
        }
    }

    #[tokio::test]
    async fn bounds_probe_output_and_runtime() {
        for (script, descendant) in [
            (
                r#"
printf '%s' "$$" > root.pid
export PATH=/fixture/oversized/bin
for argument do payload=$argument; done
eval "$payload"
/usr/bin/head -c 1048577 /dev/zero
exit 0
"#,
                false,
            ),
            (
                r#"
printf '%s' "$$" > root.pid
/bin/sleep 60 &
printf '%s' "$!" > child.pid
wait
"#,
                true,
            ),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let executable = shell(directory.path(), script);
            let started = tokio::time::Instant::now();
            let environment = Environment::capture(source(
                directory.path(),
                executable,
                Duration::from_millis(500),
            ))
            .await;
            assert!(started.elapsed() < Duration::from_secs(1));
            assert_eq!(environment.path(), Some(OsStr::new("/ambient/bin")));
            gone(pid(&directory.path().join("root.pid")).await).await;
            if descendant {
                gone(pid(&directory.path().join("child.pid")).await).await;
            }
        }
    }

    #[tokio::test]
    async fn cancels_entire_probe_group() {
        let directory = tempfile::tempdir().unwrap();
        let executable = shell(
            directory.path(),
            r#"
printf '%s' "$$" > root.pid
/bin/sleep 60 &
printf '%s' "$!" > child.pid
wait
"#,
        );
        let fixture = source(directory.path(), executable, Duration::from_secs(3));
        let running = tokio::spawn(Environment::capture(fixture));
        let root = pid(&directory.path().join("root.pid")).await;
        let child = pid(&directory.path().join("child.pid")).await;
        running.abort();
        assert!(matches!(running.await, Err(error) if error.is_cancelled()));
        gone(root).await;
        gone(child).await;
    }
}
