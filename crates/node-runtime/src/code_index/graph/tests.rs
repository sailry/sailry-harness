use super::resolve::*;
use sailry_protocol::code_index::Confidence;

fn candidate(path: &str, name: &str, kind: &str, start: u32, end: u32) -> Candidate {
    Candidate {
        path: path.into(),
        name: name.into(),
        kind: kind.into(),
        start_line: start,
        end_line: end,
        start,
        end,
        depth: 0,
        signature: None,
    }
}

fn paths<'a>(chosen: &[&'a Candidate]) -> Vec<&'a str> {
    chosen
        .iter()
        .map(|candidate| candidate.path.as_str())
        .collect()
}

#[test]
fn prefers_nearer_definitions_and_flags_ambiguity() {
    let all = [
        candidate("src/app/main.rs", "run", "function", 10, 20),
        candidate("src/store/mod.rs", "run", "function", 0, 5),
        candidate("src/app/other.rs", "run", "function", 0, 5),
        candidate("tools/run.rs", "run", "function", 0, 5),
    ];
    let imported = ["store".to_owned()];
    let site = Site {
        path: "src/app/main.rs",
        start: 30,
        receiver: false,
        imports: &imported,
    };
    let (tier, chosen) = resolve(&site, &all).unwrap();
    assert_eq!(
        (tier, paths(&chosen)),
        (Confidence::Local, vec!["src/app/main.rs"])
    );
    let site = Site {
        path: "src/app/lib.rs",
        start: 0,
        receiver: false,
        imports: &imported,
    };
    let (tier, chosen) = resolve(&site, &all).unwrap();
    assert_eq!(
        (tier, paths(&chosen)),
        (Confidence::Imported, vec!["src/store/mod.rs"])
    );
    let site = Site {
        path: "src/app/lib.rs",
        start: 0,
        receiver: false,
        imports: &[],
    };
    let (tier, chosen) = resolve(&site, &all[1..]).unwrap();
    assert_eq!(
        (tier, paths(&chosen)),
        (Confidence::Directory, vec!["src/app/other.rs"])
    );
    let site = Site {
        path: "docs/example.rs",
        start: 0,
        receiver: false,
        imports: &[],
    };
    let (tier, chosen) = resolve(&site, &all[1..]).unwrap();
    assert_eq!(tier, Confidence::Workspace);
    assert_eq!(chosen.len(), 3, "workspace matches stay ambiguous");
    assert!(resolve(&site, &[]).is_none());
}

#[test]
fn narrows_members_to_their_owner() {
    let outline = [
        candidate("src/lib.rs", "Left", "impl", 0, 100),
        candidate("src/lib.rs", "new", "method", 10, 20),
        candidate("src/lib.rs", "Right", "impl", 200, 300),
        candidate("src/lib.rs", "new", "method", 210, 220),
        candidate("src/lib.rs", "new", "function", 400, 410),
    ];
    let targets: Vec<&Candidate> = outline.iter().filter(|item| item.name == "new").collect();
    let starts = |chosen: Vec<&Candidate>| chosen.iter().map(|item| item.start).collect::<Vec<_>>();
    let member = Site {
        path: "src/lib.rs",
        start: 250,
        receiver: true,
        imports: &[],
    };
    assert_eq!(starts(narrow(&member, targets.clone(), &outline)), [210]);
    let bare = Site {
        receiver: false,
        ..member
    };
    assert_eq!(starts(narrow(&bare, targets.clone(), &outline)), [210, 400]);
    let outside = Site {
        start: 500,
        receiver: true,
        ..bare
    };
    assert_eq!(starts(narrow(&outside, targets, &outline)), [400]);
}

#[test]
fn matches_import_keys_on_segments() {
    for (key, path) in [
        ("src/lib/render", "src/lib/render.ts"),
        ("store", "src/store/mod.rs"),
        ("store", "src/store.rs"),
        ("code_index/index", "crates/node/src/code_index/index.rs"),
        ("example.com/app/internal/store", "internal/store/db.go"),
        ("app/models", "app/models.py"),
    ] {
        assert!(imports(key, path), "{key} -> {path}");
    }
    for (key, path) in [
        ("store", "src/restore/mod.rs"),
        ("render", "src/lib/renderer.ts"),
        ("store/open", "tools/open.rs"),
        ("", "main.rs"),
    ] {
        assert!(!imports(key, path), "{key} -> {path}");
    }
    assert!(definition("method") && definition("class") && definition("constant"));
    assert!(!definition("variable") && !definition("impl") && !definition("data"));
}
