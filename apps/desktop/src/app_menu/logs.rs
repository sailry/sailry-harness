use gpui_kit::{App, Window};
use std::{io, path::PathBuf};

pub(super) fn open(window: &mut Window, cx: &mut App) {
    let job = cx.background_executor().spawn(async {
        #[cfg(target_os = "macos")]
        {
            // Open the native viewer without selecting a text document. There is no
            // documented start/filter argument; those controls remain in Console.
            let status = command().status()?;
            if !status.success() {
                return Err(io::Error::other("log viewer could not open"));
            }
            Ok::<Option<PathBuf>, io::Error>(None)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let path = sailry_node_runtime::logging::current_file()?
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "log file is unavailable"))?
                .canonicalize()?;
            Ok::<Option<PathBuf>, io::Error>(Some(path))
        }
    });
    window
        .spawn(cx, async move |cx| {
            let result = job.await;
            let _ = cx.update(|window, cx| match result {
                Ok(path) => {
                    #[cfg(not(target_os = "macos"))]
                    if let Some(path) = path {
                        cx.open_with_system(&path);
                    }
                    #[cfg(target_os = "macos")]
                    let _ = path;
                }
                Err(_) => crate::feedback::error(
                    &crate::tr("menu_logs"),
                    &crate::tr("files_open_failed"),
                    window,
                    cx,
                ),
            });
        })
        .detach();
}

#[cfg(target_os = "macos")]
fn command() -> std::process::Command {
    let mut command = std::process::Command::new("/usr/bin/open");
    command.args(["-b", "com.apple.Console"]);
    command
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn opens_console_without_a_text_file() {
        let command = command();
        assert_eq!(command.get_program(), OsStr::new("/usr/bin/open"));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [OsStr::new("-b"), OsStr::new("com.apple.Console")]
        );
    }
}
