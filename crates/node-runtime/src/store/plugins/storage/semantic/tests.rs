use super::*;
use sailry_protocol::plugin;
use serde_json::json;

struct Fixture {
    db: Connection,
    namespace: Namespace,
}

impl Fixture {
    fn new(semantic: bool) -> Self {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../../../schema.sql"))
            .unwrap();
        let summary = plugin::Summary {
            name: "owner".into(),
            revision: 1,
            digest: "a".repeat(64),
            settings_revision: 0,
            enabled: true,
            version: None,
            description: None,
        };
        let reference = summary.reference();
        let extension = serde_json::from_value(json!({
            "api_version":"v1","actions":["storage.read","storage.write"],
            "storage":{"collections":[{"scope":"node","prefix":"entry/","schema":true,"index":{"semantic":semantic}}]}
        })).unwrap();
        let info = plugin::Info {
            release_notes: None,
            summary,
            origin: None,
            icon: None,
            skill: None,
            mcp_source: None,
            extension: Some(extension),
            skills: vec![],
            mcp: vec![],
            settings: None,
            issues: vec![],
        };
        db.execute(
            "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
            params![
                reference.digest,
                reference.name,
                serde_json::to_vec(&info).unwrap()
            ],
        )
        .unwrap();
        let namespace = Namespace::from_reference(&db, &reference).unwrap();
        Self { db, namespace }
    }

    fn write(&self, name: &str, key: &str, value: Value, expected: u64, index: Option<Index>) {
        super::super::write(
            &self.db,
            name,
            key,
            &value,
            &serde_json::to_vec(&value).unwrap(),
            expected,
            index.as_ref(),
        )
        .unwrap();
    }

    fn body(&self, value: &str, expected: u64) {
        self.write("owner", "page", json!({"bodies":{"a/b~c":{"body":value,"revision":1},"neighbor":{"body":"Other source"}}}), expected, None);
    }

    fn entry(&self, key: &str, source: Option<Source>) {
        self.write(
            "owner",
            key,
            json!({"summary":{"title":"Raw title!"},"body_revision":1}),
            0,
            Some(Index {
                fields: ["Raw title!".into(), "sorted keywords".into()],
                tags: vec!["active".into()],
                order: 8,
                source,
            }),
        );
    }

    fn source() -> Source {
        Source {
            key: "page".into(),
            pointer: "/bodies/a~1b~0c/body".into(),
        }
    }
}

#[test]
fn captures_raw_text_and_current_page_revision() {
    let fixture = Fixture::new(true);
    fixture.body("Raw body with punctuation!\nOriginal order", 0);
    fixture.entry("entry/one", Some(Fixture::source()));
    fixture.body("Raw body with punctuation!\nOriginal order", 1);
    let captured = record(&fixture.db, &fixture.namespace, "entry/one")
        .unwrap()
        .unwrap();
    assert_eq!(captured.entry.key, "entry/one");
    assert_eq!(captured.entry.revision, 1);
    assert_eq!(captured.entry.value["body_revision"], 1);
    assert_eq!(captured.title, "Raw title!");
    assert_eq!(captured.body, "Raw body with punctuation!\nOriginal order");
    assert_eq!(captured.source_revision, 2);
    assert_eq!(captured.index.tags, ["active"]);
    assert_eq!(captured.index.order, 8);
    fixture.body(&captured.body, 2);
    let refreshed = record(&fixture.db, &fixture.namespace, "entry/one")
        .unwrap()
        .unwrap();
    assert_eq!(refreshed.source_revision, 3);
    assert_eq!(refreshed.entry, captured.entry);
    assert_eq!(refreshed.source_hash, captured.source_hash);
}

#[test]
fn preserves_captured_ownership_and_opt_in() {
    let fixture = Fixture::new(true);
    fixture.write(
        "other",
        "page",
        json!({"bodies":{"a/b~c":{"body":"Other namespace"}}}),
        0,
        None,
    );
    fixture.entry("entry/one", Some(Fixture::source()));
    assert!(
        record(&fixture.db, &fixture.namespace, "entry/one")
            .unwrap()
            .is_none()
    );
    fixture.body("Owned namespace", 0);
    fixture.entry("entry/keyword", None);
    fixture.entry("undeclared", Some(Fixture::source()));
    let captured = record(&fixture.db, &fixture.namespace, "entry/one")
        .unwrap()
        .unwrap();
    assert_eq!(captured.body, "Owned namespace");
    for key in ["entry/keyword", "undeclared"] {
        assert!(
            record(&fixture.db, &fixture.namespace, key)
                .unwrap()
                .is_none()
        );
    }
    let plain = Fixture::new(false);
    plain.body("Explicit opt-in required", 0);
    plain.entry("entry/one", Some(Fixture::source()));
    assert!(
        record(&plain.db, &plain.namespace, "entry/one")
            .unwrap()
            .is_none()
    );
}

#[test]
fn requires_raw_string_at_source_pointer() {
    let fixture = Fixture::new(true);
    fixture.write(
        "owner",
        "page",
        json!({"bodies":{"a/b~c":{"body":42}}}),
        0,
        None,
    );
    fixture.entry("entry/one", Some(Fixture::source()));
    assert!(matches!(
        record(&fixture.db, &fixture.namespace, "entry/one"),
        Err(Fault {
            code: ErrorCode::InvalidRequest,
            ..
        })
    ));
    let mut index = Index {
        fields: [String::new(), String::new()],
        tags: vec![],
        order: 0,
        source: Some(Source {
            key: "page".into(),
            pointer: "/bad~2".into(),
        }),
    };
    assert!(search::validate_index(&index).is_err());
    index.source = Some(Fixture::source());
    search::validate_index(&index).unwrap();
}
