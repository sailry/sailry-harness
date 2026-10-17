use super::super::{Access, Environment, Launch, Sandbox};
use crate::process::{
    environment::{EnvironmentFixture, Source},
    execute,
};
use adk_sandbox::sandbox::AccessMode;
use sailry_link::CancellationToken;
use sailry_protocol::process::Outcome;
use std::{
    collections::BTreeMap,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    time::Duration,
};

struct Fixture {
    _directory: tempfile::TempDir,
    home: PathBuf,
    root: PathBuf,
    profile: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().canonicalize().unwrap();
        let home = path.join("home");
        let root = path.join("work");
        let profile = path.join("node");
        for path in [&home, &root, &profile] {
            std::fs::create_dir(path).unwrap();
        }
        for relative in [
            ".ssh/id_ed25519",
            ".gitconfig",
            ".cargo/credentials.toml",
            ".local/share/keyrings/default",
            ".local/lib/private/data",
        ] {
            write(&home.join(relative), "protected");
        }
        write(&profile.join("credentials"), "protected");
        Self {
            _directory: directory,
            home,
            root,
            profile,
        }
    }

    fn launch(&self) -> Launch {
        Launch {
            pty: false,
            root: self.root.clone(),
            profile: Some(self.profile.clone()),
            command: "server > result.txt".into(),
            access: Access::Workspace,
            cwd: String::new(),
            timeout_ms: 5000,
            inputs: None,
            scratch: None,
            python: None,
        }
    }

    async fn environment(&self, paths: &[&Path]) -> Environment {
        let path = std::env::join_paths(
            paths
                .iter()
                .copied()
                .chain([Path::new("/usr/bin"), Path::new("/bin")]),
        )
        .unwrap();
        Environment::capture(Source::Fixture(EnvironmentFixture {
            home: self.home.clone(),
            shell: "/usr/bin/false".into(),
            environment: BTreeMap::from([("PATH".into(), path)]),
            timeout: Duration::from_millis(100),
        }))
        .await
    }

    async fn execute(&self, environment: &Environment) {
        let output = execute(
            self.launch(),
            environment,
            &CancellationToken::new(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(output.outcome, Outcome::Exited(0), "{output:?}");
        assert_eq!(
            std::fs::read_to_string(self.root.join("result.txt")).unwrap(),
            "ready"
        );
    }

    #[cfg(target_os = "macos")]
    async fn git(
        &self,
        environment: &Environment,
        sandbox: &Sandbox,
        arguments: &[&std::ffi::OsStr],
    ) -> std::process::Output {
        let mut command = tokio::process::Command::new("/usr/bin/git");
        environment.apply(&mut command);
        command
            .current_dir(&self.root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(["--no-pager", "-c", "core.hooksPath=/dev/null"])
            .args(arguments);
        let mut command = sandbox.command(command).unwrap();
        command.kill_on_drop(true);
        // A fresh private xcrun database must discover Apple's selected tools first.
        // Keep that bounded startup separate from the five-second Git operations.
        let startup = arguments == [std::ffi::OsStr::new("--version")];
        let limit = Duration::from_secs(if startup { 20 } else { 5 });
        let output = tokio::time::timeout(limit, command.output())
            .await
            .unwrap_or_else(|error| panic!("{arguments:?} exceeded {limit:?}: {error}"))
            .unwrap();
        if startup {
            assert!(output.stdout.starts_with(b"git version"), "{output:?}");
        }
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            developer_diagnostics(&String::from_utf8_lossy(&output.stderr)),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn boundaries(&self, sandbox: &Sandbox) {
        assert!(!sandbox.policy.allow_network);
        assert_eq!(
            sandbox
                .policy
                .allowed_paths
                .iter()
                .filter(|entry| entry.mode == AccessMode::ReadWrite)
                .count(),
            2
        );
        for protected in [
            self.home.clone(),
            self.home.join(".ssh/id_ed25519"),
            self.home.join(".gitconfig"),
            self.home.join(".cargo/credentials.toml"),
            self.home.join(".local/share/keyrings/default"),
            self.home.join(".local/lib/private/data"),
            self.profile.join("credentials"),
        ] {
            assert!(
                sandbox
                    .policy
                    .allowed_paths
                    .iter()
                    .all(|entry| !protected.starts_with(&entry.path)),
                "{}",
                protected.display()
            );
        }
        assert_ne!(
            Path::new(sandbox.variables.get(std::ffi::OsStr::new("HOME")).unwrap()),
            self.home
        );
    }
}

fn write(path: &Path, value: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, value).unwrap();
}

fn script(path: &Path, value: &str) {
    write(path, value);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn interpreter(path: &Path, library: &Path) {
    script(
        path,
        &format!(
            "#!/bin/sh\nset -eu\n[ \"$(cat '{}')\" = runtime ]\nexec /bin/sh \"$@\"\n",
            library.display()
        ),
    );
}

fn server(path: &Path, fixture: &Fixture) {
    script(
        path,
        &format!(
            "#!/usr/bin/env python3\nset -eu\n[ \"$HOME\" != '{}' ]\n\
        if cat '{}'/.ssh/id_ed25519 >/dev/null 2>&1; then exit 91; fi\n\
        if cat '{}'/.gitconfig >/dev/null 2>&1; then exit 92; fi\n\
        if (printf escaped > '{}'/escaped) 2>/dev/null; then exit 93; fi\n\
        printf ready\n",
            fixture.home.display(),
            fixture.home.display(),
            fixture.home.display(),
            fixture.profile.display()
        ),
    );
}

#[tokio::test]
async fn reads_installed_interpreters_and_libraries() {
    for relative in [
        "Library/FlyEnv/env/python",
        ".nvm/versions/node/fixture",
        ".pyenv/versions/fixture",
        ".local/share/uv/python/fixture",
        ".local/share/pipx/venvs/fixture",
    ] {
        let fixture = Fixture::new();
        let prefix = fixture.home.join(relative);
        let bin = prefix.join("bin");
        let library = prefix.join("lib/runtime.txt");
        write(&library, "runtime");
        interpreter(&bin.join("python3"), &library);
        server(&bin.join("server"), &fixture);
        let environment = fixture.environment(&[&bin]).await;
        let sandbox = Sandbox::prepare(&fixture.launch(), &environment, None)
            .unwrap()
            .unwrap();
        fixture.boundaries(&sandbox);
        fixture.execute(&environment).await;
        assert_eq!(std::fs::read_to_string(&library).unwrap(), "runtime");
        let mut command = fixture.launch();
        command.command = format!(
            "if (printf changed > '{}') 2>/dev/null; then exit 94; fi",
            library.display()
        );
        let output = execute(
            command,
            &environment,
            &CancellationToken::new(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(output.outcome, Outcome::Exited(0), "{output:?}");
    }
}

#[tokio::test]
async fn follows_symlinks_without_exposing_sibling_data() {
    let fixture = Fixture::new();
    let runtime = fixture.home.parent().unwrap().join("runtime");
    let library = runtime.join("lib/runtime.txt");
    write(&library, "runtime");
    write(&runtime.join("pyvenv.cfg"), "fixture");
    interpreter(&runtime.join("bin/python3"), &library);
    let bin = fixture.home.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let intermediate = fixture.home.join("interpreter-link");
    symlink(runtime.join("bin/python3"), &intermediate).unwrap();
    symlink(&intermediate, bin.join("python3")).unwrap();
    symlink(fixture.home.join(".gitconfig"), bin.join("credential-link")).unwrap();
    server(&bin.join("server"), &fixture);
    let alias = fixture.home.join("bin-alias");
    symlink(&bin, &alias).unwrap();
    let environment = fixture.environment(&[&alias]).await;
    let sandbox = Sandbox::prepare(&fixture.launch(), &environment, None)
        .unwrap()
        .unwrap();
    fixture.boundaries(&sandbox);
    fixture.execute(&environment).await;
}

#[tokio::test]
async fn reads_global_package_dependencies() {
    let fixture = Fixture::new();
    let runtime = fixture.home.join("Library/FlyEnv/env/python");
    let library = runtime.join("lib/runtime.txt");
    write(&library, "runtime");
    interpreter(&runtime.join("bin/python3"), &library);
    let package = fixture.home.join(".npm-global/lib/node_modules/fixture");
    let module = package.join("lib/module.txt");
    write(&module, "module");
    script(
        &package.join("server"),
        &format!(
            "#!/usr/bin/env python3\nset -eu\n[ \"$(cat '{}')\" = module ]\nprintf ready\n",
            module.display()
        ),
    );
    let bin = fixture.home.join(".npm-global/bin");
    std::fs::create_dir(&bin).unwrap();
    symlink(package.join("server"), bin.join("server")).unwrap();
    let environment = fixture.environment(&[&bin, &runtime.join("bin")]).await;
    let sandbox = Sandbox::prepare(&fixture.launch(), &environment, None)
        .unwrap()
        .unwrap();
    fixture.boundaries(&sandbox);
    fixture.execute(&environment).await;
}

#[tokio::test]
async fn keeps_project_tooling_writable() {
    let fixture = Fixture::new();
    let bin = fixture.root.join("bin");
    script(&bin.join("server"), "#!/bin/sh\nprintf ready\n");
    let environment = fixture.environment(&[&bin]).await;
    fixture.execute(&environment).await;
    let mut command = fixture.launch();
    command.command = "printf updated > bin/server".into();
    let output = execute(
        command,
        &environment,
        &CancellationToken::new(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(output.outcome, Outcome::Exited(0), "{output:?}");
    assert_eq!(
        std::fs::read_to_string(bin.join("server")).unwrap(),
        "updated"
    );
}

#[tokio::test]
async fn honors_server_path_overrides() {
    let fixture = Fixture::new();
    let runtime = fixture.home.join(".local/share/pipx/venvs/fixture");
    let library = runtime.join("lib/runtime.txt");
    write(&library, "runtime");
    interpreter(&runtime.join("bin/python3"), &library);
    server(&fixture.root.join("server"), &fixture);
    let environment = fixture.environment(&[]).await;
    let sandbox = Sandbox::prepare(&fixture.launch(), &environment, None)
        .unwrap()
        .unwrap();
    let mut command = tokio::process::Command::new(fixture.root.join("server"));
    environment.apply(&mut command);
    command.current_dir(&fixture.root).env(
        "PATH",
        std::env::join_paths([runtime.join("bin"), "/usr/bin".into(), "/bin".into()]).unwrap(),
    );
    let mut command = sandbox.command(command).unwrap();
    command.kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"ready");
}

#[cfg(target_os = "macos")]
fn developer_diagnostics(stderr: &str) -> bool {
    stderr.lines().all(|line| {
        line.is_empty()
            || line.ends_with("DVTFilePathFSEvents: Failed to start fs event stream.")
            || line.contains("Requested but did not find extension point with identifier ")
                && line.contains(" for extension ")
                && line.contains(" of plug-in ")
    })
}

#[cfg(target_os = "macos")]
#[test]
fn distinguishes_benign_developer_diagnostics() {
    assert!(developer_diagnostics(""));
    assert!(developer_diagnostics(
        "xcodebuild[1:2] DVTFilePathFSEvents: Failed to start fs event stream.\n\
         xcodebuild[1:2] Requested but did not find extension point with identifier Xcode.Example for extension Xcode.Example.plugin of plug-in com.example.Tool\n"
    ));
    for message in [
        "xcrun: error: unable to find utility clang",
        "dyld: Library not loaded: private framework",
        "Permission denied",
        "Operation not permitted",
        "xcodebuild: Couldn't load directory contents: Toolchains",
    ] {
        assert!(!developer_diagnostics(message), "accepted {message}");
    }
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn confines_registered_developer_toolchains() {
    let fixture = Fixture::new();
    let toolchains = fixture.home.join("Library/Developer/Toolchains");
    let runtime = toolchains.join("fixture.xctoolchain/usr/lib/runtime.txt");
    let private = fixture.home.join("Library/Developer/Private/token");
    write(&runtime, "runtime");
    write(&private, "protected");
    let environment = fixture.environment(&[]).await;
    let sandbox = Sandbox::prepare(&fixture.launch(), &environment, None)
        .unwrap()
        .unwrap();
    fixture.boundaries(&sandbox);
    assert!(
        sandbox
            .policy
            .allowed_paths
            .iter()
            .any(|entry| { entry.path == toolchains && entry.mode == AccessMode::ReadOnly })
    );
    assert!(
        sandbox
            .policy
            .allowed_paths
            .iter()
            .all(|entry| { !private.starts_with(&entry.path) })
    );
    let mut request = fixture.launch();
    request.command = format!(
        "set -eu; [ \"$(cat '{}')\" = runtime ]; \
         if (printf changed > '{}') 2>/dev/null; then exit 91; fi; \
         if cat '{}' >/dev/null 2>&1; then exit 92; fi; \
         if cat '{}' >/dev/null 2>&1; then exit 93; fi",
        runtime.display(),
        runtime.display(),
        private.display(),
        fixture.home.join(".ssh/id_ed25519").display(),
    );
    let output = execute(
        request,
        &environment,
        &CancellationToken::new(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(output.outcome, Outcome::Exited(0), "{output:?}");
    assert_eq!(std::fs::read_to_string(runtime).unwrap(), "runtime");
    assert_eq!(std::fs::read_to_string(private).unwrap(), "protected");
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn runs_developer_tools_with_private_cache() {
    let fixture = Fixture::new();
    // Apple's tools resolve the account home independently of the HOME variable.
    // Capture that account while retaining a private HOME and isolated command environment.
    let home = dirs::home_dir()
        .expect("native account home")
        .canonicalize()
        .unwrap();
    let environment = Environment::capture(Source::Fixture(EnvironmentFixture {
        home: home.clone(),
        shell: "/usr/bin/false".into(),
        environment: BTreeMap::from([("PATH".into(), "/usr/bin:/bin".into())])
            .into_iter()
            .chain(std::env::var_os("DEVELOPER_DIR").map(|value| ("DEVELOPER_DIR".into(), value)))
            .collect(),
        timeout: Duration::from_millis(100),
    }))
    .await;
    if environment.developer().is_none()
        && !Path::new("/Applications/Xcode.app/Contents/Developer/usr/bin/git").is_file()
        && !Path::new("/Library/Developer/CommandLineTools/usr/bin/git").is_file()
    {
        return;
    }
    let sandbox = Sandbox::prepare(&fixture.launch(), &environment, None)
        .unwrap()
        .unwrap();
    fixture.boundaries(&sandbox);
    for protected in [
        home.clone(),
        home.join(".ssh"),
        home.join(".gitconfig"),
        home.join("Library/Developer/Private"),
    ] {
        assert!(
            sandbox
                .policy
                .allowed_paths
                .iter()
                .all(|entry| !protected.starts_with(&entry.path)),
            "{}",
            protected.display()
        );
    }
    assert_ne!(
        Path::new(sandbox.variables.get(std::ffi::OsStr::new("HOME")).unwrap()),
        home,
    );
    let cache = Path::new(
        sandbox
            .variables
            .get(std::ffi::OsStr::new("xcrun_db"))
            .unwrap(),
    );
    assert!(cache.starts_with(sandbox.scratch().canonicalize().unwrap()));
    for (command, prefix) in [
        ("/usr/bin/git --version", "git version"),
        ("/usr/bin/xcrun --find clang", "/"),
        ("/usr/bin/xcrun --find python3", "/"),
    ] {
        let mut request = fixture.launch();
        request.command = command.into();
        if command == "/usr/bin/git --version" {
            request.timeout_ms = 20_000;
        }
        let output = execute(
            request,
            &environment,
            &CancellationToken::new(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(output.outcome, Outcome::Exited(0), "{command}: {output:?}");
        assert!(output.stdout.text.starts_with(prefix), "{output:?}");
        assert!(developer_diagnostics(&output.stderr.text), "{output:?}");
    }

    let source = fixture.home.parent().unwrap().join("source");
    let repository = git2::Repository::init_opts(
        &source,
        git2::RepositoryInitOptions::new()
            .external_template(false)
            .initial_head("main"),
    )
    .unwrap();
    write(&source.join("readme.txt"), "source content\n");
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("readme.txt")).unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let author = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let commit = repository
        .commit(Some("HEAD"), &author, &author, "Initial", &tree, &[])
        .unwrap();
    let sandbox = Sandbox::prepare(&fixture.launch(), &environment, Some(&source))
        .unwrap()
        .unwrap();
    fixture.boundaries(&sandbox);
    assert!(
        sandbox
            .policy
            .allowed_paths
            .iter()
            .any(|entry| { entry.path == source && entry.mode == AccessMode::ReadOnly })
    );
    fixture
        .git(&environment, &sandbox, &["--version".as_ref()])
        .await;
    let url = url::Url::from_directory_path(&source).unwrap();
    let target = fixture.root.join("checkout");
    let arguments = [
        "-c".as_ref(),
        "protocol.file.allow=always".as_ref(),
        "clone".as_ref(),
        "--quiet".as_ref(),
        "--no-hardlinks".as_ref(),
        "--".as_ref(),
        url.as_str().as_ref(),
        target.as_os_str(),
    ];
    let command = || {
        let mut command = tokio::process::Command::new("/usr/bin/git");
        environment.apply(&mut command);
        command
            .current_dir(&fixture.root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(["--no-pager", "-c", "core.hooksPath=/dev/null"])
            .args(arguments);
        command.kill_on_drop(true);
        command
    };
    let denied = tokio::time::timeout(
        Duration::from_secs(5),
        sandbox.command(command()).unwrap().output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!denied.status.success(), "{denied:?}");
    let diagnostics = String::from_utf8_lossy(&denied.stderr);
    let denial = format!("{}: Operation not permitted", target.join(".git").display());
    assert!(
        diagnostics.lines().any(|line| line == denial),
        "{diagnostics}"
    );
    let developer = diagnostics
        .lines()
        .filter(|line| *line != denial)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(developer_diagnostics(&developer), "{diagnostics}");
    assert!(!target.join(".git").exists());
    let mut request = fixture.launch();
    request.access = Access::Unrestricted;
    assert!(
        Sandbox::prepare(&request, &environment, Some(&source))
            .unwrap()
            .is_none()
    );
    let cloned = tokio::time::timeout(Duration::from_secs(5), command().output())
        .await
        .unwrap()
        .unwrap();
    assert!(cloned.status.success(), "{cloned:?}");
    let sandbox = Sandbox::prepare(&fixture.launch(), &environment, Some(&source))
        .unwrap()
        .unwrap();
    fixture
        .git(&environment, &sandbox, &["--version".as_ref()])
        .await;
    assert_eq!(
        std::fs::read_to_string(target.join("readme.txt")).unwrap(),
        "source content\n"
    );
    let head = fixture
        .git(
            &environment,
            &sandbox,
            &[
                "-C".as_ref(),
                target.as_os_str(),
                "rev-parse".as_ref(),
                "HEAD".as_ref(),
            ],
        )
        .await;
    assert_eq!(
        String::from_utf8(head.stdout).unwrap().trim(),
        commit.to_string()
    );
    let arguments = [
        "-C".as_ref(),
        target.as_os_str(),
        "status".as_ref(),
        "--short".as_ref(),
        "--untracked-files=all".as_ref(),
    ];
    let clean = fixture.git(&environment, &sandbox, &arguments).await;
    assert!(clean.stdout.is_empty(), "{clean:?}");
    write(&target.join("readme.txt"), "changed content\n");
    let changed = fixture.git(&environment, &sandbox, &arguments).await;
    assert_eq!(changed.stdout, b" M readme.txt\n");
    assert_eq!(
        std::fs::read_to_string(source.join("readme.txt")).unwrap(),
        "source content\n"
    );
}

#[tokio::test]
async fn retains_shim_version_selection() {
    for manager in ["pyenv", "asdf", "mise", "volta"] {
        let fixture = Fixture::new();
        let (relative, variable, parts, metadata, dispatch) = match manager {
            "pyenv" => (
                ".pyenv",
                "PYENV_ROOT",
                "versions/fixture",
                ".pyenv/version",
                "[ \"$(cat \"$PYENV_ROOT/version\")\" = fixture ]\nexec \"$PYENV_ROOT/versions/fixture/bin/python3\" \"$@\"",
            ),
            "asdf" => (
                ".asdf",
                "ASDF_DATA_DIR",
                "installs/python/fixture",
                ".tool-versions",
                "[ \"$(cat \"$HOME/.tool-versions\")\" = fixture ]\nexec \"$ASDF_DATA_DIR/installs/python/fixture/bin/python3\" \"$@\"",
            ),
            "mise" => (
                ".local/share/mise",
                "MISE_DATA_DIR",
                "installs/python/fixture",
                ".config/mise/config.toml",
                "[ \"$(cat \"$XDG_CONFIG_HOME/mise/config.toml\")\" = fixture ]\nexec \"$MISE_DATA_DIR/installs/python/fixture/bin/python3\" \"$@\"",
            ),
            "volta" => (
                ".volta",
                "VOLTA_HOME",
                "tools/image/fixture",
                ".volta/tools/user/default",
                "[ \"$(cat \"$VOLTA_HOME/tools/user/default\")\" = fixture ]\nexec \"$VOLTA_HOME/tools/image/fixture/bin/python3\" \"$@\"",
            ),
            _ => unreachable!(),
        };
        let manager_root = fixture.home.join(relative);
        let runtime = manager_root.join(parts);
        let library = runtime.join("lib/runtime.txt");
        write(&library, "runtime");
        write(&runtime.join("pyvenv.cfg"), "fixture");
        interpreter(&runtime.join("bin/python3"), &library);
        write(&fixture.home.join(metadata), "fixture");
        let shims = manager_root.join(if manager == "volta" { "bin" } else { "shims" });
        script(
            &shims.join("python3"),
            &format!("#!/bin/sh\nset -eu\n{dispatch}\n"),
        );
        server(&shims.join("server"), &fixture);
        let environment = fixture.environment(&[&shims]).await;
        let sandbox = Sandbox::prepare(&fixture.launch(), &environment, None)
            .unwrap()
            .unwrap();
        fixture.boundaries(&sandbox);
        assert_eq!(
            Path::new(
                sandbox
                    .variables
                    .get(std::ffi::OsStr::new(variable))
                    .unwrap()
            ),
            manager_root
        );
        fixture.execute(&environment).await;
        assert_eq!(
            std::fs::read_to_string(fixture.home.join(metadata)).unwrap(),
            "fixture"
        );
    }
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn confines_selected_developer_directory() {
    let fixture = Fixture::new();
    let application = fixture.root.parent().unwrap().join("Selected.app");
    let contents = application.join("Contents");
    let developer = contents.join("Developer");
    let executable = developer.join("usr/bin/xcodebuild");
    let framework = contents.join("SharedFrameworks/fixture");
    let unrelated = contents.join("Private/secret");
    for path in [&executable, &framework, &unrelated] {
        write(path, "protected");
    }
    let environment = Environment::capture(Source::Fixture(EnvironmentFixture {
        home: fixture.home.clone(),
        shell: "/usr/bin/false".into(),
        environment: BTreeMap::from([
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("DEVELOPER_DIR".into(), application.clone().into_os_string()),
        ]),
        timeout: Duration::from_millis(100),
    }))
    .await;
    let mut request = fixture.launch();
    let selected = developer.canonicalize().unwrap();
    request.command = format!(
        "test \"$DEVELOPER_DIR\" = '{}' || exit 90; \
         cat '{}' '{}' > result.txt || exit 91; \
         if printf changed > '{}'; then exit 92; fi; \
         if cat '{}'; then exit 93; fi",
        selected.display(),
        executable.display(),
        framework.display(),
        executable.display(),
        unrelated.display(),
    );
    let sandbox = Sandbox::prepare(&request, &environment, None)
        .unwrap()
        .unwrap();
    fixture.boundaries(&sandbox);
    assert!(sandbox.policy.allowed_paths.iter().all(|entry| {
        !application.starts_with(&entry.path)
            && (!entry.path.starts_with(&application) || entry.mode == AccessMode::ReadOnly)
    }));
    let output = execute(
        request,
        &environment,
        &CancellationToken::new(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(output.outcome, Outcome::Exited(0), "{output:?}");
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("result.txt")).unwrap(),
        "protectedprotected"
    );
    assert_eq!(std::fs::read_to_string(executable).unwrap(), "protected");
}
