use super::*;

impl Fixture {
    fn files(&self, files: &[(&str, &str)]) {
        for (path, text) in files {
            let path = self.root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }

    async fn graph(&self, query: Graph) -> GraphPage {
        self.service
            .graph(
                self.id,
                self.root.clone(),
                &query,
                &CancellationToken::new(),
            )
            .await
            .unwrap()
    }
}

fn query(view: View, name: Option<&str>, paths: &[&str]) -> Graph {
    Graph {
        name: name.map(str::to_owned),
        paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        ..Graph::new(view)
    }
}

fn sites(references: &[Reference]) -> Vec<(&str, Option<&str>, Option<Confidence>, bool)> {
    references
        .iter()
        .map(|reference| {
            (
                reference.path.as_str(),
                reference.scope.as_ref().map(|scope| scope.name.as_str()),
                reference.confidence,
                reference.ambiguous,
            )
        })
        .collect()
}

fn chain(fixture: &Fixture) {
    fixture.files(&[
        ("chain/a.rs", "pub fn a() {}\n"),
        ("chain/b.rs", "pub fn b() {\n    a();\n}\n"),
        ("chain/c.rs", "pub fn c() {\n    b();\n}\n"),
        ("chain/d.rs", "pub fn d() {\n    c();\n}\n"),
        ("chain/e.rs", "pub fn e() {\n    d();\n}\n"),
    ]);
}

fn reached(page: &GraphPage) -> Vec<(&str, u8)> {
    page.impact
        .iter()
        .map(|item| (item.path.as_str(), item.depth))
        .collect()
}

#[tokio::test]
async fn resolves_callers_by_preference() {
    let fixture = Fixture::new();
    fixture.files(&[
        ("src/store.rs", "pub fn open() {}\npub fn helper() {}\n"),
        (
            "src/app.rs",
            "pub fn start() {\n    open();\n    helper();\n}\n\nfn open() {}\n",
        ),
        (
            "src/cli.rs",
            "use crate::store::open;\n\npub fn main() {\n    open();\n}\n",
        ),
        ("tools/open.rs", "pub fn open() {}\n"),
        ("src/util.rs", "pub fn shared() {}\n"),
        ("src/web.rs", "pub fn serve() {\n    shared();\n}\n"),
        ("other/shared.rs", "pub fn shared() {}\n"),
        ("docs/demo.rs", "fn demo() {\n    shared();\n}\n"),
    ]);
    let store = fixture
        .graph(query(View::Callers, Some("open"), &["src/store.rs"]))
        .await;
    assert_eq!(store.targets.len(), 1);
    assert_eq!(
        sites(&store.callers),
        [(
            "src/cli.rs",
            Some("main"),
            Some(Confidence::Imported),
            false
        )]
    );
    let local = fixture
        .graph(query(View::Callers, Some("open"), &["src/app.rs"]))
        .await;
    assert_eq!(
        sites(&local.callers),
        [("src/app.rs", Some("start"), Some(Confidence::Local), false)]
    );
    let util = fixture
        .graph(query(View::Callers, Some("shared"), &["src/util.rs"]))
        .await;
    assert_eq!(
        sites(&util.callers),
        [
            (
                "src/web.rs",
                Some("serve"),
                Some(Confidence::Directory),
                false
            ),
            (
                "docs/demo.rs",
                Some("demo"),
                Some(Confidence::Workspace),
                true
            ),
        ]
    );
    let demo = &util.callers[1];
    assert_eq!(demo.candidates, 2);
    assert_eq!((demo.line, demo.column, demo.count), (1, 4, 1));
    let other = fixture
        .graph(query(View::Callers, Some("shared"), &["other/shared.rs"]))
        .await;
    assert_eq!(
        sites(&other.callers),
        [(
            "docs/demo.rs",
            Some("demo"),
            Some(Confidence::Workspace),
            true
        )]
    );
    let callees = fixture
        .graph(query(View::Callees, Some("start"), &[]))
        .await;
    let resolved: Vec<_> = callees
        .callees
        .iter()
        .map(|reference| {
            (
                reference.name.as_str(),
                reference.confidence,
                reference.targets[0].path.as_str(),
            )
        })
        .collect();
    assert_eq!(
        resolved,
        [
            ("open", Some(Confidence::Local), "src/app.rs"),
            ("helper", Some(Confidence::Directory), "src/store.rs"),
        ]
    );
}

#[tokio::test]
async fn caps_impact_depth() {
    let fixture = Fixture::new();
    chain(&fixture);
    let full = fixture.graph(query(View::Impact, Some("a"), &[])).await;
    assert_eq!(
        reached(&full),
        [("chain/b.rs", 1), ("chain/c.rs", 2), ("chain/d.rs", 3)]
    );
    assert!(full.impact.iter().all(|item| {
        item.confidence == Confidence::Directory
            && !item.ambiguous
            && item.symbols.len() == 1
            && item.symbols[0].depth == item.depth
    }));
    let near = fixture
        .graph(Graph {
            depth: 1,
            ..query(View::Impact, Some("a"), &[])
        })
        .await;
    assert_eq!(reached(&near), [("chain/b.rs", 1)]);
    let changed = fixture
        .graph(query(View::Impact, None, &["chain/a.rs"]))
        .await;
    assert_eq!(reached(&changed), reached(&full));
    let paged = fixture
        .graph(Graph {
            limit: 1,
            offset: 1,
            ..query(View::Impact, Some("a"), &[])
        })
        .await;
    assert_eq!(reached(&paged), [("chain/c.rs", 2)]);
    assert_eq!((paged.total, paged.next_offset), (3, Some(2)));
}

#[tokio::test]
async fn follows_incremental_updates_and_deletions() {
    let fixture = Fixture::new();
    chain(&fixture);
    let before = fixture.graph(query(View::Callers, Some("b"), &[])).await;
    assert_eq!(sites(&before.callers)[0].0, "chain/c.rs");
    fixture.write("chain/c.rs", "pub fn c() {}\n");
    fixture.service.changed(fixture.id, Some("chain/c.rs"));
    let updated = fixture.graph(query(View::Callers, Some("b"), &[])).await;
    assert!(updated.callers.is_empty(), "{:?}", updated.callers);
    std::fs::remove_file(fixture.root.join("chain/a.rs")).unwrap();
    fixture.service.changed(fixture.id, None);
    let removed = fixture.graph(query(View::Callers, Some("a"), &[])).await;
    assert!(removed.targets.is_empty());
    let callees = fixture.graph(query(View::Callees, Some("b"), &[])).await;
    assert_eq!(callees.callees.len(), 1);
    assert_eq!(callees.callees[0].confidence, None);
    assert!(callees.callees[0].targets.is_empty());
}

#[tokio::test]
async fn ranks_maps_deterministically() {
    let fixture = Fixture::new();
    chain(&fixture);
    let first = fixture.graph(Graph::new(View::Map)).await;
    let second = fixture.graph(Graph::new(View::Map)).await;
    assert_eq!(first.map, second.map);
    assert_eq!(first.total, 5);
    assert_eq!(first.map[0].path, "chain/a.rs");
    assert_eq!(first.map[0].score, 1000);
    assert_eq!(first.map[0].symbols[0].name, "a");
    assert_eq!(first.map[0].symbols[0].references, 1);
    let focused = fixture.graph(query(View::Map, None, &["chain/e.rs"])).await;
    assert_eq!(focused.map[0].path, "chain/e.rs");
    let bounded = fixture
        .graph(Graph {
            limit: 2,
            ..Graph::new(View::Map)
        })
        .await;
    assert_eq!(bounded.map, first.map[..2]);
    assert_eq!(bounded.next_offset, Some(2));
}

#[tokio::test]
async fn combines_context_for_one_definition() {
    let fixture = Fixture::new();
    chain(&fixture);
    let context = fixture
        .graph(Graph {
            line: Some(1),
            ..query(View::Context, None, &["chain/c.rs"])
        })
        .await;
    assert_eq!(context.targets.len(), 1);
    assert_eq!(context.targets[0].name, "c");
    assert_eq!(sites(&context.callers)[0].0, "chain/d.rs");
    assert_eq!(context.callees[0].name, "b");
    assert_eq!(reached(&context), [("chain/d.rs", 1), ("chain/e.rs", 2)]);
    assert_eq!(context.total, 2);
}

#[tokio::test]
async fn rejects_unbounded_queries() {
    let fixture = Fixture::new();
    for invalid in [
        Graph::new(View::Callers),
        Graph::new(View::Impact),
        Graph {
            depth: 4,
            ..query(View::Impact, Some("a"), &[])
        },
        Graph {
            limit: MAX_GRAPH_ITEMS + 1,
            ..Graph::new(View::Map)
        },
        Graph {
            line: Some(1),
            ..query(View::Callers, None, &["a.rs", "b.rs"])
        },
        query(View::Map, None, &["../outside"]),
    ] {
        assert_eq!(
            fixture
                .service
                .graph(
                    fixture.id,
                    fixture.root.clone(),
                    &invalid,
                    &CancellationToken::new()
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest,
            "{invalid:?}"
        );
    }
}
