use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use cap_std::fs::Metadata;
use git2::{IndexEntry, ObjectType, Oid, Repository};
use sailry_protocol::*;
use serde::Serialize;

use super::{hash, stale};
use crate::git::{Control, fault, index, io_error, path, valid_path};

const MAX_SCAN_ENTRIES: usize = 100_000;
const CACHE_IDLE: Duration = Duration::from_secs(5);
const CACHE_ENTRIES: usize = 8;

#[derive(Default)]
pub(crate) struct Cache {
    entries: Mutex<VecDeque<Cached>>,
    #[cfg(test)]
    pub(super) scans: std::sync::atomic::AtomicUsize,
}

struct Cached {
    root: PathBuf,
    capture: Arc<Capture>,
    expires: Instant,
}

impl Cache {
    fn get(&self, root: &Path, snapshot: &GitChangeSnapshot) -> Option<Arc<Capture>> {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let now = Instant::now();
        entries.retain(|entry| entry.expires > now);
        let position = entries
            .iter()
            .position(|entry| entry.root == root && entry.capture.snapshot == *snapshot)?;
        let mut entry = entries.remove(position)?;
        entry.expires = now + CACHE_IDLE;
        let capture = entry.capture.clone();
        entries.push_back(entry);
        Some(capture)
    }

    fn save(&self, root: &Path, capture: Capture) -> Arc<Capture> {
        #[cfg(test)]
        self.scans
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let capture = Arc::new(capture);
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        entries.retain(|entry| {
            entry.expires > Instant::now()
                && (entry.root != root || entry.capture.snapshot != capture.snapshot)
        });
        while entries.len() >= CACHE_ENTRIES {
            entries.pop_front();
        }
        entries.push_back(Cached {
            root: root.to_owned(),
            capture: capture.clone(),
            expires: Instant::now() + CACHE_IDLE,
        });
        capture
    }
}

pub(super) struct Capture {
    pub snapshot: GitChangeSnapshot,
    pub entries: BTreeMap<String, Candidate>,
    pub omitted: usize,
}

pub(super) struct Candidate {
    pub entry: GitChangeEntry,
    pub old: Option<Resource>,
    pub new: Option<Resource>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) enum Resource {
    Blob { oid: String, mode: u32 },
    Working(Stamp),
}

impl Resource {
    pub(super) fn mode(&self) -> u32 {
        match self {
            Self::Blob { mode, .. } => *mode,
            Self::Working(stamp) => stamp.mode,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct Stamp {
    pub mode: u32,
    pub size: u64,
    modified: Option<(u64, u32)>,
    // Discovery checks ctime without changing a captured file's metadata identity.
    #[cfg(unix)]
    #[serde(skip)]
    changed: (i64, i64),
    #[cfg(unix)]
    #[serde(skip)]
    special_device: u64,
    #[cfg(windows)]
    #[serde(skip)]
    created: Option<(u64, u32)>,
    #[cfg(unix)]
    identity: (u64, u64),
}

impl Stamp {
    pub(super) fn of(metadata: &Metadata) -> Self {
        let mode = if metadata.is_symlink() {
            0o120000
        } else if metadata.is_file() {
            file_mode(metadata)
        } else if metadata.is_dir() {
            0o160000
        } else {
            0
        };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| {
                time.into_std()
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .ok()
            })
            .map(|value| (value.as_secs(), value.subsec_nanos()));
        #[cfg(windows)]
        let created = metadata
            .created()
            .ok()
            .and_then(|time| {
                time.into_std()
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .ok()
            })
            .map(|value| (value.as_secs(), value.subsec_nanos()));
        #[cfg(unix)]
        use cap_std::fs::MetadataExt;
        Self {
            mode,
            size: metadata.len(),
            modified,
            #[cfg(unix)]
            changed: (metadata.ctime(), metadata.ctime_nsec()),
            #[cfg(unix)]
            special_device: metadata.rdev(),
            #[cfg(windows)]
            created,
            #[cfg(unix)]
            identity: (metadata.dev(), metadata.ino()),
        }
    }

    fn matches(&self, entry: &IndexEntry) -> bool {
        self.mode == entry.mode
            && self.size == u64::from(entry.file_size)
            && self.modified
                == Some((
                    entry.mtime.seconds() as u32 as u64,
                    entry.mtime.nanoseconds(),
                ))
            && {
                #[cfg(unix)]
                {
                    self.changed
                        == (
                            i64::from(entry.ctime.seconds()),
                            i64::from(entry.ctime.nanoseconds()),
                        )
                    // Git caches st_dev; the pinned libgit2 caches st_rdev.
                    && (self.identity.0 as u32 == entry.dev
                        || self.special_device as u32 == entry.dev)
                        && self.identity.1 as u32 == entry.ino
                }
                // libgit2 maps Windows index ctime to the file's creation time.
                #[cfg(windows)]
                {
                    self.created
                        == Some((
                            entry.ctime.seconds() as u32 as u64,
                            entry.ctime.nanoseconds(),
                        ))
                }
                #[cfg(not(any(unix, windows)))]
                {
                    true
                }
            }
    }
}

fn file_mode(metadata: &Metadata) -> u32 {
    #[cfg(unix)]
    {
        use cap_std::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 != 0 {
            0o100755
        } else {
            0o100644
        }
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0o100644
    }
}

struct Scan {
    visited: usize,
    omitted: usize,
}

impl Scan {
    fn visit(&mut self) -> bool {
        self.visited += 1;
        if self.visited > MAX_SCAN_ENTRIES {
            self.omitted += 1;
            false
        } else {
            true
        }
    }
    fn name<'a>(&mut self, bytes: &'a [u8]) -> Option<&'a str> {
        let value = std::str::from_utf8(bytes)
            .ok()
            .filter(|path| valid_path(path).is_ok());
        if value.is_none() {
            self.omitted += 1;
        }
        value
    }
}

