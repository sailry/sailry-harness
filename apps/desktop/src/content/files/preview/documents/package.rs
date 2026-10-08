//! Bound renderer-created cells and chart arrays after ZIP expansion is checked.
use super::{PPTX, XLSX};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
};

const LIMIT: u64 = 50_000;
const COMPARISONS: u64 = 5_000_000;
const ROWS: u64 = 1_048_576;
const COLUMNS: u64 = 16_384;
const FAILED: &str = "artifact_preview_failed";
const LARGE: &str = "artifact_preview_large";
type Archive<'a> = zip::ZipArchive<Cursor<&'a [u8]>>;

pub(super) fn validate(mime: &str, bytes: &[u8]) -> Result<(), &'static str> {
    let (content_type, relation, directory) = match mime {
        XLSX => (
            "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml",
            "/worksheet",
            "xl/worksheets/",
        ),
        PPTX => (
            "application/vnd.openxmlformats-officedocument.drawingml.chart+xml",
            "/chart",
            "ppt/charts/",
        ),
        _ => return Ok(()),
    };
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| FAILED)?;
    let mut names = BTreeMap::new();
    for name in archive.file_names().filter(|name| !name.ends_with('/')) {
        let normalized = target("", name)?;
        if names.insert(normalized, name.to_owned()).is_some() {
            return Err(FAILED);
        }
    }
    let parts = parts(&mut archive, &names, content_type, relation, directory)?;
    let mut shown = 0;
    let mut links = 0;
    let mut points = 0;
    let mut domains = 0;
    let mut comparisons = 0;
    for name in parts {
        let text = read(&mut archive, &names, &name)?;
        let document = roxmltree::Document::parse(&text).map_err(|_| FAILED)?;
        aliases(&document)?;
        if mime == XLSX {
            add(
                &mut shown,
                worksheet(&document, &mut links, &mut comparisons)?,
            )?;
        } else {
            chart(&document, &mut points, &mut domains)?;
        }
    }
    Ok(())
}

fn read(
    archive: &mut Archive<'_>,
    names: &BTreeMap<String, String>,
    name: &str,
) -> Result<String, &'static str> {
    let source = names.get(name).ok_or(FAILED)?;
    let mut file = archive.by_name(source).map_err(|_| FAILED)?;
    let mut text = String::new();
    file.read_to_string(&mut text).map_err(|_| FAILED)?;
    Ok(text)
}

fn parts(
    archive: &mut Archive<'_>,
    names: &BTreeMap<String, String>,
    content_type: &str,
    relation: &str,
    directory: &str,
) -> Result<BTreeSet<String>, &'static str> {
    let mut parts: BTreeSet<_> = names
        .keys()
        .filter(|name| name.starts_with(directory) && name.ends_with(".xml"))
        .cloned()
        .collect();
    let text = read(archive, names, "[Content_Types].xml")?;
    let types = roxmltree::Document::parse(&text).map_err(|_| FAILED)?;
    aliases(&types)?;
    for node in types.descendants().filter(|node| node.is_element()) {
        if node.attribute("ContentType") != Some(content_type) {
            continue;
        }
        match node.tag_name().name() {
            "Override" => {
                parts.insert(target("", node.attribute("PartName").ok_or(FAILED)?)?);
            }
            "Default" => {
                let extension = node.attribute("Extension").ok_or(FAILED)?;
                parts.extend(
                    names
                        .keys()
                        .filter(|name| {
                            name.rsplit_once('.')
                                .is_some_and(|(_, ext)| ext == extension)
                        })
                        .cloned(),
                );
            }
            _ => {}
        }
    }
    for name in names.keys().filter(|name| name.ends_with(".rels")) {
        let base = if let Some((base, _)) = name.rsplit_once("/_rels/") {
            base
        } else if name.starts_with("_rels/") {
            ""
        } else {
            continue;
        };
        let text = read(archive, names, name)?;
        let relationships = roxmltree::Document::parse(&text).map_err(|_| FAILED)?;
        aliases(&relationships)?;
        for node in relationships.descendants().filter(|node| {
            node.tag_name().name() == "Relationship"
                && node
                    .attribute("Type")
                    .is_some_and(|kind| kind.ends_with(relation))
        }) {
            let relative = node.attribute("Target").ok_or(FAILED)?;
            if relative.contains(':') || relative.starts_with("//") {
                continue;
            }
            // The browser libraries can read a package target even when a
            // relationship incorrectly labels it as external.
            parts.insert(target(base, relative)?);
        }
    }
    Ok(parts)
}

