use super::*;
use cap_std::fs::ReadDir;
use ignore::gitignore::Gitignore;

struct Frame {
    directory: Dir,
    entries: ReadDir,
    relative: String,
    ignore: Option<Gitignore>,
}

#[derive(Clone)]
pub(in crate::knowledge) struct Watch {
    pub relative: String,
    layers: Vec<Gitignore>,
    file: Option<String>,
    ignore_file: bool,
}

impl Watch {
    pub(in crate::knowledge) fn accepts(&self, name: &str, directory: bool) -> bool {
        if ignores::excluded(name) {
            return false;
        }
        if name == ".gitignore" && self.ignore_file {
            return true;
        }
        if self.file.as_deref().is_some_and(|file| file != name) {
            return false;
        }
        !ignores::excludes(self.layers.iter(), &join(&self.relative, name), directory)
    }
}

impl Frame {
    fn new(directory: Dir, relative: String, cancel: &CancellationToken) -> Result<Self, Fault> {
        let ignore = ignores::load(&directory, &relative, cancel)?;
        let entries = directory.entries().map_err(files::io_error)?;
        Ok(Self {
            directory,
            entries,
            relative,
            ignore,
        })
    }
}

pub(in crate::knowledge) struct Scan {
    ocr: Option<std::sync::Arc<crate::ocr::Service>>,
    frames: Vec<Frame>,
    inherited: Vec<Gitignore>,
    pending: Option<Target>,
    omitted: Option<Entry>,
    base: Option<PathBuf>,
    cancel: CancellationToken,
    finished: bool,
    incomplete: bool,
    watches: Vec<Watch>,
}

