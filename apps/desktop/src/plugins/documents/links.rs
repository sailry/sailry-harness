//! Markdown links open workspace documents through this controller; images
//! choose files with the shared execution-host picker.
use super::*;
use crate::content::{editor::OnChoose, markdown::render::OnLink};
use crate::file_picker::{Completion, Mode, Options, Selection};
use sailry_client::Client;
use std::rc::Rc;

pub(super) fn handler(
    owner: WeakEntity<Controller>,
    base: String,
    context: plugin::Context,
) -> OnLink {
    Rc::new(move |url: &SharedString, _: &mut Window, cx: &mut App| {
        if url.contains("://") || url.starts_with("mailto:") {
            cx.open_url(url);
            return;
        }
        let Some(path) = resolve(&base, url) else {
            return;
        };
        let context = context.clone();
        let _ = owner.update(cx, |controller, cx| {
            let stop = controller.stop.clone();
            controller.request_open(path, None, context, stop, cx);
        });
    })
}

pub(super) fn chooser(client: Arc<Client>) -> OnChoose {
    Rc::new(
        move |done: Rc<dyn Fn(String, &mut Window, &mut App)>,
              window: &mut Window,
              cx: &mut App| {
            let _ = crate::file_picker::open(
                Options {
                    client: Some(client.clone()),
                    preview_host: 0,
                    initial: None,
                    mode: Mode::Files { multiple: false },
                    title: "content_image_choose",
                    confirm: "content_image_choose",
                },
                move |selection, window, cx| {
                    let Selection::Files(paths) = selection else {
                        return Err("directory_stale");
                    };
                    let Some(path) = paths.into_iter().next() else {
                        return Err("directory_stale");
                    };
                    done(path, window, cx);
                    Ok(Completion::Close)
                },
                window,
                cx,
            );
        },
    )
}

/// A workspace path for a document link, relative to the linking document.
/// A leading slash starts at the workspace root; links cannot leave it.
fn resolve(base: &str, url: &str) -> Option<String> {
    let url = url.split(['#', '?']).next()?;
    if url.is_empty() {
        return None;
    }
    let mut parts: Vec<&str> = match url.strip_prefix('/') {
        Some(_) => Vec::new(),
        None => base.split('/').collect::<Vec<_>>(),
    };
    if url.strip_prefix('/').is_none() {
        parts.pop();
    }
    for part in url.trim_start_matches('/').split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn resolves_relative_to_document() {
        assert_eq!(
            resolve("docs/a.md", "node.md").as_deref(),
            Some("docs/node.md")
        );
        assert_eq!(
            resolve("docs/a.md", "../README.md#top").as_deref(),
            Some("README.md")
        );
        assert_eq!(
            resolve("docs/a.md", "/docs/node.md").as_deref(),
            Some("docs/node.md")
        );
        assert_eq!(resolve("a.md", "./b.md").as_deref(), Some("b.md"));
    }

    #[test]
    fn rejects_paths_outside_workspace() {
        assert_eq!(resolve("a.md", "../b.md"), None);
        assert_eq!(resolve("a.md", "#section"), None);
    }
}
