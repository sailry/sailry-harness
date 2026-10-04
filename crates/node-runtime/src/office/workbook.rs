use super::*;
use umya_spreadsheet as xlsx;

pub(super) fn read(bytes: &[u8]) -> Result<Vec<(String, String)>, Fault> {
    package::open(bytes)?;
    let book = xlsx::reader::xlsx::read_reader(Cursor::new(bytes), true).map_err(invalid)?;
    Ok(book
        .sheet_collection()
        .iter()
        .map(|sheet| {
            let mut text = String::new();
            for cell in sheet.cells_sorted() {
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
            (sheet.name().to_owned(), text)
        })
        .collect())
}
