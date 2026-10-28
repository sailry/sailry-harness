//! Billable bounded-page checks use a real model and files larger than the edit budget.
use super::compaction::Fixture;
use super::*;
use std::io::Write;

#[tokio::test]
#[ignore = "requires an explicit inactive provider profile and incurs real model usage"]
async fn reads_large_file_page() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote).await;
        let boundary = format!("PAGE-END-{}", uuid::Uuid::new_v4());
        let mut expected = (1..20)
            .map(|number| format!("Header {number:02}: Unicode 中文🙂\n"))
            .collect::<Vec<_>>();
        expected.push(format!("{boundary}\n"));
        let mut file = std::io::BufWriter::new(
            std::fs::File::create(fixture.root().join("large.txt")).unwrap(),
        );
        let mut hash = blake3::Hasher::new();
        for line in &expected {
            file.write_all(line.as_bytes()).unwrap();
            hash.update(line.as_bytes());
        }
        let mut buffer = [b'x'; 64 * 1024];
        *buffer.last_mut().unwrap() = b'\n';
        for _ in 0..=1024 {
            file.write_all(&buffer).unwrap();
            hash.update(&buffer);
        }
        file.flush().unwrap();
        let bytes = file.get_ref().metadata().unwrap().len();
        assert!(bytes > 64 * 1024 * 1024);
        drop(file);
        let revision = hash.finalize().to_hex().to_string();

        let page = fixture
            .submit("Use read_file to read large.txt with offset=0 and limit=20. Do not use shell commands or any other file-read tool. Reply with only the exact text of line 20, without its line number.".into())
            .await;
        let calls = tool_calls(&fixture.client, &page).await;
        assert!(
            calls
                .iter()
                .all(|(name, _)| name != &plugin_tool("commands", "run_command"))
        );
        let reads = calls
            .iter()
            .filter(|(name, _)| name == &plugin_tool("files", "read_file"))
            .collect::<Vec<_>>();
        assert_eq!(reads.len(), 1, "one bounded file read expected");
        let arguments = &reads[0].1.arguments;
        assert_eq!(arguments["path"], "large.txt");
        assert_eq!(arguments["offset"], 0);
        assert_eq!(arguments["limit"], 20);

        let details = tool_details(&fixture.client, &page).await;
        let detail = details
            .iter()
            .find(|(name, _)| name == &plugin_tool("files", "read_file"))
            .map(|(_, detail)| detail)
            .expect("file read result expected");
        let result = detail.result.as_ref().expect("successful result expected");
        assert!(!result.to_string().contains("inspection limit"));
        let Output::FileLines(lines) = serde_json::from_value(result.clone()).unwrap() else {
            panic!("file line page expected")
        };
        assert_eq!(lines.offset, 0);
        assert_eq!(lines.next_offset, Some(20));
        assert_eq!(lines.size, bytes);
        assert_eq!(lines.revision, revision);
        assert_eq!(lines.lines.len(), 20);
        for (index, line) in lines.lines.iter().enumerate() {
            assert_eq!(line.number, index + 1);
            assert_eq!(line.text, expected[index]);
        }
        let preview = &detail
            .resolved
            .as_ref()
            .expect("resolved file presentation expected")
            .output
            .as_ref()
            .expect("file output expected")
            .preview;
        assert_eq!(preview.lines().count(), 20);
        assert!(preview.starts_with("1: Header 01"));
        assert!(preview.contains(&format!("20: {boundary}")));
        assert!(preview.len() < 4096);

        let text = page
            .entries
            .iter()
            .rev()
            .filter(|entry| entry.author != "user")
            .find_map(|entry| {
                let text = entry
                    .parts
                    .iter()
                    .filter_map(|part| match part {
                        Part::Text(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>();
                (!text.is_empty()).then_some(text)
            })
            .expect("visible model answer expected");
        assert!(text.contains(&boundary));
        assert!(!text.contains("inspection limit"));
        println!(
            "live large file: remote={remote} bytes={bytes} lines={} preview_bytes={} revision=verified boundary=verified",
            lines.lines.len(),
            preview.len()
        );
        fixture.close().await;
    }
}
