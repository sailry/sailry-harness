use super::*;
use crate::conversation::file_reference::{self, Target};

pub(crate) fn from_link(url: &str, label: &str, root: &str) -> Option<File> {
    // Section and line references retain ordinary document/source navigation.
    if url.contains(['#', '?']) {
        return None;
    }
    let Target::Workspace { path, line: None } = file_reference::resolve(url, root)? else {
        return None;
    };
    if std::path::Path::new(&path).extension().is_none() || crate::content::images::supports(&path)
    {
        return None;
    }
    let mime = mime_guess::from_path(&path)
        .first_or_octet_stream()
        .essence_str()
        .to_owned();
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
            "dir",
            "photo.png",
        ] {
            assert!(from_link(url, "File", "/work").is_none(), "{url}");
        }
    }
}
