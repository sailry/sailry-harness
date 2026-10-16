use super::*;
use crate::file_fixture::Fixture;

const CHART: &[u8] = include_bytes!("../../src/office/fixtures/chart.xlsx");
const INVALID: &[u8] = include_bytes!("../../src/office/fixtures/invalid-row.xlsx");
const BOOK: &[u8] = include_bytes!("../../src/office/fixtures/book.xlsx");

#[tokio::test]
async fn reads_chart_cells() {
    let fixture = Fixture::empty().await;
    for (index, client) in fixture.clients().into_iter().enumerate() {
        let path = format!("chart-{index}.xlsx");
        std::fs::write(fixture.root.join(&path), CHART).unwrap();
        let Output::OfficeContent(content) = client
            .execute(client.prepare(Command::ReadOffice {
                worktree: fixture.worktree,
                options: Read {
                    source: Source::Path(path.clone()),
                    offset: 0,
                },
            }))
            .await
            .unwrap()
        else {
            panic!("workbook content expected")
        };
        assert_eq!(content.path, path);
        assert_eq!(content.revision, blake3::hash(CHART).to_hex().as_str());
        let sheet = content
            .sections
            .iter()
            .find(|section| section.name == "Sales")
            .expect("Sales worksheet expected");
        for line in [
            "A1: month",
            "B1: revenue",
            "A2: January",
            "B2: 12",
            "A3: February",
            "B3: 8",
            "A4: Total",
            "B4: 20 [=SUM(B2:B3)]",
        ] {
            assert!(sheet.text.lines().any(|actual| actual == line), "{line}");
        }
        assert_eq!(sheet.text.lines().count(), 8);
        assert_eq!(std::fs::read(fixture.root.join(&path)).unwrap(), CHART);
    }
    fixture.close().await;
}

#[tokio::test]
async fn reports_parser_panics() {
    let fixture = Fixture::empty().await;
    std::fs::write(fixture.root.join("invalid.xlsx"), INVALID).unwrap();
    std::fs::write(fixture.root.join("book.xlsx"), BOOK).unwrap();
    for client in fixture.clients() {
        let fault = client
            .execute(client.prepare(Command::ReadOffice {
                worktree: fixture.worktree,
                options: Read {
                    source: Source::Path("invalid.xlsx".into()),
                    offset: 0,
                },
            }))
            .await
            .unwrap_err();
        assert_eq!(fault.code, ErrorCode::InvalidRequest, "{fault}");
        assert!(fault.message.contains("workbook"), "{fault}");
        assert!(fault.message.contains("InvalidDigit"), "{fault}");
        assert_eq!(
            std::fs::read(fixture.root.join("invalid.xlsx")).unwrap(),
            INVALID
        );

        let Output::OfficeContent(content) = client
            .execute(client.prepare(Command::ReadOffice {
                worktree: fixture.worktree,
                options: Read {
                    source: Source::Path("book.xlsx".into()),
                    offset: 0,
                },
            }))
            .await
            .unwrap()
        else {
            panic!("workbook content expected")
        };
        assert_eq!(content.revision, blake3::hash(BOOK).to_hex().as_str());
        assert!(content.sections.iter().any(|section| {
            section
                .text
                .lines()
                .any(|line| line == "B2: 20 [=SUM(A2:A3)]")
        }));
        assert_eq!(std::fs::read(fixture.root.join("book.xlsx")).unwrap(), BOOK);
    }
    fixture.close().await;
}
