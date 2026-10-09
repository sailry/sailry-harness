use super::*;
use sailry_protocol::{NodeId, knowledge::Model};
use serde_json::json;

mod access;
mod configuration;
mod registry;
mod restore;

fn database() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../schema.sql")).unwrap();
    db
}

fn package(db: &Connection, name: &str, digest: char, actions: &[&str]) -> plugin::Reference {
    let summary = plugin::Summary {
        name: name.into(),
        revision: 1,
        digest: digest.to_string().repeat(64),
        settings_revision: 0,
        enabled: true,
        version: None,
        description: None,
    };
    let reference = summary.reference();
    let info = plugin::Info {
        summary,
        origin: None,
        icon: None,
        skill: None,
        mcp_source: None,
        extension: Some(
            serde_json::from_value(json!({"api_version":"v1","actions":actions})).unwrap(),
        ),
        skills: vec![],
        mcp: vec![],
        settings: None,
        issues: vec![],
    };
    db.execute(
        "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
        params![&reference.digest, name, encode(&info).unwrap()],
    )
    .unwrap();
    db.execute("INSERT INTO plugins(name,revision,installed,body) VALUES(?1,1,1,?2) ON CONFLICT(name) DO UPDATE SET body=excluded.body",
        params![name, encode(&info).unwrap()]).unwrap();
    reference
}

fn request(command: Command) -> Request {
    Request::new(NodeId([19; 32]), command)
}

fn captured(package: &plugin::Reference, command: Command) -> Request {
    request(command).with_plugin(plugin::Context {
        package: package.clone(),
        worktree: None,
        session: None,
        turn: None,
        invocation: None,
        surface: Default::default(),
    })
}

fn definition(source: Source) -> Definition {
    Definition {
        name: "Reference".into(),
        source,
        model: None,
        globs: vec![],
        tags: vec![],
        shared_with: vec![],
    }
}

fn storage() -> Definition {
    definition(Source::Storage {
        scope: Scope::Node,
        prefix: String::new(),
    })
}

fn create(db: &Connection, request: &Request) -> Collection {
    let (output, event) = execute(db, request).unwrap();
    assert!(matches!(event, Some(Event::KnowledgeCollectionsChanged)));
    let Output::KnowledgeCollection(collection) = output else {
        panic!("collection expected")
    };
    collection
}

fn insert(db: &Connection, package: &plugin::Reference, definition: Definition) -> Collection {
    create(
        db,
        &captured(
            package,
            Command::CreateKnowledgeCollection {
                definition,
                plugin: None,
            },
        ),
    )
}

fn worktree(db: &Connection, path: &std::path::Path) -> WorktreeId {
    let id = WorktreeId::new();
    db.execute(
        "INSERT INTO worktrees(id,path,main) VALUES(?1,?2,1)",
        params![id.to_string(), path.to_str().unwrap()],
    )
    .unwrap();
    id
}

fn raw(db: &Connection) -> Vec<(String, i64, i64, Vec<u8>)> {
    db.prepare("SELECT id,revision,generation,body FROM knowledge_collections ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}