fn aliases(document: &roxmltree::Document<'_>) -> Result<(), &'static str> {
    // SheetJS strips namespace prefixes and lowercases attribute aliases.
    for node in document.descendants().filter(|node| node.is_element()) {
        let names: &[&str] = match node.tag_name().name() {
            "Override" => &["ContentType", "PartName"],
            "Default" => &["ContentType", "Extension"],
            "Relationship" => &["Type", "Target"],
            "dimension" | "hyperlink" | "mergeCell" => &["ref"],
            "row" | "c" => &["r"],
            "pt" => &["idx"],
            "ptCount" => &["val"],
            _ => continue,
        };
        if node.attributes().any(|attribute| {
            names.iter().any(|name| {
                attribute.name().eq_ignore_ascii_case(name)
                    && (attribute.namespace().is_some() || attribute.name() != *name)
            })
        }) {
            return Err(FAILED);
        }
    }
    Ok(())
}

fn target(base: &str, relative: &str) -> Result<String, &'static str> {
    if relative.contains(['\\', ':', '\0']) {
        return Err(FAILED);
    }
    let mut parts = if relative.starts_with('/') {
        Vec::new()
    } else {
        base.split('/').filter(|part| !part.is_empty()).collect()
    };
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop().ok_or(FAILED)?;
            }
            part => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

#[derive(Clone, Copy)]
struct Range {
    first: (u64, u64),
    last: (u64, u64),
}

impl Range {
    fn parse(value: &str) -> Result<Self, &'static str> {
        let mut parts = value.split(':');
        let first = coordinate(parts.next().ok_or(FAILED)?)?;
        let last = parts.next().map(coordinate).transpose()?.unwrap_or(first);
        if parts.next().is_some() || first.0 > last.0 || first.1 > last.1 {
            return Err(FAILED);
        }
        Ok(Self { first, last })
    }

    fn include(&mut self, other: Self) {
        self.first.0 = self.first.0.min(other.first.0);
        self.first.1 = self.first.1.min(other.first.1);
        self.last.0 = self.last.0.max(other.last.0);
        self.last.1 = self.last.1.max(other.last.1);
    }

    fn area(self) -> Result<u64, &'static str> {
        (self.last.0 - self.first.0 + 1)
            .checked_mul(self.last.1 - self.first.1 + 1)
            .filter(|area| *area <= LIMIT)
            .ok_or(LARGE)
    }
}

fn coordinate(value: &str) -> Result<(u64, u64), &'static str> {
    let value = value.strip_prefix('$').unwrap_or(value);
    let letters = value.bytes().take_while(u8::is_ascii_uppercase).count();
    if letters == 0 {
        return Err(FAILED);
    }
    let mut column = 0_u64;
    for letter in value[..letters].bytes() {
        column = column
            .checked_mul(26)
            .and_then(|column| column.checked_add(u64::from(letter - b'A' + 1)))
            .ok_or(LARGE)?;
    }
    let row = value[letters..]
        .strip_prefix('$')
        .unwrap_or(&value[letters..]);
    let row = row.parse::<u64>().map_err(|_| FAILED)?;
    if row == 0 {
        return Err(FAILED);
    }
    if row > ROWS || column > COLUMNS {
        return Err(LARGE);
    }
    Ok((row, column))
}

fn add(total: &mut u64, value: u64) -> Result<(), &'static str> {
    *total = total
        .checked_add(value)
        .filter(|total| *total <= LIMIT)
        .ok_or(LARGE)?;
    Ok(())
}

