//! Isolated, in-memory file fixtures. Preview never accesses the host filesystem.
use super::*;
use sailry_protocol::{
    Directory,
    file_browser::{Location, LocationKind},
};
use std::collections::BTreeMap;

pub(super) struct Catalog {
    entries: BTreeMap<String, EntryKind>,
    home: &'static str,
}

impl Catalog {
    pub fn new(host: usize) -> Self {
        let home = if host == 0 {
            "/preview/sailry"
        } else {
            "/preview/remote-workspace"
        };
        let scratch = if host == 0 {
            "/preview/scratch"
        } else {
            "/preview/remote-scratch"
        };
        let mut entries: BTreeMap<_, _> = [
            "/preview",
            home,
            scratch,
            "/preview/empty",
            "/preview/restricted",
            "/preview/many",
        ]
        .into_iter()
        .map(|path| (path.into(), EntryKind::Directory))
        .collect();
        for path in [
            format!("{home}/src"),
            format!("{home}/docs"),
            format!("{scratch}/new-project"),
        ] {
            entries.insert(path, EntryKind::Directory);
        }
        for index in 0..64 {
            entries.insert(
                format!("/preview/many/directory-{index:02}-with-a-long-preview-name"),
                EntryKind::Directory,
            );
        }
        Self { entries, home }
    }

    pub fn list(&self, path: &str) -> Result<Listing, &'static str> {
        let path = path.trim().trim_end_matches('/');
        if path == "/preview/restricted" {
            return Err("directory_denied");
        }
        if self.entries.get(path) != Some(&EntryKind::Directory) {
            return Err("directory_missing");
        }
        let mut entries: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(entry, kind)| {
                let (parent, name) = entry.rsplit_once('/')?;
                (parent == path).then(|| FileEntry {
                    name: name.into(),
                    kind: *kind,
                    size: 0,
                })
            })
            .collect();
        entries.sort_by(|left, right| left.sort_key().cmp(&right.sort_key()));
        Ok(Listing {
            separator: '/',
            locations: vec![
                Location {
                    kind: LocationKind::Root,
                    name: String::new(),
                    path: "/preview".into(),
                },
                Location {
                    kind: LocationKind::Home,
                    name: String::new(),
                    path: self.home.into(),
                },
            ],
            parent: (path != "/preview").then(|| path.rsplit_once('/').unwrap().0.into()),
            directory: Directory {
                path: path.into(),
                entries,
                truncated: false,
                unsupported_names: 0,
                revision: "preview".into(),
                next: None,
            },
        })
    }

    pub fn apply(&mut self, action: &FileAction) -> Result<(), &'static str> {
        match action {
            FileAction::CreateDirectory { path } => {
                self.destination(path)?;
                self.entries.insert(path.clone(), EntryKind::Directory);
            }
            FileAction::Rename { from, to } => {
                self.destination(to)?;
                self.transfer(from, to, true)?;
            }
            FileAction::Trash { paths } => {
                for path in paths {
                    self.remove(path)?;
                }
            }
            FileAction::Transfer {
                paths,
                destination,
                cut,
                conflict,
            } => {
                self.list(destination)?;
                for path in paths {
                    let name = path.rsplit_once('/').ok_or("directory_missing")?.1;
                    let mut target = format!("{destination}/{name}");
                    if self.entries.contains_key(&target) {
                        match conflict {
                            Conflict::Stop => return Err("files_paste_conflict"),
                            Conflict::Replace => {
                                if path == &target || path.starts_with(&format!("{target}/")) {
                                    return Err("files_paste_conflict");
                                }
                                self.remove(&target)?;
                            }
                            Conflict::KeepBoth => {
                                let mut index = 2;
                                while self.entries.contains_key(&target) {
                                    target = format!("{destination}/{name} ({index})");
                                    index += 1;
                                }
                            }
                        }
                    }
                    self.transfer(path, &target, *cut)?;
                }
            }
        }
        Ok(())
    }

    fn destination(&self, path: &str) -> Result<(), &'static str> {
        let (parent, name) = path.rsplit_once('/').ok_or("directory_missing")?;
        if name.is_empty() || name == "." || name == ".." || self.entries.contains_key(path) {
            return Err("files_paste_conflict");
        }
        self.list(parent).map(|_| ())
    }

    fn transfer(&mut self, from: &str, to: &str, cut: bool) -> Result<(), &'static str> {
        if !self.entries.contains_key(from) || to.starts_with(&format!("{from}/")) {
            return Err("files_paste_conflict");
        }
        let copied: Vec<_> = self
            .entries
            .iter()
            .filter(|(path, _)| *path == from || path.starts_with(&format!("{from}/")))
            .map(|(path, kind)| (format!("{to}{}", &path[from.len()..]), *kind))
            .collect();
        if cut {
            self.remove(from)?;
        }
        self.entries.extend(copied);
        Ok(())
    }

    fn remove(&mut self, path: &str) -> Result<(), &'static str> {
        if path == "/preview" || path == "/preview/restricted" || !self.entries.contains_key(path) {
            return Err("directory_denied");
        }
        self.entries
            .retain(|entry, _| entry != path && !entry.starts_with(&format!("{path}/")));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn host_scope() {
        let local = Catalog::new(0);
        assert!(local.list("/preview/remote-workspace").is_err());
        for path in ["/", "/preview/../private", "relative", ""] {
            assert!(local.list(path).is_err());
        }
        assert!(local.list("/preview/restricted").is_err());
        assert!(
            local
                .list("/preview/empty")
                .unwrap()
                .directory
                .entries
                .is_empty()
        );
        assert_eq!(
            local.list("/preview/many").unwrap().directory.entries.len(),
            64
        );
    }

    #[test]
    fn mutations_remain_in_memory() {
        let mut catalog = Catalog::new(0);
        catalog
            .apply(&FileAction::CreateDirectory {
                path: "/preview/created".into(),
            })
            .unwrap();
        catalog
            .apply(&FileAction::Rename {
                from: "/preview/created".into(),
                to: "/preview/renamed".into(),
            })
            .unwrap();
        catalog
            .apply(&FileAction::Transfer {
                paths: vec!["/preview/renamed".into()],
                destination: "/preview/empty".into(),
                cut: false,
                conflict: Conflict::Stop,
            })
            .unwrap();
        assert!(catalog.list("/preview/empty/renamed").is_ok());
        catalog
            .apply(&FileAction::Trash {
                paths: vec!["/preview/renamed".into()],
            })
            .unwrap();
        assert!(catalog.list("/preview/renamed").is_err());
        assert!(catalog.list("/preview/empty/renamed").is_ok());
    }
}
