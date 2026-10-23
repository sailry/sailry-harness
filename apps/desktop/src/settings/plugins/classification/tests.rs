use super::*;
use core::prelude::v1::test;
use sailry_protocol::plugin::{Mcp, McpTransport, mcp, skills};

fn package(name: &str) -> Info {
    serde_json::from_value(serde_json::json!({
        "summary": { "name": name, "revision": 1, "digest": name, "settings_revision": 1,
            "enabled": true, "version": "0.1.0", "description": null },
        "icon": null, "extension": null, "settings": null,
    }))
    .unwrap()
}

#[test]
fn bundled_resources_stay_with_plugin() {
    let mut info = package("mcp-with-skills");
    info.skills.push(
        serde_json::from_value(serde_json::json!({
            "name": "documents", "description": "Read documents", "path": "skills/documents"
        }))
        .unwrap(),
    );
    info.mcp.push(Mcp {
        host: None,
        name: "documents".into(),
        transport: McpTransport::Stdio,
        allowed: true,
        availability: mcp::Availability::Available,
    });
    assert_eq!(Kind::of(&info), Kind::Plugin);
    assert_eq!(Kind::of(&info).section(), Section::Plugins);
}

#[test]
fn provenance_selects_core_management() {
    let mut skill = package("ordinary-name");
    skill.skill = Some(skills::Provenance::Imported {
        imported: skills::Imported {
            digest: "captured".into(),
            path: "skills/documents".into(),
            kind: sailry_protocol::plugin::UploadSource::Directory,
        },
    });
    assert_eq!(Kind::of(&skill).section(), Section::Skills);
    let mut mcp = package("another-name");
    mcp.mcp_source = Some(mcp::Definition::Stdio {
        command: "fixture".into(),
        args: Vec::new(),
        env: Vec::new(),
    });
    assert_eq!(Kind::of(&mcp).section(), Section::Connectors);
}

#[test]
fn missing_and_stale_metadata_stay_unclassified() {
    let current = package("current");
    let stale = package("stale");
    let missing = package("missing");
    let mut changed = stale.summary.clone();
    changed.settings_revision += 1;
    changed.revision += 1;
    let summaries = vec![missing.summary, changed, current.summary.clone()];
    let entries = BTreeMap::from([
        (current.summary.name.clone(), current.clone()),
        (stale.summary.name.clone(), stale),
    ]);
    let selected = inventory(&summaries, &entries, Kind::Plugin);
    assert_eq!(selected, vec![&current]);
}
