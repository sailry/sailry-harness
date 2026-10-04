//! Bounded OPC inspection leaves the source document unchanged.
use super::*;
use std::collections::BTreeMap;
pub(super) type Package = BTreeMap<String, Vec<u8>>;

pub(super) fn open(bytes: &[u8]) -> Result<Package, Fault> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(invalid)?;
    if zip.len() > 4096 {
        return Err(invalid("archive has too many parts"));
    }
    let mut result = Package::new();
    let mut total = 0_u64;
    for index in 0..zip.len() {
        let file = zip.by_index(index).map_err(invalid)?;
        total = total.saturating_add(file.size());
        if total > 128 * 1024 * 1024 || file.size() > 32 * 1024 * 1024 {
            return Err(invalid("expanded document exceeds the size limit"));
        }
        let name = file.name().to_owned();
        let mut content = Vec::new();
        file.take(32 * 1024 * 1024 + 1)
            .read_to_end(&mut content)
            .map_err(invalid)?;
        if content.len() > 32 * 1024 * 1024 {
            return Err(invalid("document part is too large"));
        }
        if result.insert(name, content).is_some() {
            return Err(invalid("duplicate archive part"));
        }
    }
    Ok(result)
}

pub(super) fn xml<'a>(package: &'a Package, name: &str) -> Result<&'a str, Fault> {
    std::str::from_utf8(
        package
            .get(name)
            .ok_or_else(|| invalid(format!("missing document part: {name}")))?,
    )
    .map_err(invalid)
}

pub(super) fn target(base: &str, relative: &str) -> Result<String, Fault> {
    if relative.contains(['\\', ':', '\0']) {
        return Err(invalid("invalid document relationship"));
    }
    let mut parts: Vec<_> = if relative.starts_with('/') {
        Vec::new()
    } else {
        base.split('/').collect()
    };
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(invalid("document relationship escapes its package"));
                }
            }
            value => parts.push(value),
        }
    }
    Ok(parts.join("/"))
}

fn ordered(package: &Package, ext: &str) -> Result<Vec<String>, Fault> {
    if ext == "docx" {
        return Ok(package
            .keys()
            .filter(|name| part(name, ext))
            .cloned()
            .collect());
    }
    let document =
        roxmltree::Document::parse(xml(package, "ppt/presentation.xml")?).map_err(invalid)?;
    let relations = roxmltree::Document::parse(xml(package, "ppt/_rels/presentation.xml.rels")?)
        .map_err(invalid)?;
    let mut names = Vec::new();
    for slide in document
        .descendants()
        .filter(|n| n.tag_name().name() == "sldId")
    {
        let id = slide.attribute((
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
            "id",
        ));
        let relation = relations
            .descendants()
            .find(|n| n.tag_name().name() == "Relationship" && n.attribute("Id") == id)
            .ok_or_else(|| invalid("slide relationship is missing"))?;
        let name = target(
            "ppt",
            relation
                .attribute("Target")
                .ok_or_else(|| invalid("slide target is missing"))?,
        )?;
        let (base, file) = name
            .rsplit_once('/')
            .ok_or_else(|| invalid("invalid slide part"))?;
        let rel_name = format!("{base}/_rels/{file}.rels");
        names.push(name.clone());
        if package.contains_key(&rel_name) {
            let notes = roxmltree::Document::parse(xml(package, &rel_name)?).map_err(invalid)?;
            for note in notes.descendants().filter(|n| {
                n.attribute("Type")
                    .is_some_and(|t| t.ends_with("/notesSlide"))
            }) {
                names.push(target(
                    base,
                    note.attribute("Target")
                        .ok_or_else(|| invalid("notes target is missing"))?,
                )?);
            }
        }
    }
    Ok(names)
}

fn part(name: &str, ext: &str) -> bool {
    name.ends_with(".xml")
        && if ext == "docx" {
            name == "word/document.xml"
                || name.starts_with("word/header")
                || name.starts_with("word/footer")
        } else {
            name.starts_with("ppt/slides/slide") || name.starts_with("ppt/notesSlides/notesSlide")
        }
}

fn paragraphs<'a>(
    document: &'a roxmltree::Document<'a>,
) -> impl Iterator<Item = roxmltree::Node<'a, 'a>> {
    document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "p")
}

fn runs<'a>(paragraph: roxmltree::Node<'a, 'a>) -> Vec<roxmltree::Node<'a, 'a>> {
    paragraph
        .descendants()
        .filter(|node| {
            node.is_element()
                && node.tag_name().name() == "t"
                && node
                    .ancestors()
                    .skip(1)
                    .find(|n| n.is_element() && n.tag_name().name() == "p")
                    == Some(paragraph)
        })
        .collect()
}

pub(super) fn read(bytes: &[u8], ext: &str) -> Result<Vec<(String, String)>, Fault> {
    let package = open(bytes)?;
    let mut result = Vec::new();
    for name in ordered(&package, ext)? {
        let text = xml(&package, &name)?;
        let document = roxmltree::Document::parse(text).map_err(invalid)?;
        let text = paragraphs(&document)
            .map(|p| runs(p).iter().filter_map(|n| n.text()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        result.push((name.clone(), text));
    }
    if result.is_empty() {
        return Err(invalid("document has no readable parts"));
    }
    Ok(result)
}