pub(super) fn start(
    root: &Path,
    repository: &Repository,
    target: GitChangeTarget,
    control: &Control,
    cache: &Cache,
) -> Result<Arc<Capture>, Fault> {
    let target = match target {
        GitChangeTarget::Worktree { scope } => GitChangeState::Worktree {
            scope,
            head: head(repository)?,
            index_revision: index::revision(repository)?,
        },
        GitChangeTarget::Commit { revision } => {
            let commit = resolve(repository, &revision)?;
            let parent = commit.parent_ids().next().map(|id| id.to_string());
            GitChangeState::Commit {
                revision: commit.id().to_string(),
                parent,
            }
        }
        GitChangeTarget::Range { base, head } => GitChangeState::Range {
            base: resolve(repository, &base)?.id().to_string(),
            head: resolve(repository, &head)?.id().to_string(),
        },
    };
    capture(root, repository, target, control).map(|capture| cache.save(root, capture))
}

pub(super) fn restore(
    root: &Path,
    repository: &Repository,
    snapshot: &GitChangeSnapshot,
    control: &Control,
    cache: &Cache,
    full: bool,
) -> Result<Arc<Capture>, Fault> {
    super::digest(&snapshot.metadata_revision)?;
    validate_target(repository, snapshot, control)?;
    if !full && let Some(capture) = cache.get(root, snapshot) {
        return Ok(capture);
    }
    let capture = capture(root, repository, snapshot.target.clone(), control)?;
    if capture.snapshot != *snapshot {
        return Err(stale());
    }
    validate_target(repository, snapshot, control)?;
    Ok(cache.save(root, capture))
}

fn validate_target(
    repository: &Repository,
    snapshot: &GitChangeSnapshot,
    control: &Control,
) -> Result<(), Fault> {
    control.check()?;
    let GitChangeState::Worktree {
        head: expected_head,
        index_revision,
        ..
    } = &snapshot.target
    else {
        return Ok(());
    };
    if head(repository)? != *expected_head || index::revision(repository)? != *index_revision {
        return Err(stale());
    }
    Ok(())
}

pub(super) fn validate(
    root: &Path,
    repository: &Repository,
    snapshot: &GitChangeSnapshot,
    candidate: &Candidate,
    control: &Control,
) -> Result<(), Fault> {
    validate_target(repository, snapshot, control)?;
    let GitChangeState::Worktree { scope, .. } = &snapshot.target else {
        return Ok(());
    };
    if *scope != GitDiffScope::Staged {
        let expected = match &candidate.new {
            Some(Resource::Working(stamp)) => Some(stamp),
            None => None,
            _ => return Err(stale()),
        };
        if stamp(root, &candidate.entry.path)?.as_ref() != expected {
            return Err(stale());
        }
    }
    Ok(())
}

