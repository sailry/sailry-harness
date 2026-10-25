use super::*;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use std::time::Duration;

#[tokio::test]
async fn reconciles_writes_renames_and_external_deletions() {
    let fixture = Fixture::empty().await;
    let initial = "pub fn original_symbol() {}\n";
    let replacement = "pub fn changed_symbol() {}\n";
    for client in fixture.clients() {
        client
            .execute(client.prepare(Command::WriteFile {
                worktree: fixture.worktree,
                path: "src/original.rs".into(),
                text: initial.into(),
                expected_revision: None,
            }))
            .await
            .unwrap();
        let first = client
            .search_code(fixture.worktree, code_index::Search::default())
            .await
            .unwrap();
        assert!(
            first
                .symbols
                .iter()
                .any(|item| item.name == "original_symbol")
        );
        client
            .execute(client.prepare(Command::WriteFile {
                worktree: fixture.worktree,
                path: "src/original.rs".into(),
                text: replacement.into(),
                expected_revision: Some(revision(initial)),
            }))
            .await
            .unwrap();
        let changed = client
            .search_code(fixture.worktree, code_index::Search::default())
            .await
            .unwrap();
        assert!(changed.index_revision > first.index_revision);
        assert!(
            changed
                .symbols
                .iter()
                .any(|item| item.name == "changed_symbol")
        );
        assert!(
            changed
                .symbols
                .iter()
                .all(|item| item.name != "original_symbol")
        );
        client
            .execute(client.prepare(Command::MoveFile {
                worktree: fixture.worktree,
                from: "src/original.rs".into(),
                to: "src/renamed.rs".into(),
                expected_revision: revision(replacement),
            }))
            .await
            .unwrap();
        let moved = client
            .search_code(fixture.worktree, code_index::Search::default())
            .await
            .unwrap();
        assert!(moved.index_revision > changed.index_revision);
        assert!(!moved.symbols.is_empty());
        assert!(
            moved
                .symbols
                .iter()
                .all(|item| item.path == "src/renamed.rs")
        );
        std::fs::remove_file(fixture.root.join("src/renamed.rs")).unwrap();
        let deleted = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let page = client
                    .search_code(fixture.worktree, code_index::Search::default())
                    .await
                    .unwrap();
                if page.symbols.is_empty() {
                    break page;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(deleted.index_revision > moved.index_revision);
    }
    fixture.close().await;
}

fn commit(repository: &git2::Repository) -> git2::Oid {
    let mut index = repository.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    repository
        .commit(Some("HEAD"), &signature, &signature, "Initial", &tree, &[])
        .unwrap()
}

fn branch(repository: &git2::Repository, parent: git2::Oid) -> git2::Oid {
    let parent = repository.find_commit(parent).unwrap();
    let mut builder = repository
        .treebuilder(Some(&parent.tree().unwrap()))
        .unwrap();
    builder
        .insert(
            "branch.rs",
            repository.blob(b"pub fn feature_symbol() {}\n").unwrap(),
            0o100644,
        )
        .unwrap();
    builder.remove("obsolete.rs").unwrap();
    builder
        .insert(
            "added.rs",
            repository.blob(b"pub fn added_symbol() {}\n").unwrap(),
            0o100644,
        )
        .unwrap();
    let tree = repository.find_tree(builder.write().unwrap()).unwrap();
    let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let id = repository
        .commit(None, &signature, &signature, "Feature", &tree, &[&parent])
        .unwrap();
    repository
        .branch("feature", &repository.find_commit(id).unwrap(), false)
        .unwrap();
    id
}

async fn switch(client: &Client, worktree: WorktreeId, name: &str, commit: git2::Oid) {
    let Output::GitStatus(status) = client
        .execute(client.prepare(Command::InspectGit { worktree }))
        .await
        .unwrap()
    else {
        panic!("Git status expected");
    };
    client
        .execute(client.prepare(Command::SwitchGitBranch {
            worktree,
            name: name.into(),
            commit: commit.to_string(),
            expected_head: status.head,
            expected_branch: status.branch,
            expected_index: status.index_revision.unwrap(),
        }))
        .await
        .unwrap();
}

#[tokio::test]
async fn replaces_branch_symbols_in_the_shared_cache() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let repository = git2::Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("branch.rs"), "pub fn main_symbol() {}\n").unwrap();
    std::fs::write(root.join("obsolete.rs"), "pub fn obsolete_symbol() {}\n").unwrap();
    let main = commit(&repository);
    let feature = branch(&repository, main);
    let node = Node::start_empty(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let worktree = file_fixture::register(&local, &root).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let fixture = Fixture {
        _temp: temp,
        root,
        node,
        controller,
        worktree,
    };
    for client in fixture.clients() {
        let before = client
            .search_code(worktree, code_index::Search::default())
            .await
            .unwrap();
        assert!(before.symbols.iter().any(|item| item.name == "main_symbol"));
        assert!(
            before
                .symbols
                .iter()
                .any(|item| item.name == "obsolete_symbol")
        );
        switch(&client, worktree, "feature", feature).await;
        let changed = client
            .search_code(worktree, code_index::Search::default())
            .await
            .unwrap();
        assert!(changed.index_revision > before.index_revision);
        assert!(
            changed
                .symbols
                .iter()
                .any(|item| item.name == "feature_symbol")
        );
        assert!(
            changed
                .symbols
                .iter()
                .any(|item| item.name == "added_symbol")
        );
        assert!(changed.symbols.iter().all(|item| {
            item.name != "main_symbol"
                && item.name != "obsolete_symbol"
                && item.path != "obsolete.rs"
        }));
        switch(&client, worktree, "main", main).await;
        let restored = client
            .search_code(worktree, code_index::Search::default())
            .await
            .unwrap();
        assert!(restored.index_revision > changed.index_revision);
        assert!(
            restored
                .symbols
                .iter()
                .any(|item| item.name == "main_symbol")
        );
        assert!(restored.symbols.iter().all(|item| item.path != "added.rs"));
        assert_eq!(repository.head().unwrap().target(), Some(main));
        assert!(!fixture.root.join("added.rs").exists());
    }
    fixture.close().await;
}
