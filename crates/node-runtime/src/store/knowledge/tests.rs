use super::*;
use crate::store::local_models;
use sailry_protocol::NodeId;
use serde_json::json;

mod access;
mod configuration;
mod registry;
mod restore;

async fn database() -> Connection {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../schema.sql"))
        .await
        .unwrap();
    crate::store::local_models::initialize(&db).await.unwrap();
    db
}

async fn package(db: &Connection, name: &str, digest: char, actions: &[&str]) -> plugin::Reference {
    let summary = plugin::Summary {
        compatibility: Default::default(),
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
        versions: Vec::new(),
        release_notes: None,
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
        params![reference.digest.as_str(), name, encode(&info).unwrap()],
    )
    .await
    .unwrap();
    db.execute("INSERT INTO plugins(name,revision,installed,body) VALUES(?1,1,1,?2) ON CONFLICT(name) DO UPDATE SET body=excluded.body",
        params![name, encode(&info).unwrap()]).await.unwrap();
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

async fn create(db: &Connection, request: &Request) -> Collection {
    let (output, event) = execute(db, request).await.unwrap();
    assert!(matches!(event, Some(Event::KnowledgeCollectionsChanged)));
    let Output::KnowledgeCollection(collection) = output else {
        panic!("collection expected")
    };
    collection
}

async fn insert(
    db: &Connection,
    package: &plugin::Reference,
    definition: Definition,
) -> Collection {
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
    .await
}

async fn worktree(db: &Connection, path: &std::path::Path) -> WorktreeId {
    let id = WorktreeId::new();
    db.execute(
        "INSERT INTO worktrees(id,path,main) VALUES(?1,?2,1)",
        params![id.to_string(), path.to_str().unwrap()],
    )
    .await
    .unwrap();
    id
}

async fn raw(db: &Connection) -> Vec<(String, i64, i64, Vec<u8>)> {
    (async {
        let mut rows = db
            .prepare("SELECT id,revision,generation,body FROM knowledge_collections ORDER BY id")
            .await
            .unwrap()
            .query(())
            .await?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await? {
            let mapped: turso::Result<_> =
                (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))(&row);
            values.push(mapped);
        }
        Ok::<_, turso::Error>(values.into_iter())
    })
    .await
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

async fn execute(db: &Connection, request: &Request) -> Result<(Output, Option<Event>), Fault> {
    crate::store::commands::execute(db, request.target, request.target, None, request).await
}
async fn settings(
    db: &Connection,
    request: &Request,
) -> Result<sailry_protocol::models::Settings, Fault> {
    let (Output::LocalModelSettings(settings), _) = execute(db, request).await? else {
        panic!("local model settings expected")
    };
    Ok(settings)
}
fn select_model(settings: &mut sailry_protocol::models::Settings, model: Option<String>) {
    settings.choices.insert(
        sailry_protocol::models::Kind::Embedding,
        sailry_protocol::models::Choice {
            enabled: model.is_some(),
            model,
        },
    );
}
