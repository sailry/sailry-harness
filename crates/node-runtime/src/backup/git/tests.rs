use super::*;
use git2::{Signature, Status, WorktreeAddOptions};
use sailry_protocol::{Command, GitChangeKind, Output, RepositoryKind, WorktreeId};
use std::{fs, path::PathBuf};

struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    profile: PathBuf,
    main: PathBuf,
    worktree: PathBuf,
    relative: String,
    head: Oid,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let profile = root.join("profile");
        crate::profile::private_directory(&profile).unwrap();
        let main = root.join("main");
        let repository = Repository::init_opts(
            &main,
            RepositoryInitOptions::new()
                .external_template(false)
                .initial_head("main"),
        )
        .unwrap();
        repository.set_config(&Config::new().unwrap()).unwrap();
        fs::write(main.join("source.txt"), b"base\n").unwrap();
        let mut index = repository.index().unwrap();
        index.add_path(Path::new("source.txt")).unwrap();
        index.write().unwrap();
        let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = Signature::now("Fixture", "fixture@example.invalid").unwrap();
        let head = repository
            .commit(Some("HEAD"), &signature, &signature, "Initial", &tree, &[])
            .unwrap();
        let commit = repository.find_commit(head).unwrap();
        repository
            .tag(
                "retained",
                commit.as_object(),
                &signature,
                "Retained tag",
                false,
            )
            .unwrap();
        let branch = repository.branch("owned", &commit, false).unwrap();
        let reference = branch.into_reference();
        let relative = format!("worktrees/{}", WorktreeId::new());
        let worktree = profile.join(&relative);
        fs::create_dir_all(worktree.parent().unwrap()).unwrap();
        repository
            .worktree(
                "owned",
                &worktree,
                Some(WorktreeAddOptions::new().reference(Some(&reference))),
            )
            .unwrap();
        let mut local = Config::open(&repository.path().join("config")).unwrap();
        local.set_bool("extensions.worktreeConfig", true).unwrap();
        local
            .set_str("core.worktree", main.to_str().unwrap())
            .unwrap();
        let mut output = fs::OpenOptions::new()
            .append(true)
            .open(repository.path().join("config"))
            .unwrap();
        output
            .write_all(
                b"\n[custom]\n\tmultiple = first\n\tmultiple = \n\tmultiple = last\n[include]\n\tpath = included.conf\n",
            )
            .unwrap();
        fs::write(
            repository.path().join("included.conf"),
            b"[custom]\n\tincluded = captured\n",
        )
        .unwrap();
        fs::write(repository.path().join("info/exclude"), b"ignored.txt\n").unwrap();
        let checkout = Repository::open(&worktree).unwrap();
        fs::write(
            checkout.path().join("config.worktree"),
            b"[custom]\n\tcheckout = retained\n[core]\n\tworktree = /old/checkout\n",
        )
        .unwrap();
        fs::write(worktree.join("source.txt"), b"staged\n").unwrap();
        let mut index = checkout.index().unwrap();
        index.add_path(Path::new("source.txt")).unwrap();
        index.write().unwrap();
        fs::write(worktree.join("source.txt"), b"unstaged\n").unwrap();
        fs::write(worktree.join("untracked.txt"), b"untracked\n").unwrap();
        fs::write(worktree.join("ignored.txt"), b"ignored\n").unwrap();
        Self {
            _directory: directory,
            root,
            profile,
            main,
            worktree,
            relative,
            head,
        }
    }

    fn snapshot(&self) -> super::super::archive::Snapshot {
        let identity = sailry_link::Identity::open(&self.profile).unwrap();
        let mut snapshot =
            super::super::archive::Snapshot::new(&self.profile, identity.id()).unwrap();
        let database = snapshot.database_path().unwrap();
        fs::write(&database, b"archive database fixture").unwrap();
        fs::write(
            &crate::profile::database_files(&database)[1],
            b"archive logical log fixture",
        )
        .unwrap();
        snapshot.database(&CancellationToken::new()).unwrap();
        snapshot
            .capture(&["link.key".into()], &CancellationToken::new())
            .unwrap();
        snapshot
    }
}