fn capture(
    root: &Path,
    repository: &Repository,
    target: GitChangeState,
    control: &Control,
) -> Result<Capture, Fault> {
    let mut scan = Scan {
        visited: 0,
        omitted: 0,
    };
    let (old, new, unchanged) = match &target {
        GitChangeState::Commit { revision, parent } => {
            let commit = fixed(repository, revision)?;
            if commit.parent_ids().next().map(|id| id.to_string()) != *parent {
                return Err(stale());
            }
            (
                tree(repository, parent.as_deref(), &mut scan, control)?,
                tree(repository, Some(revision), &mut scan, control)?,
                BTreeSet::new(),
            )
        }
        GitChangeState::Range { base, head } => (
            tree(repository, Some(base), &mut scan, control)?,
            tree(repository, Some(head), &mut scan, control)?,
            BTreeSet::new(),
        ),
        GitChangeState::Worktree {
            scope,
            head: expected_head,
            index_revision,
        } => {
            if head(repository)? != *expected_head
                || index::revision(repository)? != *index_revision
            {
                return Err(stale());
            }
            let head = tree(repository, expected_head.as_deref(), &mut scan, control)?;
            let index = repository.index().map_err(fault)?;
            let mut staged = BTreeMap::new();
            let mut stat = BTreeMap::new();
            for entry in index.iter() {
                control.check()?;
                if !scan.visit() {
                    break;
                }
                let Some(name) = scan.name(&entry.path).map(str::to_owned) else {
                    continue;
                };
                let conflict = entry.flags & 0x3000 != 0;
                staged.insert(
                    name.clone(),
                    Resource::Blob {
                        oid: entry.id.to_string(),
                        mode: if conflict { 0 } else { entry.mode },
                    },
                );
                stat.insert(name, entry);
            }
            if *scope == GitDiffScope::Staged {
                (head, staged, BTreeSet::new())
            } else {
                let mut names: BTreeSet<_> = head.keys().chain(staged.keys()).cloned().collect();
                let links: BTreeSet<_> = head
                    .iter()
                    .chain(staged.iter())
                    .filter(|(_, resource)| resource.mode() == 0o160000)
                    .map(|(name, _)| name.clone())
                    .collect();
                workdir(root, repository, &mut names, &links, &mut scan, control)?;
                let mut working = BTreeMap::new();
                let mut unchanged = BTreeSet::new();
                for name in names {
                    control.check()?;
                    match stamp(root, &name) {
                        Ok(Some(stamp)) => {
                            if stat.get(&name).is_some_and(|entry| {
                                entry.flags & 0x3000 == 0 && stamp.matches(entry)
                            }) && (*scope == GitDiffScope::Unstaged
                                || head.get(&name) == staged.get(&name))
                            {
                                unchanged.insert(name.clone());
                            }
                            working.insert(name, Resource::Working(stamp));
                        }
                        Ok(None) => {}
                        Err(_) => scan.omitted += 1,
                    }
                }
                (
                    if *scope == GitDiffScope::Unstaged {
                        staged
                    } else {
                        head
                    },
                    working,
                    unchanged,
                )
            }
        }
    };
    let mut entries = BTreeMap::new();
    for name in old.keys().chain(new.keys()).collect::<BTreeSet<_>>() {
        control.check()?;
        let before = old.get(name);
        let after = new.get(name);
        if unchanged.contains(name) || before == after {
            continue;
        }
        let kind = match (before, after) {
            (None, Some(_)) => GitChangeKind::Added,
            (Some(_), None) => GitChangeKind::Deleted,
            (Some(old), Some(new)) if old.mode() & 0o170000 != new.mode() & 0o170000 => {
                GitChangeKind::TypeChanged
            }
            _ => GitChangeKind::Modified,
        };
        let metadata_revision = hash(&(name, kind, before, after))?;
        entries.insert(
            name.clone(),
            Candidate {
                entry: GitChangeEntry {
                    path: name.clone(),
                    old_path: None,
                    kind,
                    old_mode: before.map(Resource::mode),
                    new_mode: after.map(Resource::mode),
                    metadata_revision,
                },
                old: before.cloned(),
                new: after.cloned(),
            },
        );
    }
    let metadata_revision = hash(&(
        &target,
        entries
            .values()
            .map(|value| &value.entry)
            .collect::<Vec<_>>(),
        scan.omitted,
    ))?;
    Ok(Capture {
        snapshot: GitChangeSnapshot {
            target,
            metadata_revision,
        },
        entries,
        omitted: scan.omitted,
    })
}

