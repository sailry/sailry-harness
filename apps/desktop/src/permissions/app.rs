//! Current-process permissions, distinct from an operation's execution-device requirements.
use super::*;
use std::{
    io,
    path::{Path, PathBuf},
};

pub(super) const DISK_SETTINGS: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

pub(super) fn catalog(overrides: Vec<Card>, cx: &App) -> Vec<Card> {
    let enabled = cfg!(target_os = "macos") && cx.has_global::<crate::backend::Services>();
    let mut cards = vec![
        native(Resource::FullDisk, DISK_SETTINGS),
        native(
            Resource::Accessibility,
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        ),
        native(
            Resource::Screen,
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
        ),
        microphone::card(Status::Unknown),
    ];
    if !enabled {
        for card in &mut cards {
            card.status = Status::Unavailable;
            card.check = None;
            card.request = None;
        }
    }
    for card in overrides {
        if let Some(existing) = cards
            .iter_mut()
            .find(|existing| existing.resource == card.resource)
        {
            *existing = card;
        } else {
            cards.push(card);
        }
    }
    cards
}

fn native(resource: Resource, settings: &'static str) -> Card {
    let check: Action = Rc::new(move |cx, _| {
        cx.background_executor()
            .spawn(async move { Ok(vec![(resource, status(resource))]) })
    });
    let request: Action = Rc::new(move |cx, stop| {
        cx.background_executor().spawn(async move {
            if stop.is_cancelled() {
                return Err(Failure {
                    key: "permission_cancelled".into(),
                    status: Status::Unknown,
                });
            }
            #[cfg(target_os = "macos")]
            match resource {
                Resource::Screen => {
                    cua_platform_macos::permissions::status::request_screen_recording();
                }
                Resource::Accessibility => {
                    cua_platform_macos::permissions::status::request_accessibility();
                }
                _ => {}
            }
            Ok(vec![(resource, status(resource))])
        })
    });
    Card {
        resource,
        status: Status::Unknown,
        settings: Some(settings),
        check: Some(check),
        request: Some(request),
        requires: None,
    }
}

fn status(resource: Resource) -> Status {
    #[cfg(target_os = "macos")]
    {
        use cua_platform_macos::permissions::status;
        let granted = match resource {
            Resource::FullDisk => {
                return dirs::home_dir()
                    .map(|home| disk_status(&home.join(".Trash")))
                    .unwrap_or(Status::Unknown);
            }
            Resource::Screen => status::screen_recording_granted(),
            Resource::Accessibility => status::accessibility_granted(),
            _ => return Status::Unknown,
        };
        if granted {
            Status::Granted
        } else {
            Status::Required
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = resource;
        Status::Unavailable
    }
}

fn disk_status(path: &Path) -> Status {
    // WWDC 2019 session 701: Trash enumeration needs Full Disk Access.
    // A denied protected open may list the responsible app unchecked in Settings.
    // Drop the iterator without visiting entries: never read contents or alter the Trash.
    disk_result(std::fs::read_dir(path).map(drop))
}

fn disk_result(result: io::Result<()>) -> Status {
    match result {
        Ok(()) => Status::Granted,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Status::Required,
        Err(_) => Status::Unknown,
    }
}

pub(super) fn bundle() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    bundle_at(&executable)
}

fn bundle_at(executable: &Path) -> Option<PathBuf> {
    executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn disk_errors_do_not_imply_authorization() {
        assert_eq!(disk_result(Ok(())), Status::Granted);
        assert_eq!(
            disk_result(Err(io::ErrorKind::PermissionDenied.into())),
            Status::Required
        );
        for kind in [io::ErrorKind::NotFound, io::ErrorKind::Other] {
            assert_eq!(disk_result(Err(kind.into())), Status::Unknown);
        }
    }

    #[test]
    fn directory_check_avoids_content_access() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("unopened");
        std::fs::write(&file, b"preserved").unwrap();
        assert_eq!(disk_status(root.path()), Status::Granted);
        assert_eq!(std::fs::read(file).unwrap(), b"preserved");
        assert_eq!(disk_status(&root.path().join("missing")), Status::Unknown);
    }

    #[test]
    fn reveals_the_bundle_not_a_bare_executable() {
        assert_eq!(
            bundle_at(Path::new(
                "/Applications/Sailry.app/Contents/MacOS/sailry-desktop"
            )),
            Some(PathBuf::from("/Applications/Sailry.app"))
        );
        assert_eq!(
            bundle_at(Path::new("/work/target/debug/sailry-desktop")),
            None
        );
    }

    #[gpui::test]
    async fn cancelled_requests_do_not_prompt(cx: &mut TestAppContext) {
        for resource in [Resource::Screen, Resource::Accessibility] {
            let stop = CancellationToken::new();
            stop.cancel();
            let task = cx.update(|cx| native(resource, DISK_SETTINGS).request.unwrap()(cx, stop));
            let failure = task.await.err().unwrap();
            assert_eq!(failure.key, "permission_cancelled");
            assert_eq!(failure.status, Status::Unknown);
        }
    }
}
