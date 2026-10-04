use super::*;
use std::path::{Path, PathBuf};

pub(super) struct Fixture {
    pub base: super::super::Fixture,
    pub linked: PathBuf,
    pub head: String,
    remote: bool,
}

impl Fixture {
    pub fn new(remote: bool) -> Self {
        let base = super::super::Fixture::new(remote);
        let root = base.directory.path().join("project");
        let repository = git2::Repository::open(&root).unwrap();
        repository.set_head("refs/heads/main").unwrap();
        std::fs::write(root.join("notes.txt"), "committed\n").unwrap();
        let mut index = repository.index().unwrap();
        index.add_path(Path::new("notes.txt")).unwrap();
        index.write().unwrap();
        let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
        let head = repository
            .commit(Some("HEAD"), &signature, &signature, "Fixture", &tree, &[])
            .unwrap();
        repository
            .tag_lightweight(
                "release",
                &repository.find_object(head, None).unwrap(),
                false,
            )
            .unwrap();
        let linked = base.directory.path().join("linked");
        repository.worktree("feature", &linked, None).unwrap();
        let package = root.join("worktrees-package");
        crate::plugins::fixture::copy_package(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/worktrees"),
            &package,
        );
        let view = package.join("dev.sailry.platform/desktop/view.js");
        let source = std::fs::read_to_string(&view).unwrap()
            .replace("return div().id(id).child(control);", "return Anchor.new(id).child(control);")
            .replace("div().id(id).child(TextField", "Anchor.new(id).child(TextField")
            .replace("div().id(owner.pending || !owner.status ? 'location-branch-loading' : 'location-branch-name')", "Anchor.new(owner.pending || !owner.status ? 'location-branch-loading' : 'location-branch-name')")
            .replace("div().id('location-copy-changes').child", "Anchor.new('location-copy-changes').child");
        std::fs::write(
            view,
            format!("import {{Anchor}} from 'sailry/test';\n{source}"),
        )
        .unwrap();
        let Output::Plugin(previous) = base.execute(Command::ReadPlugin {
            name: "worktrees".into(),
        }) else {
            panic!("Worktrees package expected")
        };
        let Output::Plugin(package) = base.execute(Command::InstallPlugin {
            worktree: base.session.worktree,
            path: "worktrees-package".into(),
            name: "worktrees".into(),
            expected_revision: previous.summary.revision,
        }) else {
            panic!("Worktrees package expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        assert!(package.summary.enabled);
        Self {
            base,
            linked,
            head: head.to_string(),
            remote,
        }
    }

    pub fn root(&self) -> PathBuf {
        self.base.directory.path().join("project")
    }
    pub fn current_head(&self) -> String {
        git2::Repository::open(self.root())
            .unwrap()
            .head()
            .unwrap()
            .target()
            .unwrap()
            .to_string()
    }
    pub fn advance(&self, path: &Path) {
        let repo = git2::Repository::open(path).unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "Next",
            &parent.tree().unwrap(),
            &[&parent],
        )
        .unwrap();
    }
    pub fn move_tag(&self) {
        self.advance(&self.root());
        let repo = git2::Repository::open(self.root()).unwrap();
        repo.tag_lightweight(
            "release",
            &repo.head().unwrap().peel_to_commit().unwrap().into_object(),
            true,
        )
        .unwrap();
    }
    pub fn mutations(&self) -> Vec<Request> {
        self.base
            .transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| {
                matches!(
                    request.command,
                    Command::CreateWorktree { .. }
                        | Command::CreateManagedWorktree { .. }
                        | Command::RegisterWorktree { .. }
                        | Command::RemoveWorktree { .. }
                )
            })
            .cloned()
            .collect()
    }
    pub fn creates(&self) -> Vec<Request> {
        self.mutations()
            .into_iter()
            .filter(|request| matches!(request.command, Command::CreateWorktree { .. }))
            .collect()
    }
    pub fn removals(&self) -> Vec<Request> {
        self.mutations()
            .into_iter()
            .filter(|request| matches!(request.command, Command::RemoveWorktree { .. }))
            .collect()
    }
    pub fn selected(&self, shell: &Entity<Shell>, cx: &App) -> Option<WorktreeId> {
        shell
            .read(cx)
            .live
            .as_ref()?
            .selected_worktree()
            .map(|tree| tree.id)
    }
    pub fn close(self) {
        self.base.close();
    }

