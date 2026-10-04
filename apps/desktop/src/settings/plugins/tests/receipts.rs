use super::*;
use sailry_protocol::plugin::Origin;

#[gpui::test]
fn old_install_receipts_preserve_a_newer_directory(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for source in [Origin::Directory, Origin::Archive] {
            let fixture = Fixture::new(remote);
            fixture.package("1.0.0");
            let original = fixture.directory.path().join("project/package");
            let selected = match &source {
                Origin::Directory => original.clone(),
                Origin::Archive => fixture.archive(),
                _ => unreachable!(),
            };
            let (owner, visual) = fixture.mount(cx);
            let first = visual.update(|window, cx| {
                let editor = create(owner.clone(), None, false, window, cx).unwrap();
                editor.update(cx, |editor, cx| editor.choose(window, cx));
                editor
            });
            fixture.transport.mode.store(1, Ordering::SeqCst);
            select(visual, selected);
            shown(visual, "plugin-retry", true);
            updates::ready(&owner, visual, "example", 1);
            let request = first.read_with(visual, |editor, _| {
                assert!(!editor.pending && !editor.closed);
                editor.request.as_ref().unwrap().id
            });
            let installed = fixture.plugins().remove(0);
            let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
                name: "example".into(),
            }) else {
                panic!("plugin expected")
            };
            assert_eq!(info.origin.as_ref(), Some(&source));

            let replacement = fixture.directory.path().join("replacement-package");
            std::fs::create_dir_all(replacement.join("skills/analysis")).unwrap();
            if source == Origin::Directory {
                std::fs::create_dir_all(replacement.join(sailry_protocol::plugin::NAMESPACE))
                    .unwrap();
            }
            for path in ["plugin.json", "mcp.json", "skills/analysis/SKILL.md"] {
                std::fs::copy(original.join(path), replacement.join(path)).unwrap();
            }
            let canonical = replacement.canonicalize().unwrap();
            tap(visual, "plugins-install");
            select(visual, replacement);
            updates::ready(&owner, visual, "example", 2);
            let current = fixture.plugins().remove(0);
            assert_eq!(current.digest, installed.digest);
            wait(visual, |cx| {
                crate::preferences::plugins::directory(
                    fixture.client.target(),
                    "example",
                    &current.digest,
                    cx,
                ) == Some(canonical.clone())
            });
            first.read_with(visual, |editor, _| {
                assert_eq!(editor.request.as_ref().unwrap().id, request);
                assert!(!editor.closed);
            });

            // Replay the retained admission through the Editor lifecycle, not
            // a second selection or a synthetic replacement command.
            visual.update(|window, cx| {
                first.update(cx, |editor, cx| editor.submit(window, cx));
            });
            wait(visual, |cx| first.read(cx).closed);
            assert_eq!(fixture.plugins().as_slice(), std::slice::from_ref(&current));
            assert_eq!(
                visual.update(|_, cx| crate::preferences::plugins::directory(
                    fixture.client.target(),
                    "example",
                    &current.digest,
                    cx,
                )),
                Some(canonical)
            );
            let ids = fixture.transport.requests.lock().unwrap();
            assert_eq!(ids.len(), 3);
            assert_eq!(ids[0], request);
            assert_ne!(ids[1], request);
            assert_eq!(ids[2], request);
            drop(ids);
            drop(first);
            fixture.close(visual);
        }
    }
}
