use super::*;
use sailry_link::Response;
use sailry_protocol::{Event, NodeId, plugin};
use tokio::sync::{broadcast, oneshot};

struct Fixture {
    _directory: tempfile::TempDir,
    database: Database,
    events: broadcast::Sender<sailry_protocol::EventEnvelope>,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().canonicalize().unwrap();
        let mut database =
            Database::open(&profile.join("node.sqlite3"), NodeId([1; 32]), None).unwrap();
        let host = crate::plugins::Host::new(Some(profile));
        let mut result = Ok(Output::Plugin(host.install_bundled("code").unwrap()));
        super::super::plugins::finish(
            &database.connection,
            &Command::InstallBundledPlugin {
                name: "code".into(),
                expected_revision: 0,
            },
            &mut result,
        )
        .unwrap();
        result.unwrap();
        initialize(&mut database.connection).unwrap();
        Self {
            _directory: directory,
            database,
            events: broadcast::channel(16).0,
        }
    }

    fn execute(&mut self, request: Request) -> Response {
        let (reply, response) = oneshot::channel();
        assert!(
            self.database
                .dispatch(self.database.node, request, reply, &self.events)
                .is_none()
        );
        response
            .blocking_recv()
            .unwrap()
            .unwrap()
            .completion
            .blocking_recv()
            .unwrap()
    }

    fn package(&self) -> plugin::Info {
        super::super::plugins::get(&self.database.connection, "code")
            .unwrap()
            .unwrap()
    }

    fn events(&self) -> Vec<Event> {
        let mut query = self
            .database
            .connection
            .prepare("SELECT body FROM events ORDER BY cursor")
            .unwrap();
        query
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .unwrap()
            .map(|body| serde_json::from_slice(&body.unwrap()).unwrap())
            .collect()
    }
}

#[test]
fn rolls_back_both_switches_on_failure() {
    let mut fixture = Fixture::new();
    let original = fixture.package();
    let before = settings(&fixture.database.connection).unwrap();
    let events = fixture.events();
    fixture.database.connection.execute_batch("CREATE TRIGGER reject_code BEFORE INSERT ON code_settings BEGIN SELECT RAISE(ABORT,'fixture code failure'); END;").unwrap();
    let request = Request::new(
        fixture.database.node,
        Command::SetPluginEnabled {
            name: "code".into(),
            expected_revision: original.summary.revision,
            enabled: false,
        },
    );
    let failed = fixture.execute(request.clone()).unwrap_err();
    assert_eq!(fixture.package(), original);
    assert_eq!(settings(&fixture.database.connection).unwrap(), before);
    assert_eq!(fixture.events(), events);
    assert_eq!(fixture.execute(request).unwrap_err(), failed);
    assert_eq!(fixture.events(), events);
    fixture
        .database
        .connection
        .execute_batch("DROP TRIGGER reject_code")
        .unwrap();
    let mut changed = before;
    changed.after_edit = false;
    let request = Request::new(
        fixture.database.node,
        Command::SaveCodeSettings {
            expected_revision: changed.revision,
            settings: changed,
        },
    );
    fixture.execute(request).unwrap();
    let before = settings(&fixture.database.connection).unwrap();
    let events = fixture.events();
    fixture.database.connection.execute_batch("CREATE TRIGGER reject_plugin BEFORE UPDATE ON plugins BEGIN SELECT RAISE(ABORT,'fixture plugin failure'); END;").unwrap();
    let mut changed = before.clone();
    changed.enabled = false;
    let request = Request::new(
        fixture.database.node,
        Command::SaveCodeSettings {
            expected_revision: changed.revision,
            settings: changed,
        },
    );
    let failed = fixture.execute(request.clone()).unwrap_err();
    assert_eq!(fixture.package(), original);
    assert_eq!(settings(&fixture.database.connection).unwrap(), before);
    assert_eq!(fixture.events(), events);
    assert_eq!(fixture.execute(request).unwrap_err(), failed);
    assert_eq!(fixture.events(), events);
    fixture
        .database
        .connection
        .execute_batch("DROP TRIGGER reject_plugin")
        .unwrap();
    let mut changed = before;
    changed.enabled = false;
    let request = Request::new(
        fixture.database.node,
        Command::SaveCodeSettings {
            expected_revision: changed.revision,
            settings: changed,
        },
    );
    fixture.execute(request).unwrap();
    let package = fixture.package();
    assert!(!package.summary.enabled);
    assert_eq!(
        fixture.events().last(),
        Some(&Event::PluginChanged(package.summary))
    );
}

#[test]
fn initializes_declared_service_owner() {
    let mut fixture = Fixture::new();
    let mut package = fixture.package();
    let mut owner = package.clone();
    owner.summary.name = "language-tools".into();
    owner.summary.enabled = false;
    package.extension.as_mut().unwrap().enabled_service = None;
    fixture
        .database
        .connection
        .execute(
            "UPDATE plugins SET body=?1 WHERE name=?2",
            params![encode(&package).unwrap(), package.summary.name],
        )
        .unwrap();
    fixture
        .database
        .connection
        .execute(
            "INSERT INTO plugins(name,revision,installed,body) VALUES(?1,?2,1,?3)",
            params![
                owner.summary.name,
                owner.summary.revision as i64,
                encode(&owner).unwrap()
            ],
        )
        .unwrap();
    let initialized = initialize(&mut fixture.database.connection).unwrap();
    assert!(!initialized.enabled);
    assert_eq!(fixture.package(), package);
    assert_eq!(
        initialize(&mut fixture.database.connection).unwrap(),
        initialized
    );
    owner.summary.enabled = true;
    package.summary.enabled = false;
    for updated in [&owner, &package] {
        fixture
            .database
            .connection
            .execute(
                "UPDATE plugins SET body=?1 WHERE name=?2",
                params![encode(updated).unwrap(), updated.summary.name],
            )
            .unwrap();
    }
    assert!(
        initialize(&mut fixture.database.connection)
            .unwrap()
            .enabled
    );
}

#[test]
fn preserves_removed_package_choice_at_startup() {
    let mut fixture = Fixture::new();
    let original = fixture.package();
    let mut code = settings(&fixture.database.connection).unwrap();
    code.after_edit = false;
    let expected = code.revision;
    save(&fixture.database.connection, &code, expected).unwrap();
    fixture
        .execute(Request::new(
            fixture.database.node,
            Command::RemovePlugin {
                name: "code".into(),
                expected_revision: original.summary.revision,
            },
        ))
        .unwrap();
    let before = settings(&fixture.database.connection).unwrap();
    let mut inconsistent = before.clone();
    inconsistent.enabled = true;
    save(
        &fixture.database.connection,
        &inconsistent,
        inconsistent.revision,
    )
    .unwrap();
    let initialized = initialize(&mut fixture.database.connection).unwrap();
    assert!(!initialized.enabled);
    assert!(!initialized.after_edit);
    assert_eq!(initialized.revision, before.revision + 2);
    assert!(
        super::super::plugins::get(&fixture.database.connection, "code")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        initialize(&mut fixture.database.connection).unwrap(),
        initialized
    );
    fixture
        .database
        .connection
        .execute("DELETE FROM plugins WHERE name='code'", [])
        .unwrap();
    assert_eq!(
        initialize(&mut fixture.database.connection).unwrap(),
        initialized
    );
}
