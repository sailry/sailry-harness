use super::*;
use std::{io::Read, path::PathBuf};

pub(super) struct Imported {
    pub name: SharedString,
    pub key: Secret,
}

impl Editor {
    pub(super) fn key_file_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        Button::new("ssh-key-file")
            .debug_selector(|| "ssh-key-file".into())
            .w_full()
            .icon(IconName::Folder)
            .label(
                self.file
                    .as_ref()
                    .map(|file| file.name.clone())
                    .unwrap_or_else(|| tr("ssh_key_choose")),
            )
            .disabled(self.locked || self.picking)
            .on_click(cx.listener(|this, _, window, cx| this.choose_key(window, cx)))
    }

    fn choose_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.locked || self.picking {
            return;
        }
        self.picking = true;
        self.file_error = None;
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr("ssh_key_choose")),
        });
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = match selected.await {
                Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                    Some(path) => executor.spawn(async move { read(path) }).await.map(Some),
                    None => Ok(None),
                },
                Ok(Ok(None)) => Ok(None),
                _ => Err("ssh_key_read_failed"),
            };
            let _ = this.update_in(cx, |this, _, cx| {
                this.picking = false;
                match result {
                    Ok(Some(file)) => {
                        this.file = Some(file);
                        this.invalid = false;
                    }
                    Ok(None) => {}
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
        name: path.to_string_lossy().into_owned().into(),
        key: Secret::new(contents),
    })
}
