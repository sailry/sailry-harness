use super::*;
use core::prelude::v1::test;
use std::io::Write;

const WORKSHEET: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const CHART: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";

fn archive(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    if !parts.iter().any(|(name, _)| *name == "[Content_Types].xml") {
        writer
            .start_file(
                "[Content_Types].xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(b"<Types/>").unwrap();
    }
    for (name, text) in parts {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(text.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn declared(name: &str, kind: &str, text: &str) -> Vec<u8> {
    let types = format!("<Types><Override PartName='/{name}' ContentType='{kind}'/></Types>");
    archive(&[("[Content_Types].xml", &types), (name, text)])
}

fn linked(name: &str, kind: &str, text: &str) -> Vec<u8> {
    let relationships = format!(
        "<Relationships><Relationship Id='target' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}' Target='../{name}' TargetMode='External'/></Relationships>"
    );
    archive(&[("xl/_rels/workbook.xml.rels", &relationships), (name, text)])
}

fn check_sheet(text: &str) -> Result<(), &'static str> {
    validate(XLSX, &archive(&[("xl/worksheets/sheet1.xml", text)]))
}

#[test]
fn accepts_fixture_packages() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/node-runtime/src/office/fixtures");
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        let mime = match path.extension().and_then(|extension| extension.to_str()) {
            Some("xlsx") => XLSX,
            Some("pptx") => PPTX,
            _ => continue,
        };
        assert_eq!(
            validate(mime, &std::fs::read(&path).unwrap()),
            Ok(()),
            "{}",
            path.display()
        );
    }
}

mod worksheets {
    use super::*;

    #[test]
    fn cell_and_link_limits() {
        let mut cases = Vec::new();
        {
            cases.push(("accepts bounded cells and links", ("<worksheet><dimension ref='A1:B2'/><sheetData><row r='1'><c r='A1'><v>1</v></c></row></sheetData><hyperlinks><hyperlink ref='A1:B2'/></hyperlinks></worksheet>").to_owned(), Ok(())));
            cases.push((
                "accepts bounded cells and links",
                ("<worksheet><dimension ref='A1:AX1000'/></worksheet>").to_owned(),
                Ok(()),
            ));
            cases.push(("accepts bounded cells and links", ("<worksheet><dimension ref='A1:C2'/><mergeCells><mergeCell ref='A1:B1'/><mergeCell ref='A2:B2'/></mergeCells></worksheet>").to_owned(), Ok(())));
        }
        {
            cases.push((
                "rejects sparse dimensions",
                ("<worksheet><dimension ref='A1:XFD1048576'/></worksheet>").to_owned(),
                Err(LARGE),
            ));
        }
        {
            for text in [
                "<worksheet><sheetData><row r='1'><c r='A1'><v>1</v></c></row><row r='1000'><c r='AZ1000'><v>1</v></c></row></sheetData></worksheet>",
                "<worksheet><sheetData><row r='1'><c><v>1</v></c></row><row r='50001'/></sheetData></worksheet>",
            ] {
                cases.push(("bounds inferred extents", (text).to_owned(), Err(LARGE)));
            }
        }
        {
            for dimension in ["ref='A1:A1'", "ref=\"A1\"", "ref=\"A1:A1\""] {
                cases.push(("bounds blank rows with dimensions", (&format!(
                    "<worksheet><dimension {dimension}/><sheetData><row r='1'><c r='A1'><v>1</v></c></row><row r='50001'/></sheetData></worksheet>"
                )).to_owned(), Err(LARGE)));
            }
            cases.push(("bounds blank rows with dimensions", ("<worksheet><dimension ref='A1'/><sheetData><row r='50001'/></sheetData></worksheet>").to_owned(), Err(LARGE)));
        }
        {
            for reference in ["A1048577", "XFE1", "A9007199254740993"] {
                for text in [
                    format!("<worksheet><dimension ref='{reference}'/></worksheet>"),
                    format!(
                        "<worksheet><sheetData><row r='1'><c r='{reference}'><v>1</v></c></row></sheetData></worksheet>"
                    ),
                    format!(
                        "<worksheet><hyperlinks><hyperlink ref='{reference}'/></hyperlinks></worksheet>"
                    ),
                ] {
                    cases.push((
                        "bounds absolute coordinates",
                        (&text).to_owned(),
                        Err(LARGE),
                    ));
                }
            }
            cases.push(("bounds absolute coordinates", ("<worksheet><sheetData><row r='1048577'><c><v>1</v></c></row></sheetData></worksheet>").to_owned(), Err(LARGE)));
            cases.push(("bounds absolute coordinates", ("<worksheet><sheetData><row r='1048576'/><row><c><v>1</v></c></row></sheetData></worksheet>").to_owned(), Err(LARGE)));
            cases.push(("bounds absolute coordinates", ("<worksheet><sheetData><row r='1'><c r='XFD1'><v>1</v></c><c><v>2</v></c></row></sheetData></worksheet>").to_owned(), Err(LARGE)));
            cases.push((
                "bounds absolute coordinates",
                ("<worksheet><dimension ref='XFD1048576'/></worksheet>").to_owned(),
                Ok(()),
            ));
        }
        {
            for attribute in ["x:r", "R"] {
                cases.push(("rejects row attribute aliases", (&format!(
                    "<worksheet xmlns:x='urn:test'><sheetData><row r='1'><c><v>1</v></c></row><row {attribute}='50001'><c><v>2</v></c></row></sheetData></worksheet>"
                )).to_owned(), Err(FAILED)));
            }
        }
        {
            cases.push(("rejects sparse hyperlinks", ("<worksheet><dimension ref='A1'/><hyperlinks><hyperlink ref='A1:XFD1048576'/></hyperlinks></worksheet>").to_owned(), Err(LARGE)));
        }
        {
            cases.push(("sums hyperlink areas", ("<worksheet><hyperlinks><hyperlink ref='A1:Z1000'/><hyperlink ref='A1:Z1000'/></hyperlinks></worksheet>").to_owned(), Err(LARGE)));
        }
        {
            for reference in ["A0", "B2:A1", "A1:B2:C3", "1A"] {
                cases.push((
                    "rejects invalid coordinates",
                    (&format!("<worksheet><dimension ref='{reference}'/></worksheet>")).to_owned(),
                    Err(FAILED),
                ));
            }
        }
        for (index, (case, text, expected)) in cases.into_iter().enumerate() {
            assert_eq!(check_sheet(&text), expected, "{case}: row {index}");
        }
    }

    #[test]
    fn bounds_merge_comparisons() {
        for (count, result) in [(100, Ok(())), (101, Err(LARGE))] {
            let merges = "<mergeCell ref='ZZ100001'/>".repeat(count);
            assert_eq!(
                check_sheet(&format!(
                    "<worksheet><dimension ref='A1:AX1000'/><mergeCells>{merges}</mergeCells></worksheet>"
                )),
                result
            );
        }
    }

    #[test]
    fn sums_merge_comparisons() {
        let merges = "<mergeCell ref='ZZ100001'/>".repeat(101);
        let text = format!(
            "<worksheet><dimension ref='A1:Y1000'/><mergeCells>{merges}</mergeCells></worksheet>"
        );
        assert_eq!(
            validate(
                XLSX,
                &archive(&[
                    ("xl/worksheets/first.xml", &text),
                    ("xl/worksheets/second.xml", &text),
                ])
            ),
            Err(LARGE)
        );
    }

    #[test]
    fn sums_displayed_sheets() {
        let text = "<worksheet><dimension ref='A1:Z1000'/></worksheet>";
        assert_eq!(
            validate(
                XLSX,
                &archive(&[
                    ("xl/worksheets/first.xml", text),
                    ("xl/worksheets/second.xml", text)
                ])
            ),
            Err(LARGE)
        );
    }

    #[test]
    fn inspects_custom_part_types_and_relationships() {
        let text = "<worksheet><dimension ref='A1:XFD1048576'/></worksheet>";
        for bytes in [
            declared("custom/table.part", WORKSHEET, text),
            linked("custom/table.part", "worksheet", text),
        ] {
            assert_eq!(validate(XLSX, &bytes), Err(LARGE));
        }
        let types =
            format!("<Types><Default Extension='sheet' ContentType='{WORKSHEET}'/></Types>");
        assert_eq!(
            validate(
                XLSX,
                &archive(&[
                    ("[Content_Types].xml", &types),
                    ("custom/table.sheet", text)
                ])
            ),
            Err(LARGE)
        );
    }

    #[test]
    fn rejects_custom_part_selector_aliases() {
        for (attribute, value) in [
            ("x:ContentType", WORKSHEET),
            ("x:PartName", "/custom/table.part"),
            ("contenttype", WORKSHEET),
        ] {
            let types = format!(
                "<Types xmlns:x='urn:test'><Override PartName='/custom/table.part' ContentType='{WORKSHEET}' {attribute}='{value}'/></Types>"
            );
            assert_eq!(
                validate(
                    XLSX,
                    &archive(&[
                        ("[Content_Types].xml", &types),
                        (
                            "custom/table.part",
                            "<worksheet><dimension ref='A1'/></worksheet>"
                        ),
                    ])
                ),
                Err(FAILED)
            );
        }
        for attribute in ["x:Type", "type", "x:Target", "target"] {
            let value = if attribute.to_ascii_lowercase().ends_with("type") {
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet"
            } else {
                "../custom/table.part"
            };
            let relationships = format!(
                "<Relationships xmlns:x='urn:test'><Relationship Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet' Target='../custom/table.part' {attribute}='{value}'/></Relationships>"
            );
            assert_eq!(
                validate(
                    XLSX,
                    &archive(&[
                        ("xl/_rels/workbook.xml.rels", &relationships),
                        (
                            "custom/table.part",
                            "<worksheet><dimension ref='A1'/></worksheet>"
                        ),
                    ])
                ),
                Err(FAILED)
            );
        }
    }
}

mod charts {
    use super::*;

    #[test]
    fn point_limits() {
        let mut cases = Vec::new();
        {
            let text = "<chart><numCache><ptCount val='50000'/><pt idx='49999'><v>1e-20</v></pt></numCache></chart>";
            cases.push((
                "accepts bounded point indexes",
                (&declared("custom/chart.part", CHART, text)).to_owned(),
                Ok(()),
            ));
        }
        {
            let text = "<chart><plotArea><barChart><ser><val><numRef><numCache><pt idx='0'><v>1</v></pt><pt idx='1'><v>2</v></pt></numCache></numRef></val></ser><ser><val><numRef><numCache><pt idx='0'><v>3</v></pt><pt idx='1'><v>4</v></pt></numCache></numRef></val></ser></barChart></plotArea></chart>";
            cases.push((
                "accepts ordinary multi series",
                (&declared("custom/chart.part", CHART, text)).to_owned(),
                Ok(()),
            ));
        }
        {
            let series = "<ser><val><numRef><numCache><pt idx='49999'><v>1</v></pt></numCache></numRef></val></ser>";
            let text = format!(
                "<chart><plotArea><barChart>{}</barChart></plotArea></chart>",
                series.repeat(100)
            );
            cases.push((
                "bounds sparse cache domains",
                (&declared("custom/chart.part", CHART, &text)).to_owned(),
                Err(LARGE),
            ));
            let text = "<chart><numCache><pt idx='49999'><v>1</v></pt></numCache></chart>";
            cases.push((
                "bounds sparse cache domains",
                (&archive(&[
                    ("ppt/charts/first.xml", text),
                    ("ppt/charts/second.xml", text),
                ]))
                    .to_owned(),
                Err(LARGE),
            ));
        }
        {
            for (count, result) in [(1, Ok(())), (2, Err(LARGE))] {
                let points = "<pt><v>1</v></pt>".repeat(count);
                let text = format!(
                    "<chart><strCache><pt idx='49998'><v>First</v></pt></strCache><numCache>{points}</numCache></chart>"
                );
                cases.push((
                    "uses missing index fallbacks",
                    (&declared("custom/chart.part", CHART, &text)).to_owned(),
                    result,
                ));
            }
            cases.push(("uses missing index fallbacks", (&declared(
                    "custom/chart.part",
                    CHART,
                    "<chart><multiLvlStrCache><lvl><pt idx='49998'><v>First</v></pt></lvl><lvl><pt idx='49999'><v>Unused</v></pt></lvl></multiLvlStrCache><numCache><pt><v>1</v></pt></numCache></chart>"
                )).to_owned(), Ok(())));
        }
        {
            let text = "<chart><numCache><pt idx='50000'><v>1</v></pt></numCache></chart>";
            for bytes in [
                declared("custom/chart.part", CHART, text),
                linked("custom/chart.part", "chart", text),
            ] {
                cases.push((
                    "rejects sparse custom parts",
                    (&bytes).to_owned(),
                    Err(LARGE),
                ));
            }
        }
        {
            cases.push((
                "rejects large declared counts",
                (&declared(
                    "custom/chart.part",
                    CHART,
                    "<chart><numCache><ptCount val='50001'/></numCache></chart>",
                ))
                    .to_owned(),
                Err(LARGE),
            ));
        }
        {
            let text = format!(
                "<chart><numCache>{}</numCache></chart>",
                "<pt idx='0'><v>1</v></pt>".repeat(50_001)
            );
            cases.push((
                "bounds actual point counts",
                (&declared("custom/chart.part", CHART, &text)).to_owned(),
                Err(LARGE),
            ));
        }
        for (index, (case, bytes, expected)) in cases.into_iter().enumerate() {
            assert_eq!(validate(PPTX, &bytes), expected, "{case}: row {index}");
        }
    }
}

#[test]
fn rejects_colliding_normalized_parts() {
    assert_eq!(
        validate(
            XLSX,
            &archive(&[
                ("xl/worksheets/sheet1.xml", "<worksheet/>"),
                ("xl/worksheets/../worksheets/sheet1.xml", "<worksheet/>")
            ])
        ),
        Err(FAILED)
    );
}
