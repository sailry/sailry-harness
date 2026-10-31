use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use git2::Repository;
use sailry_protocol::{ErrorCode, Fault};

use super::{fault, open};

pub(crate) struct WatchFilter {
    root: PathBuf,
    repository: Option<Repository>,
    metadata: Vec<PathBuf>,
    tracked: BTreeSet<PathBuf>,
}

impl WatchFilter {
    pub(super) fn new(root: &Path) -> Result<Self, Fault> {
        let mut filter = Self {
            root: root.to_path_buf(),
            repository: None,
            metadata: Vec::new(),
            tracked: BTreeSet::new(),
        };
        filter.refresh()?;
        Ok(filter)
    }

    pub(crate) fn changed(&mut self, paths: &[PathBuf]) -> bool {
        if paths.is_empty() {
            return true;
        }
        let metadata = paths.iter().any(|path| self.metadata_change(path));
        if metadata
            || paths.iter().any(|path| {
                path.starts_with(&self.root)
                    && path.file_name().is_some_and(|name| name == ".gitignore")
            })
        {
            // Reopening drops libgit2's cached ignore rules and tracked-path inventory.
            let _ = self.refresh();
            return true;
        }
        paths.iter().any(|path| self.worktree_change(path))
    }

    fn refresh(&mut self) -> Result<(), Fault> {
        self.repository = match open(&self.root) {
            Ok(repository) => Some(repository),
            Err(error) if error.code == ErrorCode::NotFound && !self.root.join(".git").exists() => {
                None
            }
            Err(error) => return Err(error),
        };
        self.metadata.clear();
        self.tracked.clear();
        if let Some(repository) = &self.repository {
            self.metadata.push(repository.path().to_path_buf());
            self.metadata.push(repository.commondir().to_path_buf());
            self.metadata.sort_unstable();
            self.metadata.dedup();
            let mut index = repository.index().map_err(fault)?;
            index.read(true).map_err(fault)?;
            for entry in index.iter() {
                if let Ok(path) = std::str::from_utf8(&entry.path) {
                    self.tracked.insert(PathBuf::from(path));
                }
            }
        }
        Ok(())
    }

    fn metadata_change(&self, path: &Path) -> bool {
        if path == self.root.join(".git") {
            return true;
        }
        self.metadata.iter().any(|root| {
            let Ok(relative) = path.strip_prefix(root) else {
                return false;
            };
            if relative.as_os_str().is_empty() {
                return true;
            }
            if relative
                .extension()
                .is_some_and(|extension| extension == "lock")
            {
                return false;
            }
            matches!(
                relative.to_str(),
                Some("HEAD" | "index" | "packed-refs" | "config" | "commondir" | "info/exclude")
            ) || relative.starts_with("refs")
        })
    }

    fn worktree_change(&self, path: &Path) -> bool {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return false;
        };
        if relative.as_os_str().is_empty() {
            return true;
        }
        if relative.starts_with(".sailry/worktrees")
            || relative.components().any(|part| part.as_os_str() == ".git")
        {
            return false;
        }
        if self.tracked.contains(relative)
            || self
                .tracked
                .range(relative.to_path_buf()..)
                .next()
                .is_some_and(|tracked| tracked.starts_with(relative))
        {
            return true;
        }
        self.repository
            .as_ref()
            .is_some_and(|repository| !repository.status_should_ignore(relative).unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_ignored_and_managed_changes() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let repository = Repository::init(&root).unwrap();
        std::fs::write(root.join(".gitignore"), "target/\nignored.txt\n").unwrap();
        std::fs::write(root.join("ignored.txt"), "tracked despite ignore").unwrap();
        let mut index = repository.index().unwrap();
        index.add_path(Path::new("ignored.txt")).unwrap();
        index.write().unwrap();
        let mut filter = WatchFilter::new(&root).unwrap();
        assert!(!filter.changed(&[root.join("target/debug/artifact")]));
        assert!(!filter.changed(&[root.join(".sailry/worktrees/child/source.rs")]));
        assert!(filter.changed(&[root.join("ignored.txt")]));
        assert!(filter.changed(&[root.join("source.rs")]));
        let outside = tempfile::tempdir().unwrap();
        assert!(!filter.changed(&[outside.path().canonicalize().unwrap().join("source.rs")]));
    }

    #[test]
    fn observes_metadata_without_object_writes() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        Repository::init(&root).unwrap();
        let mut filter = WatchFilter::new(&root).unwrap();
        assert!(filter.changed(&[root.join(".git/index")]));
        assert!(filter.changed(&[root.join(".git/HEAD")]));
        assert!(filter.changed(&[root.join(".git/refs/heads/main")]));
        assert!(!filter.changed(&[root.join(".git/index.lock")]));
        assert!(!filter.changed(&[root.join(".git/objects/ab/object")]));
        assert!(!filter.changed(&[root.join(".git/logs/HEAD")]));
    }

    #[test]
    fn reloads_ignore_rules() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        Repository::init(&root).unwrap();
        std::fs::write(root.join(".gitignore"), "first/\n").unwrap();
        let mut filter = WatchFilter::new(&root).unwrap();
        assert!(!filter.changed(&[root.join("first/output")]));
        std::fs::write(root.join(".gitignore"), "second/\n").unwrap();
        assert!(filter.changed(&[root.join(".gitignore")]));
        assert!(filter.changed(&[root.join("first/output")]));
        assert!(!filter.changed(&[root.join("second/output")]));
    }

    #[test]
    fn observes_linked_metadata_roots() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let repository = Repository::init(root.join("main")).unwrap();
        let tree = repository
            .find_tree(repository.index().unwrap().write_tree().unwrap())
            .unwrap();
        let signature = git2::Signature::now("Fixture", "fixture@example.test").unwrap();
        repository
            .commit(Some("HEAD"), &signature, &signature, "Initial", &tree, &[])
            .unwrap();
        let linked = root.join("linked");
        repository.worktree("linked", &linked, None).unwrap();
        let linked_repository = open(&linked).unwrap();
        let metadata = linked_repository.path().to_path_buf();
        let common = linked_repository.commondir().to_path_buf();
        let roots = super::super::Git::watch_roots(&linked).unwrap();
        assert!(roots.contains(&metadata));
        assert!(roots.contains(&common));
        assert_eq!(roots.len(), 2);
        let mut filter = WatchFilter::new(&linked).unwrap();
        assert!(filter.changed(&[metadata.join("HEAD")]));
        assert!(filter.changed(&[metadata.join("index")]));
        assert!(filter.changed(&[common.join("refs/heads/main")]));
        assert!(filter.changed(&[common.join("info/exclude")]));
        assert!(!filter.changed(&[common.join("objects/ab/object")]));
        assert!(!filter.changed(&[common.join("logs/HEAD")]));
    }

    #[test]
    fn detects_repository_creation() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        assert!(super::super::Git::watch_roots(&root).unwrap().is_empty());
        let mut filter = WatchFilter::new(&root).unwrap();
        Repository::init(&root).unwrap();
        assert!(filter.changed(&[root.join(".git")]));
        assert!(filter.changed(&[root.join("new.txt")]));
        assert_eq!(super::super::Git::watch_roots(&root).unwrap().len(), 1);
    }
}
