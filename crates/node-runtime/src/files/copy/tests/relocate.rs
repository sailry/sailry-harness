use super::*;
use std::{cell::Cell, path::PathBuf};

struct Fixture {
    _temp: tempfile::TempDir,
    source: PathBuf,
    target: PathBuf,
    recovered: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let source = root.join("source");
        let target = root.join("target");
        fs::create_dir_all(source.join("folder/sub/empty")).unwrap();
        fs::create_dir_all(source.join("folder/.git")).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(source.join("folder/sub/文件"), "original").unwrap();
        fs::write(source.join("folder/.git/config"), "metadata").unwrap();
        fs::write(source.join("folder/sub/.git"), "gitdir: ../.git").unwrap();
        Self {
            _temp: temp,
            source,
            target,
            recovered: root.join("recovered"),
        }
    }

    fn recycle(&self, root: &Path, name: &str) -> Result<(), Fault> {
        assert_eq!(root, self.source);
        assert_eq!(name, "folder");
        fs::rename(root.join(name), &self.recovered).map_err(io_error)
    }

    fn run(&self, hook: &dyn Fn(Phase)) -> Result<(), Fault> {
        publish(
            &self.source,
            &self.target,
            "folder",
            "folder",
            Some(&|root, name| self.recycle(root, name)),
            hook,
        )
    }
}

#[test]
fn preserves_copied_metadata() {
    let fixture = Fixture::new();
    let bytes = vec![0xff; 10 * 1024 * 1024];
    fs::write(fixture.source.join("folder/binary"), &bytes).unwrap();
    fs::write(fixture.source.join("folder/empty"), []).unwrap();
    let paths = ["folder", "folder/sub", "folder/sub/文件", "folder/binary"];
    for name in paths {
        path::root(&fixture.source)
            .unwrap()
            .set_mtime(
                name,
                cap_std::time::SystemTime::from_std(
                    std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
                )
                .into(),
            )
            .unwrap();
    }
    let before: Vec<_> = paths
        .iter()
        .map(|name| fs::metadata(fixture.source.join(name)).unwrap())
        .collect();
    fixture.run(&|_| {}).unwrap();
    assert!(!fixture.source.join("folder").exists());
    assert!(fixture.recovered.join("sub/文件").is_file());
    assert_eq!(
        fs::read(fixture.target.join("folder/binary")).unwrap(),
        bytes
    );
    assert!(
        fs::read(fixture.target.join("folder/empty"))
            .unwrap()
            .is_empty()
    );
    assert!(fixture.target.join("folder/sub/empty").is_dir());
    for (name, metadata) in paths.into_iter().zip(before) {
        let copied = fs::metadata(fixture.target.join(name)).unwrap();
        assert_eq!(metadata.permissions(), copied.permissions());
        assert_eq!(metadata.modified().unwrap(), copied.modified().unwrap());
    }
    assert_eq!(
        fs::read_to_string(fixture.target.join("folder/.git/config")).unwrap(),
        "metadata"
    );
    assert_eq!(
        fs::read_to_string(fixture.target.join("folder/sub/.git")).unwrap(),
        "gitdir: ../.git"
    );
    assert_eq!(fs::read_dir(&fixture.target).unwrap().count(), 1);
}

#[test]
fn detects_unstamped_descendant_edits() {
    for published in [false, true] {
        for target in [false, true] {
            if target && !published {
                continue;
            }
            let fixture = Fixture::new();
            let result = fixture.run(&|phase| {
                if matches!(phase, Phase::Published) != published
                    || matches!(phase, Phase::Recycled)
                {
                    return;
                }
                let root = if target {
                    &fixture.target
                } else {
                    &fixture.source
                };
                let relative = "folder/sub/文件";
                let file = root.join(relative);
                let modified = fs::metadata(&file).unwrap().modified().unwrap();
                fs::write(&file, "modified").unwrap();
                path::root(root)
                    .unwrap()
                    .set_mtime(
                        relative,
                        cap_std::time::SystemTime::from_std(modified).into(),
                    )
                    .unwrap();
            });
            assert_eq!(
                result.unwrap_err().code,
                if published {
                    ErrorCode::OutcomeUnknown
                } else {
                    ErrorCode::RevisionConflict
                }
            );
            assert!(fixture.source.join("folder/sub/文件").exists());
            assert!(!fixture.recovered.exists());
            assert_eq!(fixture.target.join("folder").exists(), published);
        }
    }
}

