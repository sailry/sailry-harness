use super::*;
use std::{io::Write as _, ops::Range, panic::catch_unwind};
use umya_spreadsheet as xlsx;

pub(super) fn parts(bytes: &[u8]) -> Result<Vec<Part>, Fault> {
    let bytes = reader(package::open(bytes)?)?;
    catch_unwind(|| read(&bytes)).unwrap_or_else(|reason| {
        let reason = reason
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| reason.downcast_ref::<&str>().copied())
            .unwrap_or("unknown parser panic");
        Err(invalid(format!("workbook parser failed: {reason}")))
    })
}

fn read(bytes: &[u8]) -> Result<Vec<Part>, Fault> {
    // umya 3.1's cell reader preserves values and formulas without entering the
    // drawing/chart deserializer, which is irrelevant to document inspection.
    let book = xlsx::reader::xlsx::read_reader(Cursor::new(bytes), false).map_err(invalid)?;
    book.sheet_collection_no_check()
        .iter()
        .enumerate()
        .map(|(index, sheet)| {
            let cells = book.lazy_read_sheet_cells(index).map_err(invalid)?;
            let mut text = String::new();
            let mut bounds = None::<(u32, u32, u32, u32)>;
            for cell in cells.iter_cells_sorted_by_row_column() {
                let coordinate = cell.coordinate();
                let column = coordinate.col_num();
                let row = coordinate.row_num();
                bounds = Some(match bounds {
                    Some((left, top, right, bottom)) => (
                        left.min(column),
                        top.min(row),
                        right.max(column),
                        bottom.max(row),
                    ),
                    None => (column, row, column, row),
                });
                let formula = if cell.is_formula() {
                    format!(" [={}]", cell.formula())
                } else {
                    String::new()
                };
                text.push_str(&format!(
                    "{}: {}{}\n",
                    cell.coordinate(),
                    cell.value(),
                    formula
                ));
            }
            let (left, top, right, bottom) = bounds.unwrap_or((1, 1, 1, 1));
            Ok(Part {
                name: sheet.name().to_owned(),
                text,
                position: Position::Cells {
                    sheet: sheet.name().to_owned(),
                    start: xlsx::helper::coordinate::coordinate_from_index(left, top),
                    end: xlsx::helper::coordinate::coordinate_from_index(right, bottom),
                },
            })
        })
        .collect()
}

fn reader(mut package: package::Package) -> Result<Vec<u8>, Fault> {
    let relationships =
        roxmltree::Document::parse(package::xml(&package, "xl/_rels/workbook.xml.rels")?)
            .map_err(invalid)?;
    let worksheets = relationships
        .descendants()
        .filter(|node| {
            node.tag_name().name() == "Relationship"
                && node
                    .attribute("Type")
                    .is_some_and(|kind| kind.ends_with("/worksheet"))
        })
        .map(|node| {
            package::target(
                "xl",
                node.attribute("Target")
                    .ok_or_else(|| invalid("workbook worksheet target is missing"))?,
            )
        })
        .collect::<Result<Vec<_>, Fault>>()?;
    for name in worksheets {
        let text = package::xml(&package, &name)?;
        let normalized = normalize(text)?;
        package.insert(name, normalized.into_bytes());
    }
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in package {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .map_err(invalid)?;
        writer.write_all(&bytes).map_err(invalid)?;
    }
    Ok(writer.finish().map_err(invalid)?.into_inner())
}

fn normalize(text: &str) -> Result<String, Fault> {
    const NAMESPACE: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    let document = roxmltree::Document::parse(text).map_err(invalid)?;
    let mut prefixes = Vec::new();
    // umya 3.1 matches literal element names. Normalize only SpreadsheetML
    // element prefixes in the private reader copy; retain attributes and text.
    for node in document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().namespace() == Some(NAMESPACE))
    {
        let range = node.range();
        if let Some(opening) = prefix(text, range.start + 1) {
            prefixes.push(opening);
            if let Some(end) = text[range.clone()].rfind("</") {
                if let Some(prefix) = prefix(text, range.start + end + 2) {
                    prefixes.push(prefix);
                }
            }
        }
    }
    prefixes.sort_by_key(|range| range.start);
    let mut normalized = String::with_capacity(text.len());
    let mut offset = 0;
    for range in prefixes {
        normalized.push_str(&text[offset..range.start]);
        offset = range.end;
    }
    normalized.push_str(&text[offset..]);
    Ok(normalized)
}

fn prefix(text: &str, start: usize) -> Option<Range<usize>> {
    let name = text[start..]
        .split([' ', '\t', '\r', '\n', '/', '>'])
        .next()?;
    name.find(':').map(|length| start..start + length + 1)
}
