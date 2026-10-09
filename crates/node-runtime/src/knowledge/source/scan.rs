use super::*;
use cap_std::fs::ReadDir;
use ignore::gitignore::Gitignore;

struct Frame {
    directory: Dir,
    entries: ReadDir,
    relative: String,
    ignore: Option<Gitignore>,
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
    frames: Vec<Frame>,
    inherited: Vec<Gitignore>,
    pending: Option<Target>,
    omitted: Option<Entry>,
    base: Option<PathBuf>,
    cancel: CancellationToken,
    finished: bool,
}

impl Scan {
    pub(super) fn select(
        mut directory: Dir,
        components: &[&str],
        base: Option<PathBuf>,
        cancel: CancellationToken,
    ) -> Result<Self, Fault> {
        let mut scan = Self {
            frames: Vec::new(),
            inherited: Vec::new(),
            pending: None,
            omitted: None,
            base,
            cancel,
            finished: false,
        };
        let mut relative = String::new();
        for (index, name) in components.iter().enumerate() {
            cancelled(&scan.cancel)?;
            if let Some(layer) = ignores::load(&directory, &relative, &scan.cancel)? {
                scan.inherited.push(layer);
            }
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
                scan.omitted = Some(skipped(
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
        })
    }

    fn locator(&self, relative: &str) -> String {
        match &self.base {
            Some(base) => base.join(relative).to_string_lossy().into_owned(),
            None => relative.into(),
        }
    }

    fn extract(&mut self, target: Target) -> Result<Entry, Fault> {
        let path = self.locator(&target.relative);
        match read::document(target, path.clone(), &self.cancel) {
            Ok(document) => Ok(Entry::Document(document)),
            Err(error) if error.code == ErrorCode::Cancelled => Err(error),
            Err(error) => Ok(skipped(path, error.message)),
        }
    }

    fn advance(&mut self) -> Result<Option<Entry>, Fault> {
        loop {
            cancelled(&self.cancel)?;
            if let Some(entry) = self.omitted.take() {
                return Ok(Some(entry));
            }
            if let Some(target) = self.pending.take() {
                return self.extract(target).map(Some);
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
                Err(error) => return Ok(Some(skipped(self.locator(&parent), error))),
            };
            let relative = join(&parent, &name);
            let path = self.locator(&relative);
            if ignores::excluded(&name) {
                return Ok(Some(skipped(path, "protected source path")));
            }
            if files::path::components(&name, false).is_err() {
                return Ok(Some(skipped(path, "unsupported source entry name")));
            }
            let metadata = match directory.symlink_metadata(&name) {
                Ok(metadata) => metadata,
                Err(error) => return Ok(Some(skipped(path, error))),
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
                return Ok(Some(skipped(path, "source is excluded by .gitignore")));
            }
            if metadata.is_dir() {
                if self.frames.len() >= 64 {
                    return Ok(Some(skipped(path, "directory nesting exceeds 64 levels")));
                }
                let child = files::path::descend(directory, &[&name])
                    .and_then(|directory| Frame::new(directory, relative, &self.cancel));
                match child {
                    Ok(frame) => self.frames.push(frame),
                    Err(error) if error.code == ErrorCode::Cancelled => return Err(error),
                    Err(error) => return Ok(Some(skipped(path, error.message))),
                }
                continue;
            }
            return self
                .extract(Target {
                    directory,
                    name,
                    relative,
                })
                .map(Some);
        }
    }
}

impl Iterator for Scan {
    type Item = Result<Entry, Fault>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        match self.advance() {
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

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.into()
    } else {
        format!("{parent}/{name}")
    }
}
