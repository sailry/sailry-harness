use super::*;
use sailry_protocol::{NodeId, TurnId};
use serde_json::{Value, json};
pub(super) mod snapshots;

async fn database() -> Connection {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../../schema.sql"))
        .await
        .unwrap();
    crate::store::local_models::initialize(&db).await.unwrap();
    db
}

async fn change_count(db: &Connection) -> i64 {
    db.prepare("SELECT total_changes()")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap()
}

async fn package(db: &Connection, digest: char, semantic: bool) -> plugin::Reference {
    let summary = plugin::Summary {
        compatibility: Default::default(),
        name: "private-memory".into(),
        revision: 1,
        digest: digest.to_string().repeat(64),
        settings_revision: 0,
        enabled: true,
        version: None,
        description: None,
    };
    let info = plugin::Info {
versions: Vec::new(),
        release_notes: None,
        summary, origin: None, icon: None, skill: None, mcp_source: None,
        extension: Some(serde_json::from_value(json!({
            "api_version":"v1", "actions":["storage.read","storage.write"],
            "storage":{"collections":[{"scope":"node","prefix":"meta/",
                "schema":{"type":"object","properties":{"visible":{"type":"boolean","default":true}}},
                "index":{"semantic":semantic}}]}
        })).unwrap()),
        skills:vec![], mcp:vec![], settings:None, issues:vec![],
    };
    let reference = info.summary.reference();
    db.execute(
        "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
        params![
            reference.digest.as_str(),
            reference.name.as_str(),
            encode(&info).unwrap()
        ],
    )
    .await
    .unwrap();
    db.execute("INSERT INTO plugins(name,revision,installed,body) VALUES(?1,1,1,?2) ON CONFLICT(name) DO UPDATE SET body=excluded.body",params![reference.name.as_str(),encode(&info).unwrap()]).await.unwrap();
    startup(db, &CancellationToken::new()).await.unwrap();
    reference
}

fn request(package: &plugin::Reference) -> Request {
    Request::new(
        NodeId([19; 32]),
        Command::SearchPluginValues(plugin::storage::Search {
            terms: vec!["vector".into()],
            query: Some("original vector query".into()),
            mode: knowledge::Mode::Hybrid,
            weights: [3, 1],
            all: vec![],
            any: vec![],
            offset: 0,
            limit: 100,
        }),
    )
    .with_plugin(plugin::Context {
        package: package.clone(),
        worktree: None,
        session: None,
        turn: None,
        invocation: None,
        surface: Default::default(),
    })
}

async fn value(
    db: &Connection,
    name: &str,
    key: &str,
    revision: u64,
    value: Value,
    index: Option<plugin::storage::Index>,
) {
    db.execute("INSERT INTO plugin_values(name,key,revision,value,index_data) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(name,key) DO UPDATE SET revision=excluded.revision,value=excluded.value,index_data=excluded.index_data",params![name,key,i64::try_from(revision).unwrap(),serde_json::to_vec(&value).unwrap(),index.map(|index|serde_json::to_vec(&index).unwrap())]).await.unwrap();
}

fn metadata(key: &str, pointer: &str) -> plugin::storage::Index {
    plugin::storage::Index {
        fields: ["Raw vector title".into(), "vector keyword body".into()],
        tags: vec!["visible".into()],
        order: 1,
        source: Some(plugin::storage::Source {
            key: key.into(),
            pointer: pointer.into(),
        }),
    }
}

async fn seed(db: &Connection) {
    seed_values(db).await;
    startup(db, &CancellationToken::new()).await.unwrap();
}

async fn seed_values(db: &Connection) {
    value(
        db,
        "private-memory",
        "bodies/page",
        7,
        json!({"bodies":{"escaped/id":{"body":"Original raw vector body"}}}),
        None,
    )
    .await;
    value(
        db,
        "private-memory",
        "meta/one",
        2,
        json!({"id":"one"}),
        Some(metadata("bodies/page", "/bodies/escaped~1id/body")),
    )
    .await;
    let mut plain = metadata("bodies/page", "/bodies/escaped~1id/body");
    plain.source = None;
    value(
        db,
        "private-memory",
        "meta/plain",
        3,
        json!({"id":"plain"}),
        Some(plain),
    )
    .await;
    value(
        db,
        "foreign-memory",
        "meta/foreign",
        1,
        json!({"id":"foreign"}),
        Some(metadata("bodies/page", "/bodies/escaped~1id/body")),
    )
    .await;
    search::rebuild(db).await.unwrap();
}

