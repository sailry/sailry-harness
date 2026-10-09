use super::*;

fn rules(root: &Path, selected: &str) -> Rules {
    let mut scan = source::worktree(root, selected, CancellationToken::new()).unwrap();
    let mut rules = Rules::new();
    while let Some(entry) = scan.next_watched(&mut |_, _| Ok(false), &mut |watches| {
        for watch in watches {
            rules
                .entry(root.join(&watch.relative))
                .or_default()
                .push(watch);
        }
    }) {
        entry.unwrap();
    }
    assert!(scan.complete());
    rules
}

fn event(root: &Path, path: &str, kind: EventKind) -> notify::Event {
    notify::Event::new(kind).add_path(root.join(path))
}

#[test]
fn excludes_build_metadata_and_nested_ignored_events() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    for path in ["target", ".git", "notes"] {
        std::fs::create_dir(root.path().join(path)).unwrap();
    }
    std::fs::write(
        root.path().join(".gitignore"),
        "target/\n*.tmp\nnotes/*.log\n",
    )
    .unwrap();
    std::fs::write(root.path().join("notes/.gitignore"), "!kept.log\n").unwrap();
    std::fs::write(root.path().join("notes/source.md"), "source").unwrap();
    let rules = rules(root.path(), "");
    assert!(!rules.contains_key(&root.path().join("target")));
    assert!(!rules.contains_key(&root.path().join(".git")));
    let modified = EventKind::Modify(notify::event::ModifyKind::Any);
    for path in [
        "target/object.o",
        ".git/index",
        "scratch.tmp",
        "notes/ignored.log",
    ] {
        assert!(
            !relevant(&rules, &event(root.path(), path, modified)),
            "unexpected dirty event for {path}"
        );
    }
    for path in [
        "notes/source.md",
        "notes/kept.log",
        ".gitignore",
        "notes/.gitignore",
    ] {
        assert!(
            relevant(&rules, &event(root.path(), path, modified)),
            "lost dirty event for {path}"
        );
    }
    assert!(relevant(
        &rules,
        &event(
            root.path(),
            "notes",
            EventKind::Remove(notify::event::RemoveKind::Folder)
        )
    ));
    assert!(!relevant(
        &rules,
        &event(
            root.path(),
            "target",
            EventKind::Create(notify::event::CreateKind::Folder)
        )
    ));
    assert!(!relevant(
        &rules,
        &event(
            root.path(),
            "notes/source.md",
            EventKind::Access(notify::event::AccessKind::Any)
        )
    ));
}

#[test]
fn filters_explicit_files_and_inherited_ignore_changes() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    std::fs::create_dir(root.path().join("notes")).unwrap();
    let path = root.path().join("notes/source.md");
    std::fs::write(&path, "source").unwrap();
    let scoped = rules(root.path(), "notes/source.md");
    let modified = EventKind::Modify(notify::event::ModifyKind::Any);
    assert!(relevant(
        &scoped,
        &event(root.path(), ".gitignore", modified)
    ));
    assert!(relevant(
        &scoped,
        &event(root.path(), "notes/.gitignore", modified)
    ));
    assert!(!relevant(
        &scoped,
        &event(root.path(), "other.md", modified)
    ));
    let mut scan = source::controller(
        &Source::File {
            path: path.to_str().unwrap().into(),
        },
        CancellationToken::new(),
    )
    .unwrap();
    let parent = path.parent().unwrap();
    let explicit = scan
        .take_watches()
        .into_iter()
        .map(|watch| (parent.to_owned(), vec![watch]))
        .collect();
    assert!(relevant(
        &explicit,
        &event(root.path(), "notes/source.md", modified)
    ));
    assert!(!relevant(
        &explicit,
        &event(root.path(), "notes/.gitignore", modified)
    ));
    assert!(!relevant(
        &explicit,
        &event(root.path(), "notes/other.md", modified)
    ));
}
