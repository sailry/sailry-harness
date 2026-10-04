use super::*;
use sailry_protocol::plugin::{Info, Origin};
use serde_json::json;
use std::collections::BTreeMap;

fn package(fixture: &Fixture) {
    fixture.package("1.0.0");
    let root = fixture.directory.path().join("project/package");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("plugin.json")).unwrap()).unwrap();
    manifest["extensions"] = json!({
        "dev.sailry.platform": {
            "api_version": "v1",
            "actions": [],
            "display": {"label": "Metadata fixture"},
            "settings_schema": "dev.sailry.platform/settings.json"
        }
    });
    std::fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/settings.json"),
        json!({
            "$schema": sailry_protocol::plugin::settings::SCHEMA,
            "type": "object",
            "additionalProperties": false,
            "properties": {"label": {"type": "string", "default": "Original"}}
        })
        .to_string(),
    )
    .unwrap();
}

fn replace(fixture: &Fixture, original: &Info) -> Info {
    let Output::PluginSettings(settings) = fixture.execute(Command::SavePluginSettings {
        package: original.summary.reference(),
        values: BTreeMap::from([("label".into(), json!("Saved"))]),
        secrets: BTreeMap::new(),
    }) else {
        panic!("plugin settings expected")
    };
    assert_eq!(settings.package.settings_revision, 1);
    assert_eq!(fixture.plugins()[0].revision, 2);
    let Output::Plugin(current) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.worktree,
        path: "package".into(),
        name: "example".into(),
        expected_revision: 2,
    }) else {
        panic!("plugin expected")
    };
    assert_eq!(current.summary.revision, 3);
    assert_eq!(current.summary.settings_revision, 1);
    assert_eq!(current.summary.digest, original.summary.digest);
    assert_eq!(
        current.origin,
        Some(Origin::Worktree {
            worktree: fixture.worktree,
            path: "package".into()
        })
    );
    current
}

fn coalesced(cx: &mut TestAppContext, read_mode: u8) {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        package(&fixture);
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "plugins-install");
        select(visual, fixture.directory.path().join("project/package"));
        updates::ready(&owner, visual, "example", 1);
        let cache = owner.read_with(visual, |owner, _| {
            owner.plugin_catalog.metadata.as_ref().unwrap().clone()
        });
        let original = cache.read_with(visual, |cache, _| cache.entries["example"].clone());
        assert_eq!(original.origin, Some(Origin::Directory));
        assert_eq!(original.summary.settings_revision, 0);
        assert!(
            original.settings.is_some(),
            "settings fixture must be valid"
        );
        assert!(original.issues.is_empty(), "{:?}", original.issues);
        assert_eq!(metadata::title(&original), "Metadata fixture");

        // Retain the original cache without observing intermediate mutations.
        // One recovered inventory then carries both real command outcomes.
        fixture.bind(&owner, visual, true);
        let current = replace(&fixture, &original);
        let expected = fixture.public_packages(false);
        fixture
            .transport
            .read_mode
            .store(read_mode, Ordering::SeqCst);
        cache.update(visual, |cache, cx| {
            cache.accept(&expected, cx);
            assert!(!cache.settled());
            assert_eq!(cache.entries["example"], original);
            assert_ne!(cache.entries["example"].summary, current.summary);
            assert_eq!(
                metadata::title(&cache.entries["example"]),
                "Metadata fixture"
            );
        });

        if read_mode == 2 {
            wait(visual, |_| fixture.transport.entered.is_cancelled());
            cache.read_with(visual, |cache, _| {
                assert!(!cache.settled());
                assert_eq!(cache.entries["example"], original);
                assert_ne!(cache.entries["example"].summary, current.summary);
            });
            fixture.transport.release.cancel();
            wait(visual, |cx| cache.read(cx).settled());
            cache.read_with(visual, |cache, _| {
                assert_eq!(cache.entries["example"], current);
                assert!(cache.errors.is_empty());
            });
        } else {
            wait(visual, |cx| {
                cache.read(cx).errors.get("example") == Some(&"plugins_read_failed")
            });
            cache.read_with(visual, |cache, _| {
                assert!(!cache.settled());
                assert_eq!(cache.entries["example"], original);
                assert_ne!(cache.entries["example"].summary, current.summary);
            });
        }
        drop(cache);
        fixture.close(visual);
    }
}

#[gpui::test]
fn refetches_origin_after_coalesced_changes(cx: &mut TestAppContext) {
    init(cx);
    coalesced(cx, 2);
}

#[gpui::test]
fn reports_a_failed_current_read(cx: &mut TestAppContext) {
    init(cx);
    coalesced(cx, 1);
}