    pub fn dirty(
        &self,
        shell: &Entity<Shell>,
        visual: &mut VisualTestContext,
    ) -> Entity<crate::plugins::documents::Controller> {
        let Output::Worktree(_) = self.base.execute(Command::RegisterWorktree {
            project: self.base.binding.project.unwrap(),
            path: self.linked.to_str().unwrap().into(),
        }) else {
            panic!("registered linked worktree expected")
        };
        let Output::Snapshot(snapshot) = self.base.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let linked = std::fs::canonicalize(&self.linked).unwrap();
        let tree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.path == linked.to_str().unwrap())
            .unwrap()
            .id;
        let Output::Plugin(files) = self.base.execute(Command::ReadPlugin {
            name: "files".into(),
        }) else {
            panic!("Files package expected")
        };
        let mut binding = self.base.binding.clone();
        binding.worktree = Some(tree);
        let controller = visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.documents.get(binding, None, window, cx).unwrap()
            })
        });
        let receive = visual.update(|_, cx| {
            controller.update(cx, |controller, cx| {
                controller.request_open(
                    "notes.txt".into(),
                    None,
                    sailry_protocol::plugin::Context {
                        package: files.summary.reference(),
                        worktree: Some(tree),
                        session: None,
                        turn: None,
                        invocation: None,
                        surface: Default::default(),
                    },
                    sailry_link::CancellationToken::new(),
                    cx,
                )
            })
        });
        wait(visual, |cx| {
            controller.read(cx).editor("notes.txt").is_some()
        });
        visual.update(|window, cx| {
            let editor = controller.read(cx).editor("notes.txt").unwrap();
            editor.update(cx, |editor, cx| {
                editor.set_value("Unsaved checkout draft", window, cx)
            });
        });
        wait(visual, |cx| controller.read(cx).has_unsaved(cx));
        drop(receive);
        controller
    }

    pub fn mount<'a>(
        &self,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Shell>, Entity<Registry>, &'a mut VisualTestContext) {
        let (shell, visual) = super::super::files::mount(&self.base, self.remote, cx);
        let registry = visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select_project_worktree(
                    self.base.binding.project.unwrap(),
                    self.base.session.worktree,
                );
                shell.page = Page::Project;
                let registry = shell
                    .project_plugins(self.base.binding.project.unwrap(), window, cx)
                    .unwrap();
                cx.notify();
                registry
            })
        });
        wait(visual, |cx| {
            registry.read(cx).intent(Intent::Worktrees, cx).is_some()
        });
        (shell, registry, visual)
    }
}

pub(super) fn invoke(
    shell: &Entity<Shell>,
    registry: &mut Entity<Registry>,
    intent: Intent,
    value: serde_json::Value,
    visual: &mut VisualTestContext,
) {
    *registry = visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell
                .project_plugins(shell.live.as_ref().unwrap().project.unwrap(), window, cx)
                .unwrap()
        })
    });
    wait(visual, |cx| registry.read(cx).intent(intent, cx).is_some());
    visual.update(|_, cx| {
        registry.update(cx, |registry, cx| {
            registry.invoke_intent(intent, value, cx).unwrap()
        })
    });
    visual.run_until_parked();
}

pub(super) fn ready(registry: &Entity<Registry>, visual: &mut VisualTestContext, selector: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let tree = visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            registry
                .read(cx)
                .test_panel("worktrees")
                .map(|panel| diagnostics(&panel, cx))
                .unwrap_or_default()
        });
        let native = visual.debug_bounds(selector).is_some();
        let control = selector.starts_with("entry-")
            || selector.starts_with("action-")
            || selector.starts_with("location-");
        if native || !control && tree.contains(selector) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Worktrees selector deadline: {selector}; {tree}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn input(visual: &mut VisualTestContext, selector: &'static str, text: &str) {
    click(visual, selector);
    visual.simulate_keystrokes("secondary-a");
    visual.simulate_input(text);
    visual.run_until_parked();
}
