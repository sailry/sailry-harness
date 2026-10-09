use super::*;
use umya_spreadsheet as xlsx;

pub(super) fn parts(bytes: &[u8]) -> Result<Vec<Part>, Fault> {
    package::open(bytes)?;
    let book = xlsx::reader::xlsx::read_reader(Cursor::new(bytes), true).map_err(invalid)?;
    Ok(book
        .sheet_collection()
        .iter()
        .map(|sheet| {
            let mut text = String::new();
            let mut bounds = None::<(u32, u32, u32, u32)>;
            for cell in sheet.cells_sorted() {
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
            Part {
                name: sheet.name().to_owned(),
                text,
                position: Position::Cells {
                    sheet: sheet.name().to_owned(),
                    start: xlsx::helper::coordinate::coordinate_from_index(left, top),
                    end: xlsx::helper::coordinate::coordinate_from_index(right, bottom),
                },
            }
        })
        .collect())
}