fn worksheet(
    document: &roxmltree::Document<'_>,
    links: &mut u64,
    comparisons: &mut u64,
) -> Result<u64, &'static str> {
    let mut bounds: Option<Range> = None;
    let mut rows: Option<(u64, u64)> = None;
    let mut row = 0_u64;
    let mut column = 0_u64;
    let mut cells = 0;
    let mut merges = 0_u64;
    for node in document.descendants().filter(|node| node.is_element()) {
        let range = match node.tag_name().name() {
            "dimension" => Some(Range::parse(node.attribute("ref").ok_or(FAILED)?)?),
            "row" => {
                row = match node.attribute("r") {
                    Some(value) => value.parse().map_err(|_| FAILED)?,
                    None => row.checked_add(1).ok_or(LARGE)?,
                };
                if row == 0 {
                    return Err(FAILED);
                }
                if row > ROWS {
                    return Err(LARGE);
                }
                rows =
                    Some(rows.map_or((row, row), |(first, last)| (first.min(row), last.max(row))));
                column = 0;
                None
            }
            "c" => {
                add(&mut cells, 1)?;
                let point = match node.attribute("r") {
                    Some(value) => coordinate(value)?,
                    None => {
                        if row == 0 {
                            return Err(FAILED);
                        }
                        let column = column.checked_add(1).ok_or(LARGE)?;
                        if column > COLUMNS {
                            return Err(LARGE);
                        }
                        (row, column)
                    }
                };
                column = point.1;
                Some(Range {
                    first: point,
                    last: point,
                })
            }
            "hyperlink" => {
                add(
                    links,
                    Range::parse(node.attribute("ref").ok_or(FAILED)?)?.area()?,
                )?;
                None
            }
            "mergeCell" => {
                merges = merges.checked_add(1).ok_or(LARGE)?;
                None
            }
            _ => None,
        };
        if let Some(range) = range {
            range.area()?;
            if let Some(bounds) = &mut bounds {
                bounds.include(range);
                bounds.area()?;
            } else {
                bounds = Some(range);
            }
        }
    }
    let shown = if let Some(mut bounds) = bounds {
        if let Some((first, last)) = rows {
            bounds.first.0 = bounds.first.0.min(first);
            bounds.last.0 = bounds.last.0.max(last);
        }
        bounds.area()?
    } else {
        0
    };
    // SheetJS compares each displayed cell against the complete merge list.
    *comparisons = comparisons
        .checked_add(shown.checked_mul(merges).ok_or(LARGE)?)
        .filter(|total| *total <= COMPARISONS)
        .ok_or(LARGE)?;
    Ok(shown)
}

fn chart(
    document: &roxmltree::Document<'_>,
    points: &mut u64,
    domains: &mut u64,
) -> Result<(), &'static str> {
    for node in document.descendants().filter(|node| node.is_element()) {
        match node.tag_name().name() {
            "pt" => {
                add(points, 1)?;
                if let Some(value) = node.attribute("idx") {
                    let index = value.parse::<u64>().map_err(|_| FAILED)?;
                    if index >= LIMIT {
                        return Err(LARGE);
                    }
                }
            }
            "ptCount" => {
                let count = node
                    .attribute("val")
                    .ok_or(FAILED)?
                    .parse::<u64>()
                    .map_err(|_| FAILED)?;
                if count > LIMIT {
                    return Err(LARGE);
                }
            }
            _ => {}
        }
    }
    for node in document
        .descendants()
        .filter(|node| node.is_element())
        .filter_map(|node| match node.tag_name().name() {
            "numCache" | "strCache" => Some(node),
            // The renderer uses only the first descendant category level.
            "multiLvlStrCache" => node
                .descendants()
                .find(|child| child.tag_name().name() == "lvl"),
            _ => None,
        })
    {
        add(domains, extent(node)?)?;
    }
    Ok(())
}

fn extent(cache: roxmltree::Node<'_, '_>) -> Result<u64, &'static str> {
    let mut extent = 0;
    for (fallback, node) in cache
        .descendants()
        .filter(|node| node.tag_name().name() == "pt")
        .enumerate()
    {
        let index = match node.attribute("idx") {
            Some(value) => value.parse::<u64>().map_err(|_| FAILED)?,
            None => u64::try_from(fallback).map_err(|_| LARGE)?,
        };
        extent = extent.max(index.checked_add(1).ok_or(LARGE)?);
    }
    Ok(extent)
}

#[cfg(test)]
#[path = "package/tests.rs"]
mod tests;
