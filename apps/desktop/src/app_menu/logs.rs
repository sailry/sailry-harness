use gpui_kit::{App, Window};
use std::{io, path::PathBuf};

pub(super) fn open(window: &mut Window, cx: &mut App) {
    let job = cx.background_executor().spawn(async {
        let path = sailry_node_runtime::logging::current_file()?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "log file is unavailable"))?
            .canonicalize()?;
        #[cfg(target_os = "macos")]
        {
            // GPUI's system opener only selects the default application.
            // Console requires LaunchServices' explicit bundle selection.
            let status = command(&path).status()?;
            if !status.success() {
                return Err(io::Error::other("log viewer could not open the file"));
            }
        }
        Ok::<PathBuf, io::Error>(path)
    });
    window
        .spawn(cx, async move |cx| {
            let result = job.await;
            let _ = cx.update(|window, cx| match result {
                Ok(path) => {
                    #[cfg(not(target_os = "macos"))]
                    cx.open_with_system(&path);
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
fn command(path: &std::path::Path) -> std::process::Command {
    let mut command = std::process::Command::new("/usr/bin/open");
    command.args(["-b", "com.apple.Console", "--"]).arg(path);
    command
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::{ffi::OsStr, path::Path};

    #[test]
    fn selects_console_for_current_file() {
        let path = Path::new("/isolated profile/logs/desktop/2026-10-13.log");
        let command = command(path);
        assert_eq!(command.get_program(), OsStr::new("/usr/bin/open"));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                OsStr::new("-b"),
                OsStr::new("com.apple.Console"),
                OsStr::new("--"),
                path.as_os_str(),
            ]
        );
    }
}