impl Scan {
    pub(super) fn select(
        mut directory: Dir,
        components: &[&str],
        base: Option<PathBuf>,
        cancel: CancellationToken,
    ) -> Result<Self, Fault> {
        let mut scan = Self {
            ocr: None,
            frames: Vec::new(),
            inherited: Vec::new(),
            pending: None,
            omitted: None,
            base,
            cancel,
            finished: false,
            incomplete: false,
            watches: Vec::new(),
        };
        let mut relative = String::new();
        for (index, name) in components.iter().enumerate() {
            cancelled(&scan.cancel)?;
            if let Some(layer) = ignores::load(&directory, &relative, &scan.cancel)? {
                scan.inherited.push(layer);
            }
            scan.watches.push(Watch {
                relative: relative.clone(),
                layers: scan.inherited.iter().rev().cloned().collect(),
                file: Some((*name).into()),
                ignore_file: true,
            });
            relative = join(&relative, name);
            if ignores::excluded(name) {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "protected source paths cannot be indexed",
                ));
            }
            let metadata = directory.symlink_metadata(name).map_err(files::io_error)?;
            if metadata.is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
                return Err(Fault::invalid(
                    "knowledge source must be a regular file or directory",
                ));
            }
            if ignores::excludes(scan.inherited.iter().rev(), &relative, metadata.is_dir()) {
                scan.omitted = Some(excluded(
                    scan.locator(&relative),
                    "source is excluded by .gitignore",
                ));
                return Ok(scan);
            }
            if metadata.is_file() {
                if index + 1 != components.len() {
                    return Err(Fault::invalid("knowledge source has a file as a parent"));
                }
                scan.pending = Some(Target {
                    directory,
                    name: (*name).into(),
                    relative,
                });
                return Ok(scan);
            }
            directory = files::path::descend(directory, &[*name])?;
        }
        scan.frames
            .push(Frame::new(directory, relative, &scan.cancel)?);
        scan.watch_frame();
        Ok(scan)
    }

    pub(super) fn file(
        directory: Dir,
        name: &str,
        base: Option<PathBuf>,
        cancel: CancellationToken,
    ) -> Result<Self, Fault> {
        // A selected file is explicit; sibling ignore files do not broaden its capture.
        let metadata = directory.symlink_metadata(name).map_err(files::io_error)?;
        if !metadata.is_file() || metadata.is_symlink() {
            return Err(Fault::invalid("knowledge source must be a regular file"));
        }
        Ok(Self {
            ocr: None,
            frames: Vec::new(),
            inherited: Vec::new(),
            pending: Some(Target {
                directory,
                name: name.into(),
                relative: name.into(),
            }),
            omitted: None,
            base,
            cancel,
            finished: false,
            incomplete: false,
            watches: vec![Watch {
                relative: String::new(),
                layers: Vec::new(),
                file: Some(name.into()),
                ignore_file: false,
            }],
        })
    }

    fn locator(&self, relative: &str) -> String {
        match &self.base {
            Some(base) => base.join(relative).to_string_lossy().into_owned(),
            None => relative.into(),
        }
    }

    fn watch_frame(&mut self) {
        let frame = self.frames.last().expect("scanned directory has a frame");
        self.watches.push(Watch {
            relative: frame.relative.clone(),
            layers: self
                .frames
                .iter()
                .rev()
                .filter_map(|frame| frame.ignore.clone())
                .chain(self.inherited.iter().rev().cloned())
                .collect(),
            file: None,
            ignore_file: true,
        });
    }

    fn watch_ignore(&mut self, relative: String) {
        self.watches.push(Watch {
            relative,
            layers: self
                .frames
                .iter()
                .rev()
                .filter_map(|frame| frame.ignore.clone())
                .chain(self.inherited.iter().rev().cloned())
                .collect(),
            file: Some(".gitignore".into()),
            ignore_file: true,
        });
    }

    pub(in crate::knowledge) fn take_watches(&mut self) -> Vec<Watch> {
        std::mem::take(&mut self.watches)
    }

    pub(in crate::knowledge) fn with_ocr(
        mut self,
        ocr: std::sync::Arc<crate::ocr::Service>,
    ) -> Self {
        self.ocr = Some(ocr);
        self
    }

    fn extract(
        &mut self,
        target: Target,
        reuse: &mut impl FnMut(&str, Stat) -> Result<bool, Fault>,
    ) -> Result<Entry, Fault> {
        let path = self.locator(&target.relative);
        match read::document(
            target,
            path.clone(),
            &self.cancel,
            reuse,
            self.ocr.as_deref(),
        ) {
            Ok(entry) => Ok(entry),
            Err(error) if error.code != ErrorCode::InvalidRequest => Err(error),
            Err(error) => Ok(skipped(path, error.message)),
        }
    }

    fn advance(
        &mut self,
        reuse: &mut impl FnMut(&str, Stat) -> Result<bool, Fault>,
        watch: &mut impl FnMut(Vec<Watch>),
    ) -> Result<Option<Entry>, Fault> {
        loop {
            cancelled(&self.cancel)?;
            watch(self.take_watches());
            if let Some(entry) = self.omitted.take() {
                return Ok(Some(entry));
            }
            if let Some(target) = self.pending.take() {
                return self.extract(target, reuse).map(Some);
            }
            let Some(frame) = self.frames.last_mut() else {
                return Ok(None);
            };
            let Some(entry) = frame.entries.next() else {
                self.frames.pop();
                continue;
            };
            let parent = frame.relative.clone();
            let directory = frame.directory.try_clone().map_err(files::io_error)?;
            let name = match entry {
                Ok(entry) => match entry.file_name().into_string() {
                    Ok(name) => name,
                    Err(_) => {
                        return Ok(Some(skipped(
                            self.locator(&parent),
                            "entry name is not UTF-8",
                        )));
                    }
                },
                Err(error) => return Err(files::io_error(error)),
            };
            let relative = join(&parent, &name);
            let path = self.locator(&relative);
            if ignores::excluded(&name) {
                return Ok(Some(excluded(path, "protected source path")));
            }
            if files::path::components(&name, false).is_err() {
                return Ok(Some(skipped(path, "unsupported source entry name")));
            }
            let metadata = match directory.symlink_metadata(&name) {
                Ok(metadata) => metadata,
                Err(error) => return Err(files::io_error(error)),
            };
            if metadata.is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
                return Ok(Some(skipped(
                    path,
                    "source is not a regular file or directory",
                )));
            }
            let layers = self
                .frames
                .iter()
                .rev()
                .filter_map(|frame| frame.ignore.as_ref())
                .chain(self.inherited.iter().rev());
            if ignores::excludes(layers, &relative, metadata.is_dir()) {
                return Ok(Some(excluded(path, "source is excluded by .gitignore")));
            }
            if metadata.is_dir() {
                if self.frames.len() >= 64 {
                    return Err(Fault::invalid("directory nesting exceeds 64 levels"));
                }
                let child = files::path::descend(directory, &[&name])
                    .and_then(|directory| Frame::new(directory, relative.clone(), &self.cancel));
                match child {
                    Ok(frame) => {
                        self.frames.push(frame);
                        self.watch_frame();
                    }
                    Err(error) if error.code == ErrorCode::InvalidRequest => {
                        self.incomplete = true;
                        self.watch_ignore(relative);
                        watch(self.take_watches());
                        return Ok(Some(skipped(path, error.message)));
                    }
                    Err(error) => return Err(error),
                }
                continue;
            }
            return self
                .extract(
                    Target {
                        directory,
                        name,
                        relative,
                    },
                    reuse,
                )
                .map(Some);
        }
    }
}

impl Scan {
    pub(in crate::knowledge) fn complete(&self) -> bool {
        self.finished && !self.incomplete
    }

    pub(in crate::knowledge) fn next_cached(
        &mut self,
        reuse: &mut impl FnMut(&str, Stat) -> Result<bool, Fault>,
    ) -> Option<Result<Entry, Fault>> {
        self.next_watched(reuse, &mut |_| {})
    }

    pub(in crate::knowledge) fn next_watched(
        &mut self,
        reuse: &mut impl FnMut(&str, Stat) -> Result<bool, Fault>,
        watch: &mut impl FnMut(Vec<Watch>),
    ) -> Option<Result<Entry, Fault>> {
        if self.finished {
            return None;
        }
        match self.advance(reuse, watch) {
            Ok(Some(entry)) => Some(Ok(entry)),
            Ok(None) => {
                self.finished = true;
                None
            }
            Err(error) => {
                self.finished = true;
                Some(Err(error))
            }
        }
    }
}

impl Iterator for Scan {
    type Item = Result<Entry, Fault>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_cached(&mut |_, _| Ok(false))
    }
}

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.into()
    } else {
        format!("{parent}/{name}")
    }
}
