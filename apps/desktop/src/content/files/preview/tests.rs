use super::*;
use crate::conversation::live::tests::fixture::Fixture;
use core::prelude::v1::test;

fn file(path: &str) -> File {
    File {
        path: path.into(),
        name: None,
        mime: "text/html".into(),
        size: None,
        revision: None,
    }
}

#[test]
fn reads_authorized_files_locally_and_remotely() {
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        let context = fixture.files_context();
        let path = fixture.directory.path().join("project/预览 #1.html");
        let html = "<!doctype html><button onclick=\"this.textContent='Done'\">Run</button>";
        std::fs::write(&path, html).unwrap();
        let read = |file| {
            fixture.runtime.block_on(load(
                fixture.binding.client.clone(),
                fixture.binding.worktree,
                file,
                Some(context.clone()),
                CancellationToken::new(),
            ))
        };
        let published = file("预览 #1.html");
        assert_eq!(read(published.clone()).unwrap(), html);
        let mut changed = published.clone();
        changed.revision = Some("0".repeat(64));
        assert_eq!(read(changed), Err("artifact_preview_changed"));
        assert_eq!(
            read(file("../outside.html")),
            Err("artifact_preview_failed")
        );
        std::fs::write(&path, vec![b' '; 2 * 1024 * 1024 + 1]).unwrap();
        assert_eq!(read(published.clone()), Err("artifact_preview_large"));
        std::fs::write(&path, html).unwrap();
        let sailry_protocol::Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
            name: context.package.name.clone(),
        }) else {
            panic!("Files package expected");
        };
        fixture.execute(Command::SetPluginEnabled {
            name: context.package.name.clone(),
            expected_revision: info.summary.revision,
            enabled: false,
        });
        assert_eq!(read(published), Err("artifact_preview_failed"));
        fixture.close();
    }
}
