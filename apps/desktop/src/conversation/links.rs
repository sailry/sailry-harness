//! One link normalization and routing decision for internal and external opening.
use super::file_reference::Target;
use gpui_kit::{App, Modifiers};
use sailry_protocol::SessionId;

/// Whether a link opens in Sailry or with this device's system handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Internal,
    External,
}

impl Mode {
    /// The platform modifier (Command on macOS, Control elsewhere) opens externally.
    pub(crate) fn of(modifiers: Modifiers, cx: &App) -> Self {
        if modifiers.secondary(cx) {
            Self::External
        } else {
            Self::Internal
        }
    }
}

/// Recover link forms Markdown leaves without a usable scheme.
pub(crate) fn normalize(link: &str) -> String {
    let link = link.trim();
    if link
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("www."))
    {
        return format!("https://{link}");
    }
    // `file:path` names a relative path; as a URL it would read as root-absolute.
    if let Some(path) = link
        .get(..5)
        .filter(|scheme| scheme.eq_ignore_ascii_case("file:"))
        .map(|_| &link[5..])
        .filter(|path| !path.is_empty() && !path.starts_with(['/', '\\']))
    {
        return path.to_owned();
    }
    link.to_owned()
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Route {
    /// Sailry's browser resource.
    Browser,
    /// The system default handler.
    System,
    /// A session reply link; it opens in Sailry in either mode.
    Session(SessionId),
    /// Knowledge citations, workspace and local files resolved by the caller.
    Resource,
}

/// Route a normalized link. Mail and telephone links have no internal handler.
pub(crate) fn route(link: &str, mode: Mode) -> Route {
    if let Some(session) = sailry_client::conversation::links::session(link) {
        return Route::Session(session);
    }
    let Ok(url) = url::Url::parse(link) else {
        return Route::Resource;
    };
    match url.scheme() {
        "http" | "https" if mode == Mode::External => Route::System,
        "http" | "https" => Route::Browser,
        "mailto" | "tel" => Route::System,
        _ => Route::Resource,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Opening {
    /// The folder in the system file manager.
    Folder,
    /// The internal document viewer.
    Document,
    /// The local file with its system default app.
    System,
    /// A path outside every worktree on another Node.
    Unavailable,
}

/// Only files on this device reach the system; remote files stay internal.
pub(crate) fn opening(target: &Target, folder: bool, local: bool, mode: Mode) -> Opening {
    match target {
        Target::Workspace { path, .. } if folder || path.is_empty() => Opening::Folder,
        Target::Workspace { .. } if local && mode == Mode::External => Opening::System,
        Target::Workspace { .. } => Opening::Document,
        Target::External(_) if local => Opening::System,
        Target::External(_) => Opening::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod normalize {
        use super::*;

        #[test]
        fn adds_https_to_bare_hosts() {
            assert_eq!(normalize("www.example.com"), "https://www.example.com");
            assert_eq!(
                normalize(" WWW.example.com/a?b "),
                "https://WWW.example.com/a?b"
            );
        }

        #[test]
        fn makes_schemed_relative_files_relative() {
            assert_eq!(normalize("file:src/main.rs:3"), "src/main.rs:3");
            assert_eq!(normalize("file:///project/a.md"), "file:///project/a.md");
            assert_eq!(normalize("file:/project/a.md"), "file:/project/a.md");
        }

        #[test]
        fn keeps_other_links() {
            for link in [
                "https://example.com",
                "mailto:a@example.com",
                "src/www.rs",
                "example.com/path",
                "sailry-knowledge://collection/chunk",
                "sailry://session/0b7c3e1a-5d2f-4c6b-9a8e-1f2d3c4b5a69",
                "/project/a.md#L3",
            ] {
                assert_eq!(normalize(link), link);
            }
        }
    }

    mod route {
        use super::*;

        #[test]
        fn opens_web_pages_by_mode() {
            let link = normalize("www.example.com");
            assert_eq!(route(&link, Mode::Internal), Route::Browser);
            assert_eq!(route(&link, Mode::External), Route::System);
        }

        #[test]
        fn sends_mail_to_the_system() {
            for mode in [Mode::Internal, Mode::External] {
                assert_eq!(route("mailto:a@example.com", mode), Route::System);
                assert_eq!(route("tel:+1555", mode), Route::System);
            }
        }

        #[test]
        fn keeps_session_links_in_sailry() {
            let session = SessionId::new();
            let link = sailry_client::conversation::links::link(session);
            let query = format!("{link}?turn=1");
            for mode in [Mode::Internal, Mode::External] {
                assert_eq!(route(&link, mode), Route::Session(session));
                assert_eq!(
                    route(&normalize(&format!(" {} ", link.to_uppercase())), mode),
                    Route::Session(session)
                );
                for other in [
                    "sailry://return",
                    "sailry://session/not-a-session",
                    query.as_str(),
                ] {
                    assert_eq!(route(other, mode), Route::Resource, "{other}");
                }
            }
        }

        #[test]
        fn leaves_files_and_citations_to_resolution() {
            for link in [
                "src/main.rs:3",
                "/project/a.md",
                "C:/project/a.md",
                "file:///project/a.md",
                "sailry-knowledge://collection/chunk",
                "javascript:alert(1)",
            ] {
                for mode in [Mode::Internal, Mode::External] {
                    assert_eq!(route(link, mode), Route::Resource, "{link}");
                }
            }
        }
    }

    mod opening {
        use super::*;

        fn file() -> Target {
            Target::Workspace {
                path: "src/main.rs".into(),
                line: Some(3),
            }
        }

        #[test]
        fn opens_local_files_in_the_system_externally() {
            assert_eq!(
                opening(&file(), false, true, Mode::External),
                Opening::System
            );
            assert_eq!(
                opening(&file(), false, true, Mode::Internal),
                Opening::Document
            );
        }

        #[test]
        fn keeps_remote_files_internal() {
            for mode in [Mode::Internal, Mode::External] {
                assert_eq!(opening(&file(), false, false, mode), Opening::Document);
                let outside = Target::External("/outside.md".into());
                assert_eq!(opening(&outside, false, false, mode), Opening::Unavailable);
                assert_eq!(opening(&outside, false, true, mode), Opening::System);
            }
        }

        #[test]
        fn opens_folders_in_the_file_manager() {
            let root = Target::Workspace {
                path: String::new(),
                line: None,
            };
            for mode in [Mode::Internal, Mode::External] {
                assert_eq!(opening(&root, false, true, mode), Opening::Folder);
                assert_eq!(opening(&file(), true, false, mode), Opening::Folder);
            }
        }
    }
}
