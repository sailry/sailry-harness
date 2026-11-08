use super::*;
use sailry_protocol::plugin::mcp::RuntimeState;

#[path = "../../../../tests/support/mcp.rs"]
#[allow(dead_code)]
mod peer;

struct Fixture {
    directory: tempfile::TempDir,
    node: crate::Node,
    pool: Pool,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("version.txt"), "pooled").unwrap();
        let node = crate::Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let pool = Pool::default();
        pool.observe(&package());
        Self {
            directory,
            node,
            pool,
        }
    }

    fn launch(&self, token: &str) -> Launch {
        let path = self.directory.path().canonicalize().unwrap();
        let root = cap_std::fs::Dir::open_ambient_dir(&path, cap_std::ambient_authority()).unwrap();
        Launch::Stdio {
            command: std::env::current_exe().unwrap(),
            args: vec![
                "--exact".into(),
                "agent::mcp::pool::tests::peer::stdio_peer".into(),
                "--ignored".into(),
                "--nocapture".into(),
                "--test-threads=1".into(),
            ],
            env: BTreeMap::from([
                ("PLUGIN_ROOT".into(), path.to_str().unwrap().into()),
                ("PLUGIN_DATA".into(), path.to_str().unwrap().into()),
                ("SAILRY_MCP_FIXTURE".into(), "1".into()),
                ("SAILRY_MCP_MODE".into(), "normal".into()),
                ("SAILRY_MCP_TEXT".into(), "fixture".into()),
                ("PRIVATE_TOKEN".into(), token.into()),
            ]),
            cwd: path,
            roots: (root.try_clone().unwrap(), root),
            settings: None,
            python: None,
            computer: false,
        }
    }

    async fn lease(&self, server: &str) -> Result<Arc<Connected>, Fault> {
        self.pool
            .lease(
                &config(server),
                self.launch("unchanged"),
                None,
                &self.node.ingress,
                &CancellationToken::new(),
                true,
            )
            .await
            .map(|(connection, _)| connection)
    }

    async fn close(self) {
        self.pool.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

fn config(server: &str) -> Configured {
    Configured {
        plugin: "example".into(),
        reference: package().reference(),
        name: server.into(),
        launch: Err(unavailable()),
        options: Default::default(),
        tools: None,
    }
}

fn info() -> sailry_protocol::plugin::Info {
    serde_json::from_value(json!({
        "summary": package(),
        "mcp":[{"name":"native","transport":"stdio"}],
    }))
    .unwrap()
}

#[tokio::test]
async fn expires_idle_without_changing_status() {
    let fixture = Fixture::new().await;
    let connection = fixture.lease("native").await.unwrap();
    let transport = connection.transport.clone();
    let stopped = transport.lock().await.stopped();
    let attempt = fixture.pool.begin(&package().reference(), "native");
    fixture.pool.finish(attempt, Ok(4));
    *connection.used.lock().unwrap() = Instant::now() - IDLE_TTL;
    fixture.pool.expire().await.unwrap();
    assert_eq!(fixture.pool.entries.lock().unwrap().len(), 1);
    drop(connection);
    fixture.pool.expire().await.unwrap();
    assert!(fixture.pool.entries.lock().unwrap().is_empty());
    let mut projected = info();
    fixture.pool.project(&mut projected);
    assert_eq!(
        projected.mcp[0].runtime.as_ref().unwrap().state,
        RuntimeState::Connected
    );
    tokio::time::timeout(Duration::from_secs(10), stopped.cancelled())
        .await
        .unwrap();
    fixture.close().await;
}

#[tokio::test]
async fn bounds_capacity_and_retires_idle() {
    let fixture = Fixture::new().await;
    let busy = fixture.lease("busy").await.unwrap();
    let idle = fixture.lease("idle").await.unwrap();
    let stopped = idle.transport.lock().await.stopped();
    {
        let mut entries = fixture.pool.entries.lock().unwrap();
        for index in 0..MAX_CONNECTIONS - 2 {
            let key = Key::new(
                &config(&format!("busy{index}")),
                &fixture.launch("unchanged"),
                None,
            )
            .unwrap();
            entries.insert(
                key,
                Arc::new(Entry {
                    connection: OnceCell::new_with(Some(Ok(busy.clone()))),
                    ready: Notify::new(),
                    stop: fixture.pool.stopped.child_token(),
                    created: Instant::now(),
                }),
            );
        }
    }
    let settled = fixture.pool.begin(&package().reference(), "next");
    fixture.pool.finish(settled, Ok(7));
    let mut projected = info();
    projected.mcp[0].name = "next".into();
    fixture.pool.project(&mut projected);
    let previous = projected.mcp[0].runtime.clone();
    let attempt = fixture.pool.begin(&package().reference(), "next");
    let error = fixture.lease("next").await.err().unwrap();
    assert_eq!(error.code, ErrorCode::Busy);
    fixture.pool.finish(attempt, Err(error));
    fixture.pool.project(&mut projected);
    assert_eq!(projected.mcp[0].runtime, previous);
    assert!(!stopped.is_cancelled());
    drop(idle);
    let next = fixture.lease("next").await.unwrap();
    let entries = fixture.pool.entries.lock().unwrap();
    assert_eq!(entries.len(), MAX_CONNECTIONS);
    assert!(entries.keys().all(|key| key.server != "idle"));
    drop(entries);
    tokio::time::timeout(Duration::from_secs(10), stopped.cancelled())
        .await
        .unwrap();
    drop(next);
    drop(busy);
    fixture.close().await;
}

#[tokio::test]
async fn retires_rotated_tokens_after_last_lease() {
    use crate::plugins::connectors::{Target, mcp::Authorization};
    use sailry_protocol::plugin::connector::{Declaration, Id, Kind};
    let fixture = Fixture::new().await;
    let declaration: Declaration = serde_json::from_value(json!({
        "id":"service","display":{"label":"Service"},"accounts":"single",
        "origins":["https://example.invalid"],
        "auth":[{"kind":"secret","fields":["token"]}],
        "fields":[{"name":"token","title":"Token","secret":true}],
        "inject":[{"server":"native","env":"PRIVATE_TOKEN","field":"token"}]
    }))
    .unwrap();
    let injection = declaration.inject[0].clone();
    let target = Target {
        id: Id::new(),
        revision: 1,
        grant_revision: 1,
        package: package().reference(),
        declaration,
        method: Kind::Secret,
        fields: BTreeMap::new(),
        secrets: BTreeMap::new(),
        identity: None,
        grant: None,
        mcp_endpoint: None,
        manual: false,
        checking: false,
    };
    let authorization = Authorization::new(
        fixture.node.ingress.clone(),
        vec![(target, injection)],
        CancellationToken::new(),
    );
    let first_launch = fixture.launch("first-private-token");
    let second_launch = fixture.launch("second-private-token");
    let config = config("native");
    let first_key = Key::new(&config, &first_launch, Some(&authorization)).unwrap();
    let second_key = Key::new(&config, &second_launch, Some(&authorization)).unwrap();
    assert!(first_key.authorization != second_key.authorization);
    assert_eq!(first_key.settings, second_key.settings);
    let (first, _) = fixture
        .pool
        .lease(
            &config,
            first_launch,
            Some(authorization.clone()),
            &fixture.node.ingress,
            &CancellationToken::new(),
            true,
        )
        .await
        .unwrap();
    let held = first.clone();
    let stopped = first.transport.lock().await.stopped();
    let (second, _) = fixture
        .pool
        .lease(
            &config,
            second_launch,
            Some(authorization),
            &fixture.node.ingress,
            &CancellationToken::new(),
            true,
        )
        .await
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(fixture.pool.entries.lock().unwrap().len(), 1);
    assert!(first.retired.load(Ordering::Relaxed));
    assert!(!stopped.is_cancelled());
    let latest = fixture.pool.begin(&package().reference(), "native");
    fixture.pool.finish(latest, Ok(4));
    first.poisoned.store(true, Ordering::Relaxed);
    fixture.pool.failed(&first);
    let mut projected = info();
    fixture.pool.project(&mut projected);
    assert_eq!(
        projected.mcp[0].runtime.as_ref().unwrap().state,
        RuntimeState::Connected
    );
    first.release().await.unwrap();
    drop(first);
    assert!(!stopped.is_cancelled());
    let stop = CancellationToken::new();
    held.toolset
        .call("read", serde_json::Map::new(), &stop)
        .await
        .unwrap();
    drop(held);
    tokio::time::timeout(Duration::from_secs(10), stopped.cancelled())
        .await
        .unwrap();
    drop(second);
    fixture.close().await;
}

#[test]
fn ignores_stale_status_and_disabled_packages() {
    let pool = Pool::default();
    let mut summary = package();
    pool.observe(&summary);
    let first = pool.begin(&summary.reference(), "native");
    let latest = pool.begin(&summary.reference(), "native");
    pool.finish(latest, Ok(4));
    pool.finish(first, Err(unavailable()));
    let mut projected = info();
    pool.project(&mut projected);
    assert_eq!(
        projected.mcp[0].runtime.as_ref().unwrap().state,
        RuntimeState::Connected
    );
    summary.enabled = false;
    summary.revision += 1;
    pool.observe(&summary);
    pool.project(&mut projected);
    assert!(projected.mcp[0].runtime.is_none());
}

#[test]
fn preserves_status_after_cancelled_attempts() {
    let pool = Pool::default();
    pool.observe(&package());
    let initial = pool.begin(&package().reference(), "native");
    pool.finish(initial, Ok(4));
    let mut projected = info();
    pool.project(&mut projected);
    let connected = projected.mcp[0].runtime.clone();
    let attempt = pool.begin(&package().reference(), "native");
    pool.started(&package().reference(), "native");
    pool.finish(attempt, Err(cancelled()));
    pool.project(&mut projected);
    assert_eq!(projected.mcp[0].runtime, connected);
}

#[test]
fn preserves_public_faults_without_private_details() {
    let pool = Pool::default();
    pool.observe(&package());
    let mut projected = info();
    let public = Fault::new(ErrorCode::NotConfigured, "MCP credentials changed");
    let attempt = pool.begin(&package().reference(), "native");
    pool.finish(attempt, Err(public.clone()));
    pool.project(&mut projected);
    assert_eq!(
        projected.mcp[0].runtime.as_ref().unwrap().fault,
        Some(public)
    );
    let attempt = pool.begin(&package().reference(), "native");
    pool.finish(
        attempt,
        Err(Fault::new(
            ErrorCode::PermissionDenied,
            "Header: private-token",
        )),
    );
    pool.project(&mut projected);
    let fault = projected.mcp[0]
        .runtime
        .as_ref()
        .unwrap()
        .fault
        .as_ref()
        .unwrap();
    assert_eq!(fault.code, ErrorCode::PermissionDenied);
    assert_eq!(fault.message, "MCP server is unavailable");
}

fn package() -> sailry_protocol::plugin::Summary {
    serde_json::from_value(json!({
        "name":"example","digest":"initial","revision":1,"settings_revision":1,
        "enabled":true,"version":"0.1.0","description":null
    }))
    .unwrap()
}

#[test]
fn ignores_unchanged_configuration() {
    let pool = Pool::default();
    let summary = package();
    assert!(pool.observe(&summary));
    assert!(!pool.observe(&summary));
    assert!(pool.reusable(&summary.reference()));
}

#[test]
fn preserves_newer_configuration() {
    let pool = Pool::default();
    let original = package();
    assert!(pool.observe(&original));
    let mut current = original.clone();
    current.settings_revision += 1;
    assert!(pool.observe(&current));
    assert!(!pool.observe(&original));
    assert!(!pool.reusable(&original.reference()));
    assert!(pool.reusable(&current.reference()));
}

#[test]
fn tracks_enable_changes() {
    let pool = Pool::default();
    let mut summary = package();
    assert!(pool.observe(&summary));
    summary.revision += 1;
    summary.enabled = false;
    assert!(pool.observe(&summary));
    assert!(!pool.reusable(&summary.reference()));
    summary.revision += 1;
    summary.enabled = true;
    assert!(pool.observe(&summary));
    assert!(pool.reusable(&summary.reference()));
}

#[test]
fn excludes_absent_packages_after_inventory() {
    let pool = Pool::default();
    pool.finish_inventory();
    assert!(!pool.reusable(&package().reference()));
}
