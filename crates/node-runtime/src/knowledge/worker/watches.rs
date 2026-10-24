use super::super::*;
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

type Rules = BTreeMap<PathBuf, Vec<source::Watch>>;

pub(super) struct Watches {
    owner: Weak<Service>,
    backend: Option<RecommendedWatcher>,
    registered: BTreeSet<PathBuf>,
    rules: Arc<Mutex<Rules>>,
    desired: Rules,
    failed: Arc<AtomicBool>,
    incomplete: bool,
    changed: bool,
}

impl Watches {
    pub(super) fn new(owner: Weak<Service>) -> Self {
        let rules = Arc::new(Mutex::new(Rules::new()));
        let failed = Arc::new(AtomicBool::new(false));
        let backend = start(owner.clone(), rules.clone(), failed.clone());
        Self {
            owner,
            backend,
            registered: BTreeSet::new(),
            rules,
            desired: Rules::new(),
            failed,
            incomplete: false,
            changed: false,
        }
    }

    pub(super) fn begin(&mut self) {
        self.desired.clear();
        if self.failed.swap(false, Ordering::AcqRel) || self.backend.is_none() {
            // A backend error or dropped-events report invalidates every registration.
            // Rebuild instead of treating retained path names as functioning watches.
            self.backend = start(self.owner.clone(), self.rules.clone(), self.failed.clone());
            self.registered.clear();
        }
    }

    pub(super) fn add(&mut self, base: &Path, watches: Vec<source::Watch>) {
        for watch in watches {
            let path = base.join(&watch.relative);
            self.desired.entry(path.clone()).or_default().push(watch);
            // Register before extracting or encoding the newly discovered directory.
            // Earlier roots remain active until a complete pass finishes.
            self.rules
                .lock()
                .expect("knowledge watches lock is healthy")
                .insert(path.clone(), self.desired[&path].clone());
            if !self.registered.contains(&path)
                && let Some(backend) = &mut self.backend
            {
                match backend.watch(&path, RecursiveMode::NonRecursive) {
                    Ok(()) => {
                        self.registered.insert(path);
                        self.changed = true;
                    }
                    Err(error) => {
                        self.failed.store(true, Ordering::Release);
                        eprintln!(
                            "knowledge watch unavailable; periodic rescan remains active: {error}"
                        );
                    }
                }
            }
        }
    }

    pub(super) fn finish(&mut self, complete: bool) {
        self.incomplete = !complete;
        if complete {
            let removed = self
                .registered
                .iter()
                .filter(|path| !self.desired.contains_key(*path))
                .cloned()
                .collect::<Vec<_>>();
            for path in removed {
                if let Some(backend) = &mut self.backend
                    && let Err(error) = backend.unwatch(&path)
                {
                    self.failed.store(true, Ordering::Release);
                    eprintln!("knowledge watch removal failed: {error}");
                }
                self.registered.remove(&path);
                self.changed = true;
            }
            *self
                .rules
                .lock()
                .expect("knowledge watches lock is healthy") = self.desired.clone();
        }
        // FSEvents restarts and purges pending events when watch paths change.
        // Recheck after the entire topology settles, including writes made after Ready.
        if std::mem::take(&mut self.changed)
            && self.backend.is_some()
            && !self.failed.load(Ordering::Acquire)
            && let Some(owner) = self.owner.upgrade()
        {
            owner.refresh();
        }
    }

    pub(super) fn needs_poll(&self) -> bool {
        self.backend.is_none() || self.incomplete || self.failed.load(Ordering::Acquire)
    }
}

fn start(
    owner: Weak<Service>,
    rules: Arc<Mutex<Rules>>,
    failed: Arc<AtomicBool>,
) -> Option<RecommendedWatcher> {
    RecommendedWatcher::new(
        move |event: notify::Result<notify::Event>| {
            let dirty = match event {
                Ok(event) => {
                    let rules = rules.lock().expect("knowledge watches lock is healthy");
                    if event.need_rescan()
                        || (matches!(
                            event.kind,
                            EventKind::Remove(_)
                                | EventKind::Modify(notify::event::ModifyKind::Name(_))
                        ) && event.paths.iter().any(|path| rules.contains_key(path)))
                    {
                        failed.store(true, Ordering::Release);
                    }
                    relevant(&rules, &event)
                }
                Err(_) => {
                    failed.store(true, Ordering::Release);
                    true
                }
            };
            if dirty && let Some(owner) = owner.upgrade() {
                owner.refresh();
            }
        },
        Config::default().with_follow_symlinks(false),
    )
    .ok()
}

fn relevant(rules: &Rules, event: &notify::Event) -> bool {
    if event.need_rescan() {
        return true;
    }
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    event.paths.iter().any(|path| {
        if rules.contains_key(path) {
            return true;
        }
        let Some(parent) = path.parent().and_then(|parent| rules.get(parent)) else {
            return false;
        };
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return false;
        };
        let directory = match event.kind {
            EventKind::Create(notify::event::CreateKind::Folder)
            | EventKind::Remove(notify::event::RemoveKind::Folder) => Some(true),
            EventKind::Create(notify::event::CreateKind::File)
            | EventKind::Remove(notify::event::RemoveKind::File) => Some(false),
            _ => std::fs::symlink_metadata(path)
                .ok()
                .map(|metadata| metadata.is_dir()),
        };
        parent.iter().any(|rule| match directory {
            Some(directory) => rule.accepts(name, directory),
            None => rule.accepts(name, false) || rule.accepts(name, true),
        })
    })
}

#[cfg(test)]
mod tests;
