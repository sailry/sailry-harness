use super::*;
use core::prelude::v1::test;
use sailry_client::Client;

#[test]
fn preserves_text_previews() {
    for path in ["video.mp4", "recording.wav", "document.pdf", "icon.ico"] {
        assert!(crate::content::files::external(path), "{path}");
    }
    for path in ["main.rs", "data.json", "image.svg", "README", "build.toml"] {
        assert!(!crate::content::files::external(path), "{path}");
    }
}

#[test]
fn preserves_execution_scope() {
    let fixture = crate::activity::fixture::Fixture::new();
    for index in 0..2 {
        let transport = if index == 0 {
            fixture.nodes[0].local()
        } else {
            fixture.nodes[0]
                .link()
                .remote(fixture.nodes[1].link().address())
        };
        let client = Arc::new(Client::new(transport));
        fixture.runtime.block_on(async {
            let Output::Snapshot(snapshot) = client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            let worktree = snapshot
                .worktrees
                .iter()
                .find(|tree| tree.id == fixture.sessions[index].worktree)
                .unwrap();
            let original = PathBuf::from(&worktree.path).join("original.bin");
            let bytes = [index as u8, 0, 255, 128];
            std::fs::write(&original, bytes).unwrap();
            let opened = prepare_core(
                client.clone(),
                worktree.id,
                "original.bin".into(),
                index == 0,
                CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(std::fs::read(&opened.path).unwrap(), bytes);
            assert_eq!(opened.copy.is_some(), index != 0);
            assert_eq!(opened.path == original.canonicalize().unwrap(), index == 0);
            if index == 0 {
                let directory = prepare_core(
                    client.clone(),
                    worktree.id,
                    String::new(),
                    true,
                    CancellationToken::new(),
                )
                .await
                .unwrap();
                assert_eq!(
                    directory.path,
                    PathBuf::from(&worktree.path).canonicalize().unwrap()
                );
            } else {
                assert!(
                    prepare_core(
                        client.clone(),
                        worktree.id,
                        String::new(),
                        false,
                        CancellationToken::new()
                    )
                    .await
                    .is_err()
                );
            }
            let outside = PathBuf::from(&worktree.path)
                .parent()
                .unwrap()
                .join("outside.bin");
            std::fs::write(&outside, "outside workspace").unwrap();
            assert!(
                prepare_core(
                    client.clone(),
                    worktree.id,
                    "../outside.bin".into(),
                    index == 0,
                    CancellationToken::new()
                )
                .await
                .is_err()
            );
            assert_eq!(
                std::fs::read_to_string(outside).unwrap(),
                "outside workspace"
            );
            assert!(
                prepare_core(
                    client.clone(),
                    WorktreeId::new(),
                    "original.bin".into(),
                    index == 0,
                    CancellationToken::new()
                )
                .await
                .is_err()
            );
            drop(opened);
            assert_eq!(std::fs::read(&original).unwrap(), bytes);
        });
    }
    fixture.close();
}
