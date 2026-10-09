use super::*;
use sailry_protocol::{WorktreeId, plugin::Reference};

fn context() -> plugin::Context {
    plugin::Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package: Reference {
            name: "notes-fixture".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
        worktree: Some(WorktreeId::new()),
        session: None,
    }
}

#[test]
fn captures_sources_without_caller_ownership() {
    let context = context();
    let input = json!({"name":"Notes","source":{"kind":"worktree","path":"notes"}});
    let Command::CreateKnowledgeCollection { definition, plugin } =
        command(Operation::KnowledgeCreate, input.clone(), &context).unwrap()
    else {
        panic!("collection command expected")
    };
    assert_eq!(
        definition.source,
        knowledge::Source::Worktree {
            worktree: context.worktree.unwrap(),
            path: "notes".into()
        }
    );
    assert!(definition.shared_with.is_empty());
    assert_eq!(plugin, None);
    for field in ["owner", "plugin", "shared_with"] {
        let mut injected = input.clone();
        injected[field] = json!("other");
        assert!(command(Operation::KnowledgeCreate, injected, &context).is_err());
    }
    let mut missing = context;
    missing.worktree = None;
    assert!(command(Operation::KnowledgeCreate, input, &missing).is_err());
    assert!(command(Operation::KnowledgeCreate, json!({"name":"Notes","source":{"kind":"storage","scope":"conversation","prefix":"notes/"}}), &missing).is_ok());
}

#[test]
fn preserves_queries_and_revision_checks() {
    let context = context();
    let raw = "  原始 query  ";
    let Command::QueryKnowledge(query) =
        command(Operation::KnowledgeQuery, json!({"query":raw}), &context).unwrap()
    else {
        panic!("query command expected")
    };
    assert_eq!(query.query, raw);
    assert_eq!(query.mode, knowledge::Mode::Hybrid);
    assert_eq!(query.limit, knowledge::DEFAULT_LIMIT);
    for args in [
        json!({"query":""}),
        json!({"query":"text","owner":"other"}),
        json!({"query":"text","limit":knowledge::MAX_LIMIT+1}),
    ] {
        assert!(command(Operation::KnowledgeQuery, args, &context).is_err());
    }
    assert!(command(Operation::KnowledgeCollections, json!({}), &context).is_ok());
    assert!(
        command(
            Operation::KnowledgeCollections,
            json!({"owner":"other"}),
            &context
        )
        .is_err()
    );
    let id = KnowledgeId::new();
    for operation in [Operation::KnowledgeRebuild, Operation::KnowledgeRemove] {
        assert!(command(operation, json!({"id":id,"expected_revision":0}), &context).is_err());
        let actual = command(operation, json!({"id":id,"expected_revision":9}), &context).unwrap();
        assert!(actual.durable());
        assert_eq!(
            parameters(operation)["required"],
            json!(["id", "expected_revision"])
        );
    }
}

#[test]
fn accepts_registered_model_bindings() {
    let context = context();
    let model = knowledge::Model::Registered {
        id: sailry_protocol::ModelId::new(),
    };
    let encoded = serde_json::to_value(model).unwrap();
    let knowledge::Model::Registered { id } = model else {
        unreachable!()
    };
    assert_eq!(encoded, json!({"registered":{"id":id}}));
    let schema = parameters(Operation::KnowledgeCreate);
    let validator = jsonschema::draft202012::options().build(&schema).unwrap();
    let input = json!({
        "name":"Notes",
        "source":{"kind":"worktree","path":"notes"},
        "model":encoded
    });
    assert!(validator.is_valid(&input));
    let Command::CreateKnowledgeCollection { definition, .. } =
        command(Operation::KnowledgeCreate, input.clone(), &context).unwrap()
    else {
        panic!("collection command expected")
    };
    assert_eq!(definition.model, Some(model));
    for invalid in [
        json!({"registered":{}}),
        json!({"registered":{"id":7}}),
        json!({"registered":{"id":id,"owner":"other"}}),
        json!({"registered":{"id":id},"owner":"other"}),
    ] {
        let mut input = input.clone();
        input["model"] = invalid;
        assert!(!validator.is_valid(&input));
    }
}

#[test]
fn reads_verified_citation_context() {
    let operation = Operation::KnowledgeRead;
    let arguments =
        json!({"collection":KnowledgeId::new(),"chunk":"a".repeat(64),"revision":"b".repeat(64)});
    let schema = parameters(operation);
    let validator = jsonschema::draft202012::options().build(&schema).unwrap();
    assert!(validator.is_valid(&arguments));
    let Command::ReadKnowledgeCitation(input) =
        command(operation, arguments.clone(), &context()).unwrap()
    else {
        panic!("citation command expected")
    };
    assert!(input.context);
    assert_eq!(input.offset, 0);
    assert_eq!(input.limit, 16 * 1024);
    assert!(!Command::ReadKnowledgeCitation(input).durable());
    let mut limited = arguments.clone();
    limited["context"] = json!(false);
    let Command::ReadKnowledgeCitation(input) = command(operation, limited, &context()).unwrap()
    else {
        panic!("citation command expected")
    };
    assert!(!input.context);
    for (key, value) in [
        ("chunk", json!("invalid")),
        ("limit", json!(65537)),
        ("offset", json!(4294967296_u64)),
        ("path", json!("/private/file")),
        ("context", json!("yes")),
    ] {
        let mut invalid = arguments.clone();
        invalid[key] = value;
        assert!(!validator.is_valid(&invalid));
        assert!(command(operation, invalid, &context()).is_err());
    }
}
