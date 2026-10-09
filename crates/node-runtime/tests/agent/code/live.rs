use futures::FutureExt;
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::{EnvironmentFixture, Node};
use sailry_protocol::{Command, Output, WorktreeId, code};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    os::unix::fs::PermissionsExt,
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
    time::Duration,
};
use sysinfo::{Pid, System};

struct Package {
    name: &'static str,
    language: &'static str,
    path: &'static str,
    files: &'static [(&'static str, &'static str)],
    tools: &'static [&'static str],
    hover: Option<(u32, &'static str, &'static str)>,
    diagnostics: &'static [&'static str],
}

const RUST: Package = Package {
    name: "rust-analyzer",
    language: "rust",
    path: "src/main.rs",
    files: &[
        (
            "Cargo.toml",
            "[package]\nname = \"sailry_live\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        (
            "src/main.rs",
            "fn increment(value: i32) -> i32 { value + 1 }\nfn main() {\n    let broken: i32 = \"wrong\";\n    let _result = increment(1);\n}\n",
        ),
    ],
    tools: &["cargo", "rustc"],
    hover: Some((1, "increment", "i32")),
    diagnostics: &["E0308", "mismatched-types"],
};

const TYPESCRIPT: Package = Package {
    name: "typescript-language-server",
    language: "typescript",
    path: "main.ts",
    files: &[
        (
            "package.json",
            "{\"name\":\"sailry-live\",\"private\":true}\n",
        ),
        (
            "tsconfig.json",
            "{\"compilerOptions\":{\"strict\":true},\"files\":[\"main.ts\"]}\n",
        ),
        (
            "main.ts",
            "export function increment(value: number): number { return value + 1; }\nconst broken: number = \"wrong\";\nincrement(1);\n",
        ),
    ],
    tools: &["node", "npm"],
    hover: Some((1, "increment", "number")),
    diagnostics: &["2322"],
};

const PYTHON: Package = Package {
    name: "ruff",
    language: "python",
    path: "main.py",
    files: &[
        (
            "pyproject.toml",
            "[project]\nname = \"sailry-live\"\nversion = \"0.1.0\"\n",
        ),
        ("main.py", "import os\nmissing_name()\n"),
    ],
    tools: &["python3"],
    hover: None,
    diagnostics: &["F821"],
};

struct Fixture {
    directory: tempfile::TempDir,
    profile: PathBuf,
    root: PathBuf,
    node: Node,
    controller: Link,
    client: Client,
    worktree: WorktreeId,
    original: Value,
}

fn original(name: &str) -> Value {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(include_bytes!(
        "../../../src/code/upstream/registry.json.zip"
    )))
    .unwrap();
    let mut text = String::new();
    archive
        .by_name("registry.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    serde_json::from_str::<Vec<Value>>(&text)
        .unwrap()
        .into_iter()
        .find(|package| package["name"] == name)
        .expect("package must exist in the original embedded registry")
}

async fn tool(name: &str) -> PathBuf {
    if matches!(name, "cargo" | "rustc")
        && let Ok(rustup) = which::which("rustup")
    {
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::process::Command::new(rustup)
                .args(["which", name])
                .output(),
        )
        .await
        .expect("installed Rust toolchain lookup timed out")
        .unwrap();
        assert!(
            result.status.success(),
            "existing {name} toolchain is required: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        return PathBuf::from(String::from_utf8(result.stdout).unwrap().trim())
            .canonicalize()
            .unwrap();
    }
    which::which(name)
        .unwrap_or_else(|error| panic!("existing {name} toolchain is required: {error}"))
        .canonicalize()
        .unwrap()
}

fn launcher(destination: &Path, program: &Path) {
    let quoted = program.to_str().unwrap().replace('\'', "'\\''");
    std::fs::write(destination, format!("#!/bin/sh\nexec '{quoted}' \"$@\"\n")).unwrap();
    std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o700)).unwrap();
}

