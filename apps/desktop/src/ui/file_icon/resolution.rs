use crate::assets::file_types::catalog;

pub(super) fn resolve(reference: &str, mime: &str) -> String {
    let name = reference_type(reference);
    if name == "folder" {
        return name;
    }
    let mime = mime
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    // Detected format metadata remains authoritative. A generic content type
    // must not turn a named Rust source or spreadsheet into a TXT icon.
    if !matches!(
        mime.as_str(),
        "" | "application/octet-stream" | "binary/octet-stream" | "text/plain"
    ) && let Some(kind) = mime_type(&mime)
    {
        return kind;
    }
    if name != "empty" {
        return known(&name);
    }
    mime_type(&mime).unwrap_or(name)
}

fn known(kind: &str) -> String {
    if catalog().types.contains_key(kind) {
        kind.to_owned()
    } else {
        "file".into()
    }
}

fn mime_type(mime: &str) -> Option<String> {
    if let Some(kind) = catalog().mime.get(mime) {
        return Some(kind.clone());
    }
    // Preserve existing Office MIME families beyond the 99 labeled formats.
    let office = if mime
        .starts_with("application/vnd.openxmlformats-officedocument.wordprocessingml.")
        || mime.starts_with("application/vnd.ms-word.")
    {
        Some("document")
    } else if mime.starts_with("application/vnd.openxmlformats-officedocument.spreadsheetml.")
        || mime.starts_with("application/vnd.ms-excel.")
    {
        Some("spreadsheet")
    } else if mime.starts_with("application/vnd.openxmlformats-officedocument.presentationml.")
        || mime.starts_with("application/vnd.ms-powerpoint.")
    {
        Some("presentation")
    } else if mime == "application/xhtml+xml" {
        Some("html")
    } else {
        None
    };
    if let Some(kind) = office {
        return Some(kind.into());
    }
    let (family, subtype) = mime.split_once('/')?;
    if !subtype.is_empty()
        && subtype
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".+-".contains(character))
    {
        if let Some((_, tail)) = subtype.rsplit_once('.')
            && let Some(entry) = catalog().types.get(tail)
            && entry.category != tail
        {
            return Some(tail.into());
        }
        if let Some(entry) = catalog().types.get(subtype)
            && (matches!(family, "application" | "text") || entry.category == family)
        {
            return Some(subtype.into());
        }
        return match family {
            "image" | "audio" | "video" | "font" => Some(family.into()),
            "text" => Some("txt".into()),
            _ => None,
        };
    }
    None
}

fn reference_type(reference: &str) -> String {
    let input = reference.trim().to_ascii_lowercase();
    let mut path = input.as_str();
    if let Some((scheme, remainder)) = path.split_once("://")
        && !scheme.is_empty()
        && scheme.starts_with(|character: char| character.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "+.-".contains(character))
    {
        let remainder = remainder.split(['?', '#']).next().unwrap_or_default();
        path = remainder
            .find('/')
            .map(|index| &remainder[index..])
            .unwrap_or_default();
        if path == "/" {
            return "empty".into();
        }
    } else if !name_end(path) {
        path = path.split(['?', '#']).next().unwrap_or_default();
    }
    if path.is_empty() {
        return "empty".into();
    }
    if path.ends_with(['/', '\\']) {
        return "folder".into();
    }
    // Bare MIME references use the same table as explicit attachment metadata.
    if let Some((family, _)) = path.split_once('/')
        && matches!(
            family,
            "application" | "audio" | "font" | "image" | "text" | "video"
        )
        && let Some(kind) = mime_type(path.split(';').next().unwrap_or_default().trim())
    {
        return kind;
    }
    let name = path.rsplit(['/', '\\']).next().unwrap_or_default();
    match name.rsplit_once('.') {
        Some(("", _)) | None => {
            let bare = name.strip_prefix('.').unwrap_or(name);
            if catalog().types.contains_key(bare) {
                bare.into()
            } else {
                "empty".into()
            }
        }
        Some((_, "")) => "empty".into(),
        Some((_, extension)) => extension.into(),
    }
}

fn name_end(path: &str) -> bool {
    if path.ends_with(['/', '\\']) {
        return true;
    }
    let Some((_, extension)) = path.rsplit_once('.') else {
        return false;
    };
    extension
        .chars()
        .all(|character| character.is_ascii_alphanumeric())
        && extension
            .chars()
            .any(|character| character.is_ascii_alphabetic())
}
