//! Projects verified registry metadata into the standard skill format.
use super::*;
use sailry_protocol::plugin::skills::Document;

pub(in crate::plugins) fn normalize(
    bytes: &[u8],
    source: &RegistryResolved,
) -> Result<String, Fault> {
    use super::super::skills;
    if bytes.len() > skills::MAX_BYTES || !skills::valid_name(&source.slug) {
        return Err(invalid());
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| Fault::invalid("SKILL.md must be UTF-8 text"))?;
    let mut value = skills::header(text)?;
    let object = value.as_object_mut().ok_or_else(invalid)?;
    let title = object
        .get("name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(invalid)?
        .to_owned();
    if skills::valid_name(&title) && title != source.slug {
        return Err(Fault::invalid("registry skill identity does not match"));
    }
    if title.trim().is_empty() || title.len() > 256 || title.chars().any(char::is_control) {
        return Err(invalid());
    }
    if let Some(slug) = object.get("slug")
        && slug.as_str() != Some(source.slug.as_str())
    {
        return Err(Fault::invalid("registry skill identity does not match"));
    }
    object.insert("name".into(), source.slug.clone().into());
    let metadata = object
        .entry("metadata")
        .or_insert_with(|| serde_json::json!({}));
    let metadata = metadata.as_object_mut().ok_or_else(invalid)?;
    // Preserve vendor structures as strings without widening the standard Skill schema.
    for (key, value) in metadata.iter_mut() {
        if !value.is_string() {
            if key == "display-name" || key.starts_with("display-name.") {
                return Err(invalid());
            }
            *value = serde_json::to_string(value).map_err(|_| invalid())?.into();
        }
    }
    if !skills::valid_name(&title) {
        metadata
            .entry("display-name")
            .or_insert_with(|| title.into());
    }
    let header = serde_json::to_string(&value).map_err(|_| invalid())?;
    let projected = format!("---\n{header}\n---\n{}", skills::body(text));
    if projected.len() > skills::MAX_BYTES {
        return Err(Fault::invalid("skill document exceeds its resource budget"));
    }
    skills::parse_document(&projected)?;
    Ok(projected)
}

pub(super) fn parse(bytes: &[u8], source: &RegistryResolved) -> Result<Document, Fault> {
    super::super::skill_sources::document::parse(normalize(bytes, source)?.as_bytes())
}

pub(super) fn archive(
    file: File,
    source: &RegistryResolved,
    stop: &CancellationToken,
) -> Result<Document, Fault> {
    let (temporary, root, links) = super::super::archive::unpack_source(file, stop, 10_000)?;
    if !links.is_empty() {
        return Err(invalid());
    }
    let root = crate::files::path::root(&root)?;
    let bytes = super::super::package::read(&root, "SKILL.md", super::super::skills::MAX_BYTES)?
        .ok_or_else(invalid)?;
    let document = parse(&bytes, source)?;
    temporary.close().map_err(io_error)?;
    Ok(document)
}
