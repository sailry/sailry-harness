use super::*;
use crate::ui::path_picker::{Kind, PathPicker, Picked};
use std::{io::Read, path::PathBuf};

pub(super) struct Imported {
    pub path: PathBuf,
    pub key: Secret,
}

impl Editor {
    pub(super) fn key_file_field(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div().debug_selector(|| "ssh-key-file".into()).child(
            PathPicker::new("ssh-key-file")
                .kind(Kind::File)
                .label("ssh_key_choose")
                .compact(true)
                .value(
                    self.file
                        .as_ref()
                        .map(|file| Picked::new(file.path.clone())),
                )
                .busy(self.picking)
                .disabled(self.locked)
                .on_pick(cx.listener(|this, path: &PathBuf, window, cx| {
                    this.load_key(path.clone(), window, cx)
                }))
                .on_clear(cx.listener(|this, _: &ClickEvent, _, cx| {
                    if !this.locked && !this.picking {
                        this.file = None;
                        cx.notify();
                    }
                })),
        )
    }

    fn load_key(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.locked || self.picking {
            return;
        }
        self.picking = true;
        self.file_error = None;
        let read = cx.background_executor().spawn(async move { read(path) });
        cx.spawn_in(window, async move |this, cx| {
            let result = read.await;
            let _ = this.update_in(cx, |this, _, cx| {
                this.picking = false;
                match result {
                    Ok(file) => {
                        this.file = Some(file);
                        this.invalid = false;
                    }
                    Err(error) => this.file_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

fn read(path: PathBuf) -> Result<Imported, &'static str> {
    let file = std::fs::File::open(&path).map_err(|_| "ssh_key_read_failed")?;
    if !file
        .metadata()
        .map_err(|_| "ssh_key_read_failed")?
        .is_file()
    {
        return Err("ssh_key_invalid_file");
    }
    let mut contents = String::new();
    file.take(65537)
        .read_to_string(&mut contents)
        .map_err(|_| "ssh_key_read_failed")?;
    if contents.is_empty() || contents.len() > 65536 {
        return Err("ssh_key_invalid_file");
    }
    // Import credentials into the execution Node's existing protected storage.
    // A controller-local path must never be interpreted on a remote Node.
    Ok(Imported {
        path,
        key: Secret::new(contents),
    })
}
