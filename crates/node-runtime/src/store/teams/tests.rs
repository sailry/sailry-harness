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
    assert_eq!(
        list(db).await.unwrap().as_slice(),
        std::slice::from_ref(&renamed)
    );
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

mod appearance {
    use super::*;
    use crate::store::roles;
    use sailry_protocol::role::Builtin;

    #[tokio::test]
    async fn seeds_builtin_roles() {
        let (_root, database) = open().await;
        let seeded = roles::list(&database.connection).await.unwrap();
        assert_eq!(seeded.len(), 2);
        for role in seeded {
            let builtin = Builtin::find(&role.key).unwrap();
            assert_eq!(role.appearance, Some(builtin.appearance()));
        }
    }

    #[tokio::test]
    async fn fills_missing_on_save() {
        let (_root, database) = open().await;
        let db = &database.connection;
        let mut role = roles::list(db).await.unwrap().remove(0);
        role.key = "review".into();
        role.id = RoleId::new();
        role.revision = 0;
        role.appearance = None;
        let role = roles::put(db, &role, 0).await.unwrap();
        let color = &role.appearance.as_ref().unwrap().color;
        assert_ne!(color, "none");
        assert_eq!(roles::get(db, role.id).await.unwrap(), Some(role.clone()));
        let team = put(db, &profile("review", vec![role.id]), 0).await.unwrap();
        assert!(team.appearance.is_some());
        assert_eq!(get(db, team.id).await.unwrap(), Some(team));
    }

    #[tokio::test]
    async fn fills_stored_rows_at_startup() {
        let (_root, database) = open().await;
        let db = &database.connection;
        assert!(!roles::needs_refresh(db).await.unwrap());
        let mut role = roles::list(db).await.unwrap().remove(0);
        role.appearance = None;
        db.execute(
            "UPDATE roles SET body=?2 WHERE id=?1",
            params![role.id.to_string(), encode(&role).unwrap()],
        )
        .await
        .unwrap();
        let mut team = put(db, &profile("review", vec![role.id]), 0).await.unwrap();
        team.appearance = None;
        write(db, &team).await.unwrap();
        assert!(roles::needs_refresh(db).await.unwrap());
        roles::refresh(db).await.unwrap();
        assert!(!roles::needs_refresh(db).await.unwrap());
        let filled = roles::get(db, role.id).await.unwrap().unwrap();
        let builtin = Builtin::find(&role.key).unwrap();
        assert_eq!(filled.appearance, Some(builtin.appearance()));
        assert_eq!(filled.revision, role.revision + 1);
        let stored = get(db, team.id).await.unwrap().unwrap();
        assert!(stored.appearance.is_some());
        assert_eq!(stored.revision, team.revision + 1);
    }
}