fn head(repository: &Repository) -> Result<Option<String>, Fault> {
    match repository.head() {
        Ok(head) => Ok(Some(head.peel_to_commit().map_err(fault)?.id().to_string())),
        Err(error)
            if matches!(
                error.code(),
                git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(fault(error)),
    }
}

fn resolve<'a>(repository: &'a Repository, value: &str) -> Result<git2::Commit<'a>, Fault> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(Fault::invalid("invalid Git revision"));
    }
    repository
        .revparse_single(value)
        .map_err(fault)?
        .peel_to_commit()
        .map_err(fault)
}

fn fixed<'a>(repository: &'a Repository, value: &str) -> Result<git2::Commit<'a>, Fault> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Fault::invalid("expected a full Git commit object ID"));
    }
    repository
        .find_commit(Oid::from_str(value).map_err(fault)?)
        .map_err(fault)
}

fn tree(
    repository: &Repository,
    commit: Option<&str>,
    scan: &mut Scan,
    control: &Control,
) -> Result<BTreeMap<String, Resource>, Fault> {
    let mut result = BTreeMap::new();
    let Some(commit) = commit else {
        return Ok(result);
    };
    let mut pending = vec![(
        fixed(repository, commit)?.tree().map_err(fault)?.id(),
        Vec::new(),
    )];
    while let Some((id, prefix)) = pending.pop() {
        control.check()?;
        for entry in &repository.find_tree(id).map_err(fault)? {
            if !scan.visit() {
                return Ok(result);
            }
            let mut name = prefix.clone();
            if !name.is_empty() {
                name.push(b'/');
            }
            name.extend_from_slice(entry.name_bytes());
            if entry.kind() == Some(ObjectType::Tree) {
                pending.push((entry.id(), name));
            } else if let Some(name) = scan.name(&name) {
                result.insert(
                    name.to_owned(),
                    Resource::Blob {
                        oid: entry.id().to_string(),
                        mode: entry.filemode() as u32,
                    },
                );
            }
        }
    }
    Ok(result)
}

fn workdir(
    root: &Path,
    repository: &Repository,
    names: &mut BTreeSet<String>,
    links: &BTreeSet<String>,
    scan: &mut Scan,
    control: &Control,
) -> Result<(), Fault> {
    let mut pending = vec![String::new()];
    while let Some(prefix) = pending.pop() {
        control.check()?;
        let dir = path::descend(path::root(root)?, &path::components(&prefix, true)?)?;
        for child in dir.entries().map_err(io_error)? {
            control.check()?;
            if !scan.visit() {
                return Ok(());
            }
            let child = child.map_err(io_error)?;
            let name = child.file_name();
            let Some(name) = name.to_str() else {
                scan.omitted += 1;
                continue;
            };
            if name.eq_ignore_ascii_case(".git") {
                continue;
            }
            let name = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            if valid_path(&name).is_err() {
                scan.omitted += 1;
                continue;
            }
            let kind = child.file_type().map_err(io_error)?;
            let ignored = repository
                .status_should_ignore(Path::new(&if kind.is_dir() {
                    format!("{name}/")
                } else {
                    name.clone()
                }))
                .map_err(fault)?;
            if ignored {
                continue;
            }
            // A tracked directory entry is a Git link, not another source subtree.
            if kind.is_dir() && !links.contains(&name) {
                pending.push(name);
            } else {
                names.insert(name);
            }
        }
    }
    Ok(())
}

pub(super) fn stamp(root: &Path, value: &str) -> Result<Option<Stamp>, Fault> {
    let parts = path::components(value, false)?;
    let directory = match path::descend(path::root(root)?, &parts[..parts.len() - 1]) {
        Ok(directory) => directory,
        Err(error) if error.code == ErrorCode::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    match directory.symlink_metadata(parts[parts.len() - 1]) {
        Ok(metadata) => Ok(Some(Stamp::of(&metadata))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(error)),
    }
}
