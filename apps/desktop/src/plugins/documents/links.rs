//! Markdown links open workspace documents through this controller; images
//! choose files with the shared execution-host picker.
use super::*;
use crate::content::{
    editor::{OnChoose, OnImage},
    images::{self, ImageSource, Images},
    markdown::render::OnLink,
};
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

pub(super) fn chooser(client: Arc<Client>, base: String, root: Option<String>) -> OnChoose {
    Rc::new(
        move |done: Rc<dyn Fn(String, &mut Window, &mut App)>,
              window: &mut Window,
              cx: &mut App| {
            let base = base.clone();
            let root = root.clone();
            let _ = crate::file_picker::open(
                Options {
                    client: Some(client.clone()),
                    preview_host: 0,
                    initial: root.clone(),
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
                    let reference =
                        portable(&base, &path, root.as_deref()).ok_or("files_remote_path")?;
                    done(reference, window, cx);
                    Ok(Completion::Close)
                },
                window,
                cx,
            );
        },
    )
}

pub(super) fn images(
    images: Entity<Images>,
    base: String,
    root: Option<String>,
    context: plugin::Context,
) -> OnImage {
    Rc::new(move |url, urls, position, preview, cx| {
        if let Some(source) = source_for(&base, url, root.as_deref(), &context) {
            let mut index = 0;
            let gallery = urls
                .iter()
                .enumerate()
                .filter_map(|(at, url)| {
                    let source = source_for(&base, url, root.as_deref(), &context)?;
                    if at < position {
                        index += 1;
                    }
                    Some(source)
                })
                .collect();
            return images::card(
                &source,
                &images,
                gallery,
                index,
                urls.len() > 1,
                preview,
                cx,
            );
        }
        if url.starts_with("https://")
            || url.starts_with("http://")
            || url.starts_with("data:image/")
        {
            return img(SharedString::from(url.to_owned()))
                .max_w_full()
                .into_any_element();
        }
        // The renderer draws the empty-address placeholder itself.
        div()
            .child(SharedString::from(url.to_owned()))
            .into_any_element()
    })
}

fn source_for(
    base: &str,
    url: &str,
    root: Option<&str>,
    context: &plugin::Context,
) -> Option<ImageSource> {
    let path = image_path(base, url, root)?;
    let worktree = context.worktree?;
    images::displayable(&path).then(|| ImageSource::File {
        worktree,
        path,
        context: Some(context.clone()),
    })
}

fn image_path(base: &str, url: &str, root: Option<&str>) -> Option<String> {
    use crate::conversation::file_reference::{self, Target};
    if url.contains("://") || url.starts_with('/') || url.as_bytes().get(1) == Some(&b':') {
        if let Some(root) = root
            && let Some(Target::Workspace { path, .. }) = file_reference::resolve(url, root)
        {
            return Some(path);
        }
        // Leading slashes in Markdown also support workspace-root references.
        if url.starts_with('/') && !url.starts_with("//") {
            return resolve(base, url);
        }
        return None;
    }
    resolve(base, url)
}

fn portable(base: &str, path: &str, root: Option<&str>) -> Option<String> {
    use crate::conversation::file_reference::{self, Target};
    let Target::Workspace { path, .. } = file_reference::resolve(path, root?)? else {
        return None;
    };
    let mut parent = base.split('/').collect::<Vec<_>>();
    parent.pop();
    let target = path.split('/').collect::<Vec<_>>();
    let shared = parent
        .iter()
        .zip(&target)
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts = vec![".."; parent.len() - shared];
    parts.extend_from_slice(&target[shared..]);
    Some(parts.join("/"))
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
    use super::{image_path, portable, resolve};

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
    fn resolves_images_on_execution_host() {
        for (url, expected) in [
            ("../images/a.png", "images/a.png"),
            ("/remote/project/images/a.png", "images/a.png"),
            ("file:///remote/project/images/a.png", "images/a.png"),
            ("/images/a.png", "images/a.png"),
        ] {
            assert_eq!(
                image_path("docs/a.md", url, Some("/remote/project")).as_deref(),
                Some(expected)
            );
        }
        assert_eq!(
            image_path(
                "docs/a.md",
                "https://example.test/a.png",
                Some("/remote/project")
            ),
            None
        );
        assert_eq!(
            image_path("a.md", "../../private.png", Some("/remote/project")),
            None
        );
    }

    #[test]
    fn chooser_keeps_portable_references() {
        for (base, selected, root, expected) in [
            (
                "docs/a.md",
                "/remote/project/images/a.png",
                "/remote/project",
                "../images/a.png",
            ),
            (
                "docs/a.md",
                "/remote/project/docs/a.png",
                "/remote/project",
                "a.png",
            ),
            (
                "a.md",
                "C:/project/images/a.png",
                "C:/project",
                "images/a.png",
            ),
        ] {
            let reference = portable(base, selected, Some(root)).unwrap();
            assert_eq!(reference, expected);
            assert_eq!(
                image_path(base, &reference, Some(root)),
                image_path(base, selected, Some(root))
            );
        }
        assert_eq!(
            portable("a.md", "/other/private.png", Some("/remote/project")),
            None
        );
    }

    #[test]
    fn rejects_paths_outside_workspace() {
        assert_eq!(resolve("a.md", "../b.md"), None);
        assert_eq!(resolve("a.md", "#section"), None);
    }
}
