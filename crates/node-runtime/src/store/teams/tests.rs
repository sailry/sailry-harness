use super::*;
use crate::store::database::Database;
use sailry_protocol::NodeId;

fn profile(key: &str, members: Vec<RoleId>) -> team::Profile {
    team::Profile {
        id: TeamId::new(),
        revision: 0,
        key: key.into(),
        name: "Review 中文 🙂".into(),
        appearance: None,
        description: String::new(),
        model: None,
        instructions: "Coordinate the reviewers".into(),
        members,
    }
}

async fn open() -> (tempfile::TempDir, Database) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap().join("node.turso");
    let database = Database::open(&path, NodeId([34; 32]), None).await.unwrap();
    (root, database)
}

#[tokio::test]
async fn checks_revisions_and_keys() {
    let (_root, database) = open().await;
    let db = &database.connection;
    let roles = crate::store::roles::list(db).await.unwrap();
    assert!(list(db).await.unwrap().is_empty());
    let saved = put(db, &profile("review", vec![roles[0].id]), 0)
        .await
        .unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(
        put(db, &saved, 0).await.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        put(db, &profile("review", vec![]), 0)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        put(db, &profile("other", vec![RoleId::new()]), 0)
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let mut renamed = saved.clone();
    renamed.key = "analysis".into();
    renamed.members = roles.iter().map(|role| role.id).collect();
    let renamed = put(db, &renamed, 1).await.unwrap();
    assert_eq!((renamed.id, renamed.revision), (saved.id, 2));
    assert_eq!(list(db).await.unwrap(), [renamed.clone()]);
    assert_eq!(
        remove(db, renamed.id, 1).await.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    remove(db, renamed.id, 2).await.unwrap();
    assert!(list(db).await.unwrap().is_empty());
    assert_eq!(
        remove(db, renamed.id, 2).await.unwrap_err().code,
        ErrorCode::NotFound
    );
}

#[tokio::test]
async fn prunes_removed_roles() {
    let (_root, database) = open().await;
    let db = &database.connection;
    let roles = crate::store::roles::list(db).await.unwrap();
    let (removed, kept) = (roles[0].clone(), roles[1].id);
    let both = put(db, &profile("both", vec![removed.id, kept]), 0)
        .await
        .unwrap();
    let other = put(db, &profile("other", vec![kept]), 0).await.unwrap();
    let pruned = crate::store::roles::remove(db, removed.id, removed.revision)
        .await
        .unwrap();
    let mut expected = both.clone();
    expected.members = vec![kept];
    expected.revision = 2;
    assert_eq!(pruned, [expected.clone()]);
    assert_eq!(list(db).await.unwrap(), [expected, other]);
}