#[test]
fn detects_descendant_changes() {
    for published in [false, true] {
        for change in 0..4 {
            let fixture = Fixture::new();
            let result = fixture.run(&|phase| {
                if matches!(phase, Phase::Published) != published
                    || matches!(phase, Phase::Recycled)
                {
                    return;
                }
                let root = fixture.source.join("folder");
                match change {
                    0 => {
                        fs::rename(root.join("sub/文件"), root.join("sub/retained")).unwrap();
                        fs::write(root.join("sub/文件"), "original").unwrap();
                    }
                    1 => fs::write(root.join("sub/added"), "new").unwrap(),
                    2 => {
                        fs::rename(root.join("sub/文件"), fixture.source.join("retained")).unwrap()
                    }
                    _ => fs::write(root.join(".git/config"), "changed metadata").unwrap(),
                }
            });
            assert_eq!(
                result.unwrap_err().code,
                if published {
                    ErrorCode::OutcomeUnknown
                } else {
                    ErrorCode::RevisionConflict
                }
            );
            assert!(fixture.source.join("folder").exists());
            assert!(!fixture.recovered.exists());
            assert_eq!(
                fs::read_dir(&fixture.target).unwrap().count(),
                usize::from(published)
            );
        }
    }
}

#[test]
fn detects_same_content_replacement() {
    let fixture = Fixture::new();
    let result = fixture.run(&|phase| {
        if matches!(phase, Phase::Published) {
            let parent = fixture.source.join("folder/sub");
            let file = parent.join("文件");
            let file_time = fs::metadata(&file).unwrap().modified().unwrap();
            let parent_time = fs::metadata(&parent).unwrap().modified().unwrap();
            fs::rename(&file, fixture.source.join("retained-file")).unwrap();
            fs::write(&file, "original").unwrap();
            let root = path::root(&fixture.source).unwrap();
            root.set_mtime(
                "folder/sub/文件",
                cap_std::time::SystemTime::from_std(file_time).into(),
            )
            .unwrap();
            root.set_mtime(
                "folder/sub",
                cap_std::time::SystemTime::from_std(parent_time).into(),
            )
            .unwrap();
        }
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::OutcomeUnknown);
    assert!(!fixture.recovered.exists());
    assert_eq!(
        fs::read_to_string(fixture.source.join("folder/sub/文件")).unwrap(),
        "original"
    );
    assert_eq!(
        fs::read_to_string(fixture.target.join("folder/sub/文件")).unwrap(),
        "original"
    );
}

#[test]
fn preserves_copies_on_trash_failure() {
    let fixture = Fixture::new();
    let called = Cell::new(false);
    let result = publish(
        &fixture.source,
        &fixture.target,
        "folder",
        "folder",
        Some(&|_, _| {
            called.set(true);
            Err(Fault::new(
                ErrorCode::PermissionDenied,
                "injected Trash failure",
            ))
        }),
        &|_| {},
    );
    assert!(called.get());
    assert_eq!(result.unwrap_err().code, ErrorCode::OutcomeUnknown);
    for root in [&fixture.source, &fixture.target] {
        assert_eq!(
            fs::read_to_string(root.join("folder/sub/文件")).unwrap(),
            "original"
        );
        assert_eq!(fs::read_dir(root).unwrap().count(), 1);
    }
}

#[test]
fn rechecks_parents_before_trash() {
    for source in [false, true] {
        let fixture = Fixture::new();
        let root = if source {
            &fixture.source
        } else {
            &fixture.target
        };
        let retained = fixture._temp.path().join("retained");
        let result = fixture.run(&|phase| {
            if matches!(phase, Phase::Published) {
                fs::rename(root, &retained).unwrap();
                fs::create_dir_all(root.join("folder")).unwrap();
                fs::write(root.join("folder/sentinel"), "replacement").unwrap();
            }
        });
        assert_eq!(result.unwrap_err().code, ErrorCode::OutcomeUnknown);
        assert!(!fixture.recovered.exists());
        assert_eq!(
            fs::read_to_string(root.join("folder/sentinel")).unwrap(),
            "replacement"
        );
        assert_eq!(
            fs::read_to_string(retained.join("folder/sub/文件")).unwrap(),
            "original"
        );
    }
}

#[test]
fn reports_changes_after_trash() {
    let fixture = Fixture::new();
    let result = fixture.run(&|phase| {
        if matches!(phase, Phase::Recycled) {
            fs::write(fixture.target.join("folder/sub/文件"), "external edit").unwrap();
        }
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::OutcomeUnknown);
    assert!(!fixture.source.join("folder").exists());
    assert_eq!(
        fs::read_to_string(fixture.recovered.join("sub/文件")).unwrap(),
        "original"
    );
    assert_eq!(
        fs::read_to_string(fixture.target.join("folder/sub/文件")).unwrap(),
        "external edit"
    );
}

#[cfg(unix)]
#[test]
fn rejects_unsupported_entries() {
    let fixture = Fixture::new();
    std::os::unix::fs::symlink("../../outside", fixture.source.join("folder/link")).unwrap();
    assert!(fixture.run(&|_| {}).is_err());
    assert!(fixture.source.join("folder").exists());
    assert!(!fixture.target.join("folder").exists());
    assert!(!fixture.recovered.exists());
}
