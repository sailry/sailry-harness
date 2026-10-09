use super::*;
use crate::file_fixture::Fixture;
use std::{path::Path, time::Instant};

#[tokio::test]
#[ignore = "Measures the current checkout through local and paired remote Nodes"]
async fn measures_checkout_search_and_discovery() {
    let fixture = Fixture::start().await;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let clients = fixture.clients();
    let worktree = crate::file_fixture::register(&clients[0], &root).await;
    for (lane, client) in ["local", "remote"].into_iter().zip(&clients) {
        let started = Instant::now();
        let Output::SearchResults(result) = client
            .execute(client.prepare(Command::SearchFiles {
                worktree,
                options: FileSearch {
                    query: "fn ".into(),
                    mode: SearchMode::Content,
                    ..Default::default()
                },
            }))
            .await
            .unwrap()
        else {
            panic!("search results expected")
        };
        println!(
            "{lane} content search: {} ms, {} files scanned, {} skipped, partial={}",
            started.elapsed().as_millis(),
            result.scanned_files,
            result.skipped,
            result.partial
        );
        assert!(!result.partial, "{result:?}");
        assert!(result.scanned_files > 0);
        assert!(!result.matches.is_empty());
        let started = Instant::now();
        let Output::FileNames(result) = client
            .execute(client.prepare(Command::FindFiles {
                worktree,
                globs: vec!["**/*.rs".into()],
                path: String::new(),
                limit: MAX_FIND_MATCHES,
                offset: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("file names expected")
        };
        println!(
            "{lane} file discovery: {} ms, {} entries visited, {} skipped, partial={}",
            started.elapsed().as_millis(),
            result.visited,
            result.skipped,
            result.partial
        );
        assert!(!result.partial, "{result:?}");
        assert!(!result.paths.is_empty());
    }
    fixture.close().await;
}