fn selection(plan: &Plan, record: &super::super::storage::Record) -> Selection {
    Selection {
        keys: vec![record.key.clone()],
        stamps: vec![Stamp {
            collection: plan.collections[0].collection.id,
            key: record.key.clone(),
            revision: identity(record).unwrap(),
        }],
        mode: knowledge::Mode::Semantic,
        indexing: false,
    }
}

#[tokio::test]
async fn registers_only_manifest_sources() {
    let db = database().await;
    let reference = package(&db, 'a', true).await;
    seed(&db).await;
    let writes = change_count(&db).await;
    let plan = prepare(&db, &request(&reference), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(change_count(&db).await, writes);
    assert_eq!(plan.collections.len(), 1);
    assert_eq!(plan.eligible, BTreeSet::from(["meta/one".into()]));
    let stored = &plan.collections[0];
    assert!(automatic(stored));
    assert!(stored.collection.definition.model.is_none());
    assert!(stored.collection.definition.shared_with.is_empty());
    assert_eq!(stored.capture.package.as_ref(), Some(&reference));
    let page = sources(&db, stored, None, 16, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(page.records.len(), 1);
    let raw = &page.records[0];
    assert_eq!(raw.key, "meta/one");
    assert_eq!(raw.title, "Raw vector title");
    assert_eq!(raw.body, "Original raw vector body");
    assert_eq!(raw.source_revision, 7);
    assert_eq!(raw.source.pointer, "/bodies/escaped~1id/body");
    assert!(
        unchanged(&db, stored, raw, &CancellationToken::new())
            .await
            .unwrap()
    );
    let result = materialize(
        &db,
        &request(&reference),
        &plan,
        &selection(&plan, raw),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!result.stale);
    assert_eq!(result.page.mode, knowledge::Mode::Semantic);
    assert_eq!(
        result.page.entries[0].value,
        json!({"id":"one","visible":true})
    );
    assert_eq!(result.page.entries[0].revision, 2);
    assert_eq!(result.page.entries[0].key, "meta/one");
    prepare(&db, &request(&reference), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(change_count(&db).await, writes);
    let off = database().await;
    let reference = package(&off, 'b', false).await;
    seed(&off).await;
    assert!(
        prepare(&off, &request(&reference), &CancellationToken::new())
            .await
            .unwrap()
            .collections
            .is_empty()
    );
    let empty = database().await;
    let reference = package(&empty, 'c', true).await;
    let mut index = metadata("bodies/page", "/body");
    index.source = None;
    value(
        &empty,
        &reference.name,
        "meta/plain",
        1,
        json!({}),
        Some(index),
    )
    .await;
    search::rebuild(&empty).await.unwrap();
    let plan = prepare(&empty, &request(&reference), &CancellationToken::new())
        .await
        .unwrap();
    assert!(plan.collections.is_empty());
}

#[tokio::test]
async fn queries_do_not_register_missing_collections() {
    let db = database().await;
    let reference = package(&db, 'a', true).await;
    seed_values(&db).await;
    let writes = change_count(&db).await;
    let plan = prepare(&db, &request(&reference), &CancellationToken::new())
        .await
        .unwrap();
    assert!(plan.collections.is_empty());
    assert_eq!(plan.keyword.len(), 2);
    assert_eq!(change_count(&db).await, writes);
    assert!(collections::load(&db).await.unwrap().is_empty());
    assert!(startup(&db, &CancellationToken::new()).await.unwrap());
    assert_eq!(
        prepare(&db, &request(&reference), &CancellationToken::new())
            .await
            .unwrap()
            .collections
            .len(),
        1
    );
}

#[tokio::test]
async fn registrations_do_not_consume_user_capacity() {
    let db = database().await;
    let reference = package(&db, 'a', true).await;
    seed(&db).await;
    let automatic = collections::load(&db).await.unwrap().remove(0);
    let definition = automatic.collection.definition.clone();
    let creation = Request::new(
        NodeId([19; 32]),
        Command::CreateKnowledgeCollection {
            definition: definition.clone(),
            plugin: Some(reference.name.clone()),
        },
    );
    for _ in 0..knowledge::MAX_COLLECTIONS {
        collections::create(&db, &creation, &definition, Some(&reference.name))
            .await
            .unwrap();
    }
    assert_eq!(
        collections::load(&db).await.unwrap().len(),
        knowledge::MAX_COLLECTIONS + 1
    );
    assert_eq!(
        collections::create(&db, &creation, &definition, Some(&reference.name))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    let latest = package(&db, 'b', true).await;
    let retained = prepare(&db, &request(&latest), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        retained.collections[0].collection.id,
        automatic.collection.id
    );
    assert_eq!(
        retained.collections[0].capture.package.as_ref(),
        Some(&latest)
    );
}

#[tokio::test]
async fn falls_back_after_source_or_generation_changes() {
    for generation in [false, true] {
        let db = database().await;
        let reference = package(&db, 'a', true).await;
        seed(&db).await;
        let request = request(&reference);
        let stop = CancellationToken::new();
        let plan = prepare(&db, &request, &stop).await.unwrap();
        let raw = sources(&db, &plan.collections[0], None, 16, &stop)
            .await
            .unwrap()
            .records
            .remove(0);
        let selected = selection(&plan, &raw);
        if generation {
            let mut stored = plan.collections[0].clone();
            collections::advance(&mut stored).unwrap();
            collections::persist(&db, &stored).await.unwrap();
        } else {
            value(
                &db,
                "private-memory",
                "bodies/page",
                8,
                json!({"bodies":{"escaped/id":{"body":"Changed raw source"}}}),
                None,
            )
            .await;
        }
        assert!(
            !unchanged(&db, &plan.collections[0], &raw, &stop)
                .await
                .unwrap()
        );
        let result = materialize(&db, &request, &plan, &selected, &stop)
            .await
            .unwrap();
        assert!(result.stale);
        assert!(result.page.indexing);
        assert_eq!(result.page.mode, knowledge::Mode::Keyword);
        assert_eq!(result.page.entries.len(), 2);
        assert!(
            result
                .page
                .entries
                .iter()
                .all(|entry| entry.value.get("body").is_none())
        );
    }
}

#[tokio::test]
async fn guards_only_the_embedding_choice() {
    use sailry_protocol::models::Kind;
    for kind in [
        None,
        Some(Kind::Ocr),
        Some(Kind::Decision),
        Some(Kind::Embedding),
    ] {
        let db = database().await;
        let reference = package(&db, 'a', true).await;
        seed(&db).await;
        let request = request(&reference);
        let stop = CancellationToken::new();
        let plan = prepare(&db, &request, &stop).await.unwrap();
        let raw = sources(&db, &plan.collections[0], None, 16, &stop)
            .await
            .unwrap()
            .records
            .remove(0);
        let selected = selection(&plan, &raw);
        let mut settings = crate::store::local_models::settings(&db).await.unwrap();
        if let Some(kind) = kind {
            settings.choices.get_mut(&kind).unwrap().enabled = false;
        }
        crate::store::local_models::execute(
            &db,
            &Request::new(NodeId([19; 32]), Command::SaveLocalModelSettings(settings)),
        )
        .await
        .unwrap();
        let result = materialize(&db, &request, &plan, &selected, &stop)
            .await
            .unwrap();
        if kind == Some(Kind::Embedding) {
            assert!(result.stale);
            assert_eq!(result.page.mode, knowledge::Mode::Keyword);
        } else {
            assert!(!result.stale);
            assert_eq!(result.page.mode, knowledge::Mode::Semantic);
            assert_eq!(result.page.entries.len(), 1);
            assert_eq!(result.page.entries[0].key, raw.key);
        }
    }
}

#[tokio::test]
async fn separates_page_receipts_from_content_identity() {
    let db = database().await;
    let reference = package(&db, 'a', true).await;
    seed(&db).await;
    let request = request(&reference);
    let stop = CancellationToken::new();
    let plan = prepare(&db, &request, &stop).await.unwrap();
    let stored = &plan.collections[0];
    let original = sources(&db, stored, None, 16, &stop)
        .await
        .unwrap()
        .records
        .remove(0);
    value(
        &db,
        &reference.name,
        "bodies/page",
        8,
        json!({"bodies":{
            "escaped/id":{"body":"Original raw vector body"},
            "sibling":{"body":"An edited sibling"}
        }}),
        None,
    )
    .await;
    assert!(!unchanged(&db, stored, &original, &stop).await.unwrap());
    let current = source(&db, stored, "meta/one", &stop)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.source_revision, 8);
    assert_eq!(identity(&original).unwrap(), identity(&current).unwrap());
    let result = materialize(&db, &request, &plan, &selection(&plan, &original), &stop)
        .await
        .unwrap();
    assert!(!result.stale);
    assert_eq!(result.page.mode, knowledge::Mode::Semantic);
    value(
        &db,
        &reference.name,
        "meta/one",
        3,
        json!({"id":"one"}),
        Some(metadata("bodies/page", "/bodies/escaped~1id/body")),
    )
    .await;
    search::rebuild(&db).await.unwrap();
    assert!(!unchanged(&db, stored, &current, &stop).await.unwrap());
    let rewritten = source(&db, stored, "meta/one", &stop)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rewritten.revision, 3);
    assert_eq!(identity(&original).unwrap(), identity(&rewritten).unwrap());
    assert!(
        materialize(&db, &request, &plan, &selection(&plan, &original), &stop)
            .await
            .unwrap()
            .stale
    );
}

#[tokio::test]
async fn preserves_capture_across_package_updates() {
    let db = database().await;
    let old = package(&db, 'a', true).await;
    seed(&db).await;
    let stop = CancellationToken::new();
    let original = prepare(&db, &request(&old), &stop).await.unwrap();
    let latest = package(&db, 'b', true).await;
    let current = prepare(&db, &request(&latest), &stop).await.unwrap();
    assert_eq!(
        current.collections[0].collection.id,
        original.collections[0].collection.id
    );
    assert!(
        current.collections[0].collection.progress.generation
            > original.collections[0].collection.progress.generation
    );
    assert_eq!(
        prepare(&db, &request(&old), &stop)
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::RevisionConflict
    );
    let mut admitted = request(&old);
    admitted.plugin.as_mut().unwrap().turn = Some(TurnId::new());
    let admitted = prepare(&db, &admitted, &stop).await.unwrap();
    assert_eq!(admitted.package, old);
    assert_eq!(
        admitted.collections[0].capture.package.as_ref(),
        Some(&latest)
    );
}

#[tokio::test]
async fn preserves_raw_only_lexical_query() {
    let db = database().await;
    let reference = package(&db, 'a', true).await;
    seed(&db).await;
    let mut request = request(&reference);
    let Command::SearchPluginValues(query) = &mut request.command else {
        unreachable!()
    };
    query.terms.clear();
    query.query = Some("vector keyword".into());
    let plan = prepare(&db, &request, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(plan.keyword.len(), 2);
    let Command::SearchPluginValues(query) = lexical(&request).command else {
        unreachable!()
    };
    assert_eq!(query.query.as_deref(), Some("vector keyword"));
    assert_eq!(query.terms, ["vector keyword"]);
}

#[tokio::test]
async fn bounds_projection_and_resume_pages() {
    let db = database().await;
    let reference = package(&db, 'a', true).await;
    for (number, key) in ["meta/one", "meta/two", "meta/three"]
        .into_iter()
        .enumerate()
    {
        value(
            &db,
            &reference.name,
            key,
            number as u64 + 1,
            json!({"payload":"x".repeat(240*1024)}),
            Some(metadata("bodies/page", "/body")),
        )
        .await;
    }
    value(
        &db,
        &reference.name,
        "bodies/page",
        1,
        json!({"body":"Raw vector body"}),
        None,
    )
    .await;
    search::rebuild(&db).await.unwrap();
    let mut request = request(&reference);
    let Command::SearchPluginValues(query) = &mut request.command else {
        unreachable!()
    };
    query.mode = knowledge::Mode::Keyword;
    let stop = CancellationToken::new();
    let plan = prepare(&db, &request, &stop).await.unwrap();
    let selected = Selection {
        keys: plan.keyword.iter().map(|(key, _)| key.clone()).collect(),
        stamps: vec![],
        mode: knowledge::Mode::Keyword,
        indexing: false,
    };
    let first = materialize(&db, &request, &plan, &selected, &stop)
        .await
        .unwrap();
    assert_eq!(first.page.entries.len(), 2);
    assert_eq!(first.page.next, Some(2));
    let bytes = serde_json::to_vec(&sailry_protocol::plugin::transaction::sdk::output(
        Output::PluginSearch(first.page.clone()),
    ))
    .unwrap();
    assert!(bytes.len() <= plugin::storage::MAX_SEARCH_BYTES);
    let Command::SearchPluginValues(query) = &mut request.command else {
        unreachable!()
    };
    query.offset = first.page.next.unwrap();
    let last = materialize(&db, &request, &plan, &selected, &stop)
        .await
        .unwrap();
    assert_eq!(last.page.entries.len(), 1);
    assert_eq!(last.page.next, None);
    assert!(last.page.entries[0].value["visible"].as_bool().unwrap());
}

#[tokio::test]
async fn rejects_foreign_plans_and_stamps() {
    let db = database().await;
    let reference = package(&db, 'a', true).await;
    seed(&db).await;
    let request = request(&reference);
    let stop = CancellationToken::new();
    let plan = prepare(&db, &request, &stop).await.unwrap();
    let raw = sources(&db, &plan.collections[0], None, 16, &stop)
        .await
        .unwrap()
        .records
        .remove(0);
    let mut selected = selection(&plan, &raw);
    selected.stamps[0].collection = KnowledgeId::new();
    assert_eq!(
        materialize(&db, &request, &plan, &selected, &stop)
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::PermissionDenied
    );
    let mut foreign = plan.clone();
    foreign.package.name = "foreign-memory".into();
    assert_eq!(
        materialize(&db, &request, &foreign, &selection(&plan, &raw), &stop)
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::PermissionDenied
    );
}

#[tokio::test]
async fn requires_captured_plugin_provenance() {
    let db = database().await;
    let reference = package(&db, 'a', true).await;
    seed(&db).await;
    let mut request = request(&reference);
    request.plugin = None;
    assert_eq!(
        prepare(&db, &request, &CancellationToken::new())
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::PermissionDenied
    );
}

#[tokio::test]
async fn rejects_source_changes_without_revision_updates() {
    for metadata_changed in [false, true] {
        let db = database().await;
        let reference = package(&db, 'a', true).await;
        seed(&db).await;
        let request = request(&reference);
        let stop = CancellationToken::new();
        let plan = prepare(&db, &request, &stop).await.unwrap();
        let stored = &plan.collections[0];
        let captured = sources(&db, stored, None, 16, &stop)
            .await
            .unwrap()
            .records
            .remove(0);
        if metadata_changed {
            let mut index = metadata("bodies/page", "/bodies/escaped~1id/body");
            index.tags = vec!["changed".into()];
            value(
                &db,
                &reference.name,
                "meta/one",
                captured.revision,
                json!({"id":"one"}),
                Some(index),
            )
            .await;
            search::rebuild(&db).await.unwrap();
        } else {
            value(
                &db,
                &reference.name,
                "bodies/page",
                captured.source_revision,
                json!({"bodies":{"escaped/id":{"body":"Replaced without receipt"}}}),
                None,
            )
            .await;
        }
        assert!(!unchanged(&db, stored, &captured, &stop).await.unwrap());
        let current = source(&db, stored, &captured.key, &stop)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(current.revision, captured.revision);
        assert_eq!(current.source_revision, captured.source_revision);
        assert_ne!(identity(&current).unwrap(), identity(&captured).unwrap());
        let result = materialize(&db, &request, &plan, &selection(&plan, &captured), &stop)
            .await
            .unwrap();
        assert!(result.stale);
        assert!(result.page.indexing);
        assert_eq!(result.page.mode, knowledge::Mode::Keyword);
        assert_eq!(result.page.entries.len(), 2);
    }
}

#[tokio::test]
async fn bounds_source_pages_and_checks_cancellation() {
    let db = database().await;
    let reference = package(&db, 'a', true).await;
    seed(&db).await;
    for number in 0..3 {
        value(
            &db,
            &reference.name,
            &format!("meta/{number}"),
            1,
            json!({"id":number}),
            Some(metadata("bodies/page", "/bodies/escaped~1id/body")),
        )
        .await;
    }
    search::rebuild(&db).await.unwrap();
    let stop = CancellationToken::new();
    let plan = prepare(&db, &request(&reference), &stop).await.unwrap();
    let stored = &plan.collections[0];
    let first = sources(&db, stored, None, 2, &stop).await.unwrap();
    assert_eq!(first.records.len(), 2);
    assert_eq!(first.after.as_deref(), Some("meta/1"));
    let second = sources(&db, stored, first.after.as_deref(), 2, &stop)
        .await
        .unwrap();
    assert_eq!(second.records.len(), 2);
    assert_eq!(second.records[0].key, "meta/2");
    assert_eq!(second.records[1].key, "meta/one");
    assert_eq!(second.after.as_deref(), Some("meta/one"));
    let last = sources(&db, stored, second.after.as_deref(), 2, &stop)
        .await
        .unwrap();
    assert!(last.records.is_empty());
    assert!(last.after.is_none());
    for limit in [0, 17] {
        assert_eq!(
            sources(&db, stored, None, limit, &stop)
                .await
                .err()
                .unwrap()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    stop.cancel();
    assert_eq!(
        sources(&db, stored, None, 2, &stop)
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::Cancelled
    );
    assert_eq!(
        unchanged(&db, stored, &first.records[0], &stop)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
}