fn inventory(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut result = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in directory.read_dir().unwrap() {
            let entry = entry.unwrap();
            assert!(!entry.file_type().unwrap().is_symlink());
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                pending.push(path);
            } else {
                result.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    result
}

#[tokio::test]
async fn restores_independent_linked_checkouts() {
    let fixture = Fixture::new();
    let original_main = inventory(&fixture.main);
    let original_checkout = inventory(&fixture.worktree);
    let mut snapshot = fixture.snapshot();
    snapshot
        .capture(
            std::slice::from_ref(&fixture.relative),
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(inventory(&fixture.main), original_main);
    assert_eq!(inventory(&fixture.worktree), original_checkout);
    let packed = snapshot.pack(&CancellationToken::new()).unwrap();
    let target = fixture.root.join("restored");
    crate::profile::private_directory(&target).unwrap();
    super::super::archive::unpack(
        packed.file.reopen().unwrap(),
        &target,
        &CancellationToken::new(),
    )
    .unwrap();
    let restored = target.join(&fixture.relative);
    assert!(restored.join(".git").is_dir());
    assert!(!restored.join(".git/commondir").exists());
    assert!(!restored.join(".git/gitdir").exists());
    assert!(!restored.join(".git/objects/info/alternates").exists());
    fs::rename(&fixture.profile, fixture.root.join("unavailable")).unwrap();
    let repository = Repository::open(&restored).unwrap();
    assert!(!repository.is_worktree());
    assert_eq!(
        repository.workdir().unwrap().canonicalize().unwrap(),
        restored
    );
    assert_eq!(repository.head().unwrap().target(), Some(fixture.head));
    assert_eq!(repository.head().unwrap().shorthand().unwrap(), "owned");
    assert_eq!(
        repository
            .find_reference("refs/heads/main")
            .unwrap()
            .target(),
        Some(fixture.head)
    );
    assert_eq!(
        repository
            .find_reference("refs/tags/retained")
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .id(),
        fixture.head
    );
    let index = repository.index().unwrap();
    let entry = index.get_path(Path::new("source.txt"), 0).unwrap();
    assert_eq!(
        repository.find_blob(entry.id).unwrap().content(),
        b"staged\n"
    );
    assert_eq!(
        fs::read(restored.join("source.txt")).unwrap(),
        b"unstaged\n"
    );
    let status = repository.status_file(Path::new("source.txt")).unwrap();
    assert!(status.contains(Status::INDEX_MODIFIED));
    assert!(status.contains(Status::WT_MODIFIED));
    assert_eq!(
        repository.status_file(Path::new("ignored.txt")).unwrap(),
        Status::IGNORED
    );
    let configuration = Config::open(&restored.join(".git/config")).unwrap();
    assert_eq!(configuration.get_string("core.worktree").unwrap(), "..");
    assert_eq!(
        configuration.get_string("custom.included").unwrap(),
        "captured"
    );
    assert_eq!(
        configuration.get_string("custom.checkout").unwrap(),
        "retained"
    );
    assert!(configuration.get_string("include.path").is_err());
    assert!(configuration.get_bool("extensions.worktreeConfig").is_err());
    let mut entries = configuration.multivar("custom.multiple", None).unwrap();
    let mut values = vec![];
    while let Some(entry) = entries.next() {
        values.push(entry.unwrap().value().unwrap().to_owned());
    }
    assert_eq!(values, ["first", "", "last"]);
    let Output::GitStatus(status) =
        crate::git::Git::new(std::sync::Arc::new(crate::process::Environment::ambient()))
            .inspect(
                restored,
                Command::InspectGit {
                    worktree: WorktreeId::new(),
                },
                CancellationToken::new(),
            )
            .await
            .unwrap()
    else {
        panic!("Git status expected")
    };
    assert_eq!(status.kind, RepositoryKind::Ready);
    let source = status
        .entries
        .iter()
        .find(|entry| entry.path == "source.txt")
        .unwrap();
    assert_eq!(source.staged, Some(GitChangeKind::Modified));
    assert_eq!(source.unstaged, Some(GitChangeKind::Modified));
    assert!(
        status
            .entries
            .iter()
            .any(|entry| entry.path == "untracked.txt" && entry.untracked)
    );
    assert_eq!(inventory(&fixture.main), original_main);
}

#[test]
fn materializes_alternate_objects_without_links() {
    let fixture = Fixture::new();
    let alternate = fixture.root.join("alternate");
    let repository = Repository::init_opts(
        &alternate,
        RepositoryInitOptions::new()
            .bare(true)
            .external_template(false),
    )
    .unwrap();
    let blob = repository.blob(b"alternate-only object").unwrap();
    let main = Repository::open(&fixture.main).unwrap();
    fs::write(
        main.path().join("objects/info/alternates"),
        format!("{}\n", repository.path().join("objects").display()),
    )
    .unwrap();
    drop(main);
    let main = Repository::open(&fixture.main).unwrap();
    main.reference("refs/retained/alternate", blob, false, "Fixture")
        .unwrap();
    let before = inventory(&fixture.main);
    let temporary = crate::profile::temporary(&fixture.profile).unwrap();
    let prepared = prepare(&fixture.worktree, &temporary, &CancellationToken::new())
        .unwrap()
        .unwrap();
    assert_eq!(inventory(&fixture.main), before);
    assert!(!prepared.path().join("objects/info/alternates").exists());
    assert!(!prepared.path().join("commondir").exists());
    fs::rename(&alternate, fixture.root.join("unavailable-alternate")).unwrap();
    let output = fixture.root.join("materialized");
    fs::create_dir(&output).unwrap();
    let metadata = output.join(".git");
    fs::rename(prepared.path(), &metadata).unwrap();
    let repository = Repository::open(&output).unwrap();
    assert_eq!(
        repository.find_blob(blob).unwrap().content(),
        b"alternate-only object"
    );
    assert_eq!(
        repository
            .find_reference("refs/retained/alternate")
            .unwrap()
            .target(),
        Some(blob)
    );
    assert_eq!(inventory(&fixture.main), before);
}

#[test]
fn leaves_sources_unchanged_when_cancelled() {
    let fixture = Fixture::new();
    let before = inventory(&fixture.main);
    let temporary = crate::profile::temporary(&fixture.profile).unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        prepare(&fixture.worktree, &temporary, &cancel)
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    assert_eq!(inventory(&fixture.main), before);
    assert!(temporary.read_dir().unwrap().next().is_none());
}
