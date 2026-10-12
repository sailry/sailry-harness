use super::*;
use crate::conversation::file_reference::{self, Target};

pub(crate) fn from_link(url: &str, label: &str, root: &str) -> Option<File> {
    link(url, label, root, false)
}

/// A file card for an image reference that cannot be displayed inline.
pub(crate) fn from_image(url: &str, label: &str, root: &str) -> Option<File> {
    link(url, label, root, true)
}

fn link(url: &str, label: &str, root: &str, image: bool) -> Option<File> {
    // Section and line references retain ordinary document/source navigation.
    if url.contains(['#', '?']) {
        return None;
    }
    let Target::Workspace { path, line: None } = file_reference::resolve(url, root)? else {
        return None;
    };
    if !image && crate::content::images::supports(&path) {
        return None;
    }
    let mime = mime(&path);
    let file = File {
        path,
        name: (!label.trim().is_empty()).then(|| label.to_owned()),
        mime,
        size: None,
        revision: None,
    };
    file.valid().then_some(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn resolves_only_worktree_outputs() {
        assert_eq!(
            from_link("report%20final.pdf", "Report", "/work")
                .unwrap()
                .path,
            "report final.pdf"
        );
        assert_eq!(
            from_link("preview.html", "Preview", "/work").unwrap().mime,
            "text/html"
        );
        for url in [
            "../private.html",
            "/outside/a.pdf",
            "https://example.test/a.html",
            "file:///outside/a.html",
            "src/main.rs#L2",
            "guide.md#section",
            "photo.png",
        ] {
            assert!(from_link(url, "File", "/work").is_none(), "{url}");
        }
        for path in ["HANDOFF.md", "report.MD", "report.markdown", "report.mdown"] {
            assert_eq!(
                from_link(path, "Report", "/work").unwrap().mime,
                "text/markdown"
            );
        }
        for path in [
            "README",
            "notes.txt",
            "data.json",
            "stats.py",
            "verify.cjs",
            "execution.log",
            "bundle.zip",
        ] {
            assert_eq!(from_link(path, "Output", "/work").unwrap().path, path);
        }
    }
}
