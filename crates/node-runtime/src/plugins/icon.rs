//! Catalog icons are bounded PNG resources owned by each plugin package.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sailry_protocol::plugin::{Extension, Issue, IssueKind};

const MAX_BYTES: usize = 64 * 1024;

pub(super) fn encode(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 33
        || bytes.len() > MAX_BYTES
        || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || &bytes[12..16] != b"IHDR"
    {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    (width > 0 && height > 0 && width <= 512 && height <= 512).then(|| STANDARD.encode(bytes))
}

pub(super) fn read(
    root: &cap_std::fs::Dir,
    extension: Option<&Extension>,
    issues: &mut Vec<Issue>,
) -> Option<String> {
    let path = extension?.icon.as_deref()?;
    let result = (|| {
        let parts = crate::files::path::components(path, false)?;
        let (name, parents) = parts
            .split_last()
            .ok_or_else(|| sailry_protocol::Fault::invalid("empty icon path"))?;
        let parent = crate::files::path::descend(root.try_clone().map_err(io_error)?, parents)?;
        let bytes = package::read(&parent, name, MAX_BYTES)?;
        Ok::<_, Fault>(bytes.and_then(|bytes| encode(&bytes)))
    })();
    match result {
        Ok(Some(icon)) => Some(icon),
        _ => {
            issues.push(Issue {
                path: path.into(),
                kind: IssueKind::UnavailablePath,
            });
            None
        }
    }
}

/// Reuse the package image boundary for channel logos; SVG remains passive vector content.
pub(crate) fn uri(bytes: &[u8]) -> Option<String> {
    if let Some(png) = encode(bytes) {
        return Some(format!("data:image/png;base64,{png}"));
    }
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let document = roxmltree::Document::parse(text).ok()?;
    if document.root_element().tag_name().name() != "svg" {
        return None;
    }
    let local_url = regex::Regex::new(r"url\(#[A-Za-z0-9_-]+\)").ok()?;
    for node in document.descendants().filter(|node| node.is_element()) {
        if matches!(
            node.tag_name().name(),
            "script" | "foreignObject" | "image" | "style"
        ) {
            return None;
        }
        for attribute in node.attributes() {
            let name = attribute.name().to_ascii_lowercase();
            let value = attribute.value().trim();
            let safe = local_url.replace_all(value, "").to_ascii_lowercase();
            if name.starts_with("on")
                || (name == "href" && !value.starts_with('#'))
                || safe.contains("url(")
                || safe.contains("@import")
            {
                return None;
            }
        }
    }
    Some(format!(
        "data:image/svg+xml;base64,{}",
        STANDARD.encode(bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_declared_package_icon() {
        let (directory, host, source) = super::super::tests::fixture();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(source.join("plugin.json")).unwrap()).unwrap();
        manifest["extensions"] = serde_json::json!({"dev.sailry.platform": {
            "api_version":"v1", "actions":[], "icon":"dev.sailry.platform/assets/logo.png"
        }});
        std::fs::write(
            source.join("plugin.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        std::fs::create_dir(source.join("dev.sailry.platform/assets")).unwrap();
        let bytes = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==").unwrap();
        std::fs::write(source.join("dev.sailry.platform/assets/logo.png"), &bytes).unwrap();
        let info = host.install(&source, "", "example").unwrap();
        assert_eq!(info.icon, encode(&bytes));
        std::fs::write(
            source.join("dev.sailry.platform/assets/logo.png"),
            b"not a PNG",
        )
        .unwrap();
        let info = host.install(&source, "", "example").unwrap();
        assert!(info.icon.is_none());
        assert!(
            info.issues
                .iter()
                .any(|issue| issue.path == "dev.sailry.platform/assets/logo.png")
        );
        drop(directory);
    }
}
