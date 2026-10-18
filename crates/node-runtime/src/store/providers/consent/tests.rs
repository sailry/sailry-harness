use super::*;
use sailry_protocol::conversation::consent::{Approval, Scopes};

fn package(digest: char) -> plugin::Reference {
    plugin::Reference {
        name: "example".into(),
        digest: digest.to_string().repeat(64),
        settings_revision: 0,
    }
}

#[test]
fn preserves_scope_across_same_origin_patches() {
    let origin = Origin::Git {
        repository: "https://example.invalid/plugin".into(),
        path: "example".into(),
    };
    let approval = Approval {
        package: package('a'),
        origin: Some(origin.clone()),
        scopes: Scopes {
            messages: true,
            credentials: false,
        },
    };
    assert!(
        captured(
            Some(&approval),
            &package('b'),
            Some(&origin),
            Scope::Messages
        )
        .is_ok()
    );
    assert_eq!(
        captured(
            Some(&approval),
            &package('b'),
            Some(&origin),
            Scope::Credentials
        )
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        captured(
            Some(&approval),
            &package('b'),
            Some(&Origin::Bundled),
            Scope::Messages
        )
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    let other = plugin::Reference {
        name: "other".into(),
        ..package('a')
    };
    assert_eq!(
        captured(Some(&approval), &other, Some(&origin), Scope::Messages)
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
}

#[test]
fn keeps_imports_bound_to_their_immutable_source() {
    let old = Origin::Imported {
        digest: "a".repeat(64),
    };
    let new = Origin::Imported {
        digest: "b".repeat(64),
    };
    let approval = Approval {
        package: package('a'),
        origin: Some(old.clone()),
        scopes: Scopes {
            messages: true,
            credentials: true,
        },
    };
    assert!(
        captured(
            Some(&approval),
            &package('a'),
            Some(&old),
            Scope::Credentials
        )
        .is_ok()
    );
    assert_eq!(
        captured(Some(&approval), &package('b'), Some(&new), Scope::Messages)
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        captured(Some(&approval), &package('a'), None, Scope::Messages)
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
}

#[test]
fn uses_only_the_admitted_approval() {
    let origin = Origin::Bundled;
    let original = Approval {
        package: package('a'),
        origin: Some(origin.clone()),
        scopes: Scopes {
            messages: true,
            credentials: false,
        },
    };
    let admitted = original.clone();
    let mut current = original;
    current.scopes.credentials = true;
    assert!(
        captured(
            Some(&current),
            &package('a'),
            Some(&origin),
            Scope::Credentials
        )
        .is_ok()
    );
    assert_eq!(
        captured(
            Some(&admitted),
            &package('a'),
            Some(&origin),
            Scope::Credentials
        )
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    current.scopes.messages = false;
    assert!(
        captured(
            Some(&admitted),
            &package('a'),
            Some(&origin),
            Scope::Messages
        )
        .is_ok()
    );
    assert_eq!(
        captured(
            Some(&current),
            &package('a'),
            Some(&origin),
            Scope::Messages
        )
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
}

#[test]
fn stamps_only_the_displayed_current_installation() {
    use crate::store::database::{Database, encode};
    use sailry_protocol::{NodeId, ProviderId};
    use serde_json::json;
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .canonicalize()
        .unwrap()
        .join("node.sqlite3");
    let database = Database::open(&path, NodeId([19; 32]), None).unwrap();
    let db = &database.connection;
    let mut info: plugin::Info = serde_json::from_value(json!({"summary":{"name":"example","revision":1,"digest":"a".repeat(64),"settings_revision":0,"enabled":true,"version":"0.1.0","description":null},"icon":null,"origin":{"kind":"bundled"},"extension":null,"settings":null})).unwrap();
    let install = |info: &plugin::Info| {
        db.execute("INSERT OR REPLACE INTO plugins(name,revision,settings_revision,installed,body) VALUES('example',?1,0,1,?2)",rusqlite::params![i64::try_from(info.summary.revision).unwrap(),encode(info).unwrap()]).unwrap();
    };
    install(&info);
    let mut provider: Provider = serde_json::from_value(json!({"id":ProviderId::new(),"revision":1,"name":"Example","api":"chat_completions","authentication":"api_key","endpoint":"https://example.invalid","enabled":true,"models":[],"default_model":"","credential":null,"preset":{"source":{"kind":"package","data":{"name":"example"}},"id":"example","route":null}})).unwrap();
    provider.consent = Some(Box::new(Approval {
        package: package('a'),
        origin: None,
        scopes: Scopes {
            messages: true,
            credentials: false,
        },
    }));
    stamp(db, &mut provider, None).unwrap();
    assert_eq!(
        provider.consent.as_ref().unwrap().origin,
        Some(Origin::Bundled)
    );
    let previous = provider.clone();
    info.summary.digest = "b".repeat(64);
    info.summary.revision = 2;
    install(&info);
    stamp(db, &mut provider, Some(&previous)).unwrap();
    assert_eq!(provider.consent, previous.consent);
    let mut stale = previous.clone();
    stale.consent.as_mut().unwrap().scopes.credentials = true;
    assert_eq!(
        stamp(db, &mut stale, Some(&previous)).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    info.origin = Some(plugin::Origin::Archive);
    install(&info);
    assert_eq!(
        stamp(db, &mut provider, Some(&previous)).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    provider.consent = Some(Box::new(Approval {
        package: package('b'),
        origin: None,
        scopes: Scopes {
            messages: true,
            credentials: true,
        },
    }));
    stamp(db, &mut provider, Some(&previous)).unwrap();
    assert_eq!(
        provider.consent.unwrap().origin,
        Some(Origin::Imported {
            digest: "b".repeat(64)
        })
    );
}