impl Fixture {
    async fn new(package: &Package, remote: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let base = directory.path().canonicalize().unwrap();
        let profile = base.join("node");
        let root = base.join("project");
        let bin = root.join("toolchains");
        let home = base.join("home");
        let temporary = base.join("tmp");
        for path in [&root, &bin, &home, &temporary] {
            std::fs::create_dir(path).unwrap();
        }
        for (path, text) in package.files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        // PATH contains only launchers for existing toolchains, never installed servers.
        for name in package.tools {
            launcher(&bin.join(name), &tool(name).await);
            let version = tokio::time::timeout(
                Duration::from_secs(10),
                tokio::process::Command::new(bin.join(name))
                    .arg("--version")
                    .env_clear()
                    .env("PATH", &bin)
                    .env("HOME", &home)
                    .env("TMPDIR", &temporary)
                    .output(),
            )
            .await
            .expect("existing toolchain did not respond")
            .unwrap();
            assert!(
                version.status.success(),
                "{name} is unavailable in the isolated environment: {}",
                String::from_utf8_lossy(&version.stderr)
            );
        }
        for server in ["rust-analyzer", "typescript-language-server", "ruff"] {
            assert!(which::which_in(server, Some(bin.as_os_str()), &root).is_err());
        }
        let node = Node::start_with_environment(
            &profile,
            EnvironmentFixture {
                home,
                shell: "/bin/false".into(),
                environment: BTreeMap::from([
                    ("PATH".into(), bin.into_os_string()),
                    ("LANG".into(), "C".into()),
                    ("TMPDIR".into(), temporary.into_os_string()),
                ]),
                timeout: Duration::from_secs(1),
            },
        )
        .await
        .unwrap();
        let controller = Link::controller(base.join("controller"), NetworkScope::default())
            .await
            .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let Output::Project(project) = client
            .execute(client.prepare(Command::RegisterProject {
                name: "Live language service".into(),
                path: root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert!(snapshot.providers.is_empty());
        assert!(snapshot.sessions.is_empty());
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|worktree| worktree.project == Some(project.id) && worktree.main)
            .unwrap()
            .id;
        let Output::CodeSettings(mut settings) = client
            .execute(client.prepare(Command::ReadCodeSettings))
            .await
            .unwrap()
        else {
            panic!("settings expected")
        };
        let expected_revision = settings.revision;
        settings
            .languages
            .insert(package.language.into(), vec![package.name.into()]);
        client
            .execute(client.prepare(Command::SaveCodeSettings {
                settings,
                expected_revision,
            }))
            .await
            .unwrap();
        Self {
            directory,
            profile,
            root,
            node,
            controller,
            client,
            worktree,
            original: original(package.name),
        }
    }

    fn version(&self) -> &str {
        self.original["source"]["id"]
            .as_str()
            .unwrap()
            .rsplit_once('@')
            .unwrap()
            .1
    }

    fn managed(&self, package: &Package) -> PathBuf {
        self.profile
            .join("tools/lsp")
            .join(package.name)
            .join(self.version())
    }

    async fn execute(&self, command: Command) -> Output {
        self.client
            .execute(self.client.prepare(command))
            .await
            .unwrap()
    }

    async fn read(&self, query: code::Query) -> code::Result {
        let Output::Code(result) = self
            .execute(Command::ReadCode {
                worktree: self.worktree,
                query,
                overlay: None,
            })
            .await
        else {
            panic!("code expected")
        };
        result
    }

    async fn packages(&self) -> code::Workspace {
        let Output::CodePackages(workspace) = self
            .execute(Command::ListCodePackages {
                worktree: self.worktree,
            })
            .await
        else {
            panic!("packages expected")
        };
        workspace
    }

    async fn exercise(&self, package: &Package) {
        let query = code::Query::Diagnostics {
            path: package.path.into(),
            severity: None,
        };
        let cold = self.read(query.clone()).await;
        assert_eq!(cold.status, code::Status::NotInstalled, "{cold:?}");
        assert!(cold.startup.is_none());
        assert!(!cold.startup_approved);
        let info = cold
            .package_info
            .expect("original registry must offer this package");
        assert_eq!(info.name, package.name);
        assert_eq!(info.version, self.version());
        let install = info
            .install
            .unwrap_or_else(|| panic!("installation is unavailable: {:?}", info.unavailable));
        assert_eq!(
            install.purl,
            self.original["source"]["id"].as_str().unwrap()
        );
        assert_eq!(
            install.licenses,
            serde_json::from_value::<Vec<String>>(self.original["licenses"].clone()).unwrap()
        );
        let workspace = self.packages().await;
        let candidate = workspace
            .languages
            .iter()
            .flat_map(|language| &language.servers)
            .find(|candidate| candidate.server == package.name)
            .expect("package candidate expected");
        assert_eq!(candidate.status, code::Status::NotInstalled);
        assert_eq!(
            candidate.package.as_ref().unwrap().install.as_ref(),
            Some(&install)
        );
        assert!(!self.managed(package).exists());
        assert_eq!(
            self.execute(Command::ListCodeServers {
                worktree: self.worktree
            })
            .await,
            Output::CodeServers(vec![])
        );
        if let Some(extra) = self.original["source"].get("extra_packages") {
            let command = install
                .command
                .as_ref()
                .expect("npm consent must describe the command");
            for extra in extra.as_array().unwrap() {
                assert!(
                    command
                        .iter()
                        .any(|argument| argument == extra.as_str().unwrap())
                );
            }
        } else {
            let source = install
                .purl
                .strip_prefix("pkg:github/")
                .expect("original archive source must be GitHub");
            let (repository, version) = source.rsplit_once('@').unwrap();
            let prefix = format!("https://github.com/{repository}/releases/download/{version}/");
            assert!(!install.urls.is_empty());
            assert!(install.urls.iter().all(|url| url.starts_with(&prefix)));
        }
        let request = self.client.prepare(Command::InstallCodeServer {
            install: install.clone(),
        });
        assert!(
            request.plugin.is_none(),
            "installation must have explicit controller consent"
        );
        let admission = self.client.dispatch(request).await.unwrap();
        assert!(admission.receipt.durable);
        let Output::CodeInstallation(installed) =
            tokio::time::timeout(Duration::from_secs(10 * 60), admission.completion)
                .await
                .expect("real installation timed out")
                .unwrap()
                .unwrap()
        else {
            panic!("installation expected")
        };
        assert_eq!(installed.status, code::InstallStatus::Ready);
        let receipt = installed.receipt.unwrap();
        assert_eq!(receipt.purl, install.purl);
        assert_eq!(receipt.revision, install.revision);
        assert_eq!(
            receipt
                .downloads
                .iter()
                .map(|download| &download.url)
                .collect::<Vec<_>>(),
            install.urls.iter().collect::<Vec<_>>()
        );
        assert!(
            receipt
                .downloads
                .iter()
                .all(|download| download.sha256.len() == 64)
        );
        for extra in self.original["source"]["extra_packages"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let (name, version) = extra.as_str().unwrap().rsplit_once('@').unwrap();
            let metadata: Value = serde_json::from_slice(
                &std::fs::read(
                    self.managed(package)
                        .join("node_modules")
                        .join(name)
                        .join("package.json"),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(metadata["version"].as_str(), Some(version));
        }
        let cold = self.read(query.clone()).await;
        assert_eq!(cold.status, code::Status::Stopped, "{cold:?}");
        assert!(!cold.startup_approved);
        let startup = cold
            .startup
            .expect("installed server must offer exact startup consent");
        assert_eq!(startup.worktree, self.worktree);
        assert_eq!(startup.server, package.name);
        assert!(Path::new(&startup.executable).starts_with(self.managed(package)));
        assert!(
            identities(&self.managed(package)).is_empty(),
            "reading must not start the installed server"
        );
        let request = self.client.prepare(Command::StartCodeServer {
            startup: startup.clone(),
        });
        assert!(request.plugin.is_none());
        let admission = self.client.dispatch(request).await.unwrap();
        assert!(admission.receipt.durable);
        let Output::CodeServer(server) = admission.completion.await.unwrap().unwrap() else {
            panic!("server expected")
        };
        assert_eq!(server.startup, startup);
        let workspace = self.packages().await;
        let candidate = workspace
            .languages
            .iter()
            .flat_map(|language| &language.servers)
            .find(|candidate| candidate.server == package.name)
            .unwrap();
        assert_eq!(candidate.startup, Some(startup));
        assert!(candidate.startup_approved);
        if let Some((line, symbol, expected)) = package.hover {
            tokio::time::timeout(Duration::from_secs(120), async {
                loop {
                    let result = self
                        .read(code::Query::Hover {
                            path: package.path.into(),
                            line,
                            symbol: symbol.into(),
                        })
                        .await;
                    assert!(
                        matches!(result.status, code::Status::Ready | code::Status::Indexing),
                        "{result:?}"
                    );
                    let code::Content::Hover(text) = &result.content else {
                        panic!("hover expected")
                    };
                    if text.contains(symbol) && text.contains(expected) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            })
            .await
            .expect("real language server did not return the fixture signature");
        }
        tokio::time::timeout(Duration::from_secs(120), async {
            loop {
                let result = self.read(query.clone()).await;
                assert!(
                    matches!(result.status, code::Status::Ready | code::Status::Indexing),
                    "{result:?}"
                );
                let code::Content::Diagnostics(items) = &result.content else {
                    panic!("diagnostics expected")
                };
                if items.iter().any(|item| {
                    item.code
                        .as_deref()
                        .is_some_and(|code| package.diagnostics.contains(&code))
                        && item.location.path == package.path
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        })
        .await
        .expect("real language server did not diagnose the deliberate source error");
        assert!(
            !identities(&self.managed(package)).is_empty(),
            "real server process must be observable"
        );
        for (path, text) in package.files {
            assert_eq!(
                std::fs::read_to_string(self.root.join(path)).unwrap(),
                *text
            );
        }
    }

    async fn close(self, package: &Package) -> Vec<String> {
        let managed = self.managed(package);
        let running = identities(&managed);
        let mut errors = Vec::new();
        if managed.join("receipt.json").is_file() {
            match self
                .client
                .execute(self.client.prepare(Command::UninstallCodeServer {
                    package: package.name.into(),
                    version: self.version().into(),
                }))
                .await
            {
                Ok(Output::CodeUninstalled {
                    package: name,
                    version,
                }) if name == package.name && version == self.version() => {}
                result => errors.push(format!("real package uninstall failed: {result:?}")),
            }
            if managed.exists() {
                errors.push("uninstall retained the managed package".into());
            }
            match self
                .client
                .execute(self.client.prepare(Command::ListCodeServers {
                    worktree: self.worktree,
                }))
                .await
            {
                Ok(Output::CodeServers(servers)) if servers.is_empty() => {}
                result => errors.push(format!("uninstall retained a language server: {result:?}")),
            }
            if !exited(&running).await {
                errors.push("uninstall did not terminate the actual server processes".into());
            }
        }
        if let Err(error) = self.node.shutdown().await {
            errors.push(format!("Node shutdown failed: {error}"));
        }
        if let Err(error) = self.controller.close().await {
            errors.push(format!("controller shutdown failed: {error}"));
        }
        if !exited(&running).await || !identities(&managed).is_empty() {
            errors.push("Node shutdown retained an actual server process".into());
        }
        drop(self.directory);
        errors
    }
}

fn identities(directory: &Path) -> Vec<(Pid, u64)> {
    let system = System::new_all();
    let marker = directory.to_str().unwrap();
    let mut owned = system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            (process
                .exe()
                .is_some_and(|path| path.starts_with(directory))
                || process
                    .cmd()
                    .iter()
                    .any(|arg| arg.to_string_lossy().contains(marker)))
            .then_some(*pid)
        })
        .collect::<BTreeSet<_>>();
    loop {
        let descendants = system
            .processes()
            .iter()
            .filter_map(|(pid, process)| {
                process
                    .parent()
                    .is_some_and(|parent| owned.contains(&parent))
                    .then_some(*pid)
            })
            .collect::<BTreeSet<_>>();
        let previous = owned.len();
        owned.extend(descendants);
        if owned.len() == previous {
            break;
        }
    }
    owned
        .into_iter()
        .map(|pid| (pid, system.process(pid).unwrap().start_time()))
        .collect()
}

async fn exited(processes: &[(Pid, u64)]) -> bool {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let system = System::new_all();
            if processes.iter().all(|(pid, started)| {
                system
                    .process(*pid)
                    .is_none_or(|process| process.start_time() != *started)
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .is_ok()
}

async fn run(package: Package) {
    for remote in [false, true] {
        let fixture = Fixture::new(&package, remote).await;
        let outcome = AssertUnwindSafe(fixture.exercise(&package))
            .catch_unwind()
            .await;
        let cleanup = fixture.close(&package).await;
        if let Err(error) = outcome {
            if !cleanup.is_empty() {
                eprintln!("{} remote={remote} cleanup: {cleanup:?}", package.name);
            }
            std::panic::resume_unwind(error);
        }
        assert!(
            cleanup.is_empty(),
            "{} remote={remote}: {cleanup:?}",
            package.name
        );
    }
}

#[tokio::test]
#[ignore = "requires package network and existing toolchains"]
async fn installs_rust_analyzer() {
    run(RUST).await;
}

#[tokio::test]
#[ignore = "requires package network and existing toolchains"]
async fn installs_typescript_with_extra_package() {
    run(TYPESCRIPT).await;
}

#[tokio::test]
#[ignore = "requires package network and existing toolchains"]
async fn installs_ruff_and_reads_diagnostics() {
    run(PYTHON).await;
}
