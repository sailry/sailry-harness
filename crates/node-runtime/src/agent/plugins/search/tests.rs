use super::*;

fn catalog() -> Search {
    Search {
        session: "session".into(),
        stop: CancellationToken::new(),
        initial: BTreeSet::new(),
        entries: vec![
            Entry {
                name: "query_database".into(),
                description: "Read database rows".into(),
                context: vec!["Database context".into()],
            },
            Entry {
                name: "run_ssh".into(),
                description: "Run a command on a remote host".into(),
                context: vec!["SSH context".into()],
            },
        ],
    }
}

#[test]
fn adds_matches_without_removing_loaded_tools() {
    let catalog = catalog();
    let mut loaded = BTreeSet::new();
    let first = catalog.output(
        Arguments {
            query: "database".into(),
            limit: None,
        },
        &mut loaded,
    );
    assert_eq!(first["tools"][0]["name"], "query_database");
    assert_eq!(loaded, ["query_database".into()].into());
    let second = catalog.output(
        Arguments {
            query: "remote host".into(),
            limit: Some(1),
        },
        &mut loaded,
    );
    assert_eq!(second["tools"][0]["name"], "run_ssh");
    assert_eq!(loaded, ["query_database".into(), "run_ssh".into()].into());
    assert_eq!(second["sailry_context"], "SSH context");
    let repeated = catalog.output(
        Arguments {
            query: "database".into(),
            limit: None,
        },
        &mut loaded,
    );
    assert_eq!(repeated["sailry_context"], "");
    assert_eq!(loaded.len(), 2);
}

#[test]
fn unmatched_queries_leave_selection_intact() {
    let catalog = catalog();
    let mut loaded = ["run_ssh".into()].into();
    for query in ["", "unknown"] {
        let result = catalog.output(
            Arguments {
                query: query.into(),
                limit: None,
            },
            &mut loaded,
        );
        assert_eq!(result["tools"], json!([]));
        assert_eq!(loaded, ["run_ssh".into()].into());
    }
}

#[test]
fn shared_context_is_injected_once() {
    let mut catalog = catalog();
    catalog.entries.push(Entry {
        name: "write_database".into(),
        description: "Write database rows".into(),
        context: vec!["Database context".into()],
    });
    let mut loaded = BTreeSet::new();
    let first = catalog.output(
        Arguments {
            query: "query_database".into(),
            limit: Some(1),
        },
        &mut loaded,
    );
    assert_eq!(first["sailry_context"], "Database context");
    let second = catalog.output(
        Arguments {
            query: "write".into(),
            limit: Some(1),
        },
        &mut loaded,
    );
    assert_eq!(second["sailry_context"], "");
    catalog.initial.insert("SSH context".into());
    let already_mounted = catalog.output(
        Arguments {
            query: "ssh".into(),
            limit: Some(1),
        },
        &mut loaded,
    );
    assert_eq!(already_mounted["sailry_context"], "");
}
