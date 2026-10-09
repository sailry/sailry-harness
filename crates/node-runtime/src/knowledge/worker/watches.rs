use super::super::*;
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

pub(super) struct Watches {
    backend: Option<RecommendedWatcher>,
    roots: BTreeSet<PathBuf>,
}

impl Watches {
    pub(super) fn new(owner: Weak<Service>) -> Self {
        let backend = RecommendedWatcher::new(
            move |event: notify::Result<notify::Event>| {
                if matches!(&event, Ok(event) if matches!(event.kind, EventKind::Access(_)) && !event.need_rescan()) {
                    return;
                }
                if let Some(owner) = owner.upgrade() {
                    owner.refresh();
                }
            },
            Config::default().with_follow_symlinks(false),
        )
        .ok();
        Self {
            backend,
            roots: BTreeSet::new(),
        }
    }

    pub(super) fn replace(&mut self, roots: BTreeSet<PathBuf>) {
        let Some(backend) = &mut self.backend else {
            return;
        };
        for root in self.roots.difference(&roots) {
            let _ = backend.unwatch(root);
        }
        for root in roots.difference(&self.roots) {
            if let Err(error) = backend.watch(root, RecursiveMode::Recursive) {
                eprintln!("knowledge watch unavailable; periodic rescan remains active: {error}");
            }
        }
        self.roots = roots;
    }
}
