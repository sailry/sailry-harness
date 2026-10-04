//! Test adapters execute production package policy rather than reproducing its storage rules.
use sailry_client::Client;
use sailry_protocol::{
    Command, ErrorCode, Fault, Output, ProjectId, Request, RequestId, WorktreeId, plugin,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{fs, path::Path};

pub type MemoryId = RequestId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub summary: Summary,
    pub body: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub id: MemoryId,
    pub project: Option<ProjectId>,
    pub title: String,
    pub kind: Kind,
    pub revision: u64,
    pub updated_at_ms: u64,
    pub archived: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    User,
    Feedback,
    Project,
    Reference,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub revision: u64,
    pub enabled: bool,
    pub auto_write: bool,
    pub context_bytes: u32,
    pub review_after_days: u32,
}

fn copy(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

pub fn prepare(target: &Path) {
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/memory"),
        target,
    );
    fs::write(
        target.join("dev.sailry.platform/host/fixture.js"),
        include_str!("bridge.js"),
    )
    .unwrap();
    let path = target.join("plugin.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let host = &mut manifest["extensions"]["dev.sailry.platform"]["host"];
    host["entry"] = json!("dev.sailry.platform/host/fixture.js");
    host["resources"]
        .as_array_mut()
        .unwrap()
        .push(json!("dev.sailry.platform/host/fixture.js"));
    host["handlers"]
        .as_array_mut()
        .unwrap()
        .push(json!("fixture"));
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
}

pub async fn install(client: &Client, worktree: WorktreeId, path: &str) -> plugin::Context {
    let Output::Plugins(plugins) = client
        .execute(client.prepare(Command::ListPlugins))
        .await
        .unwrap()
    else {
        panic!("plugin inventory expected")
    };
    let revision = plugins
        .iter()
        .find(|package| package.name == "memory")
        .map_or(0, |package| package.revision);
    let Output::Plugin(package) = client
        .execute(client.prepare(Command::InstallPlugin {
            worktree,
            path: path.into(),
            name: "memory".into(),
            expected_revision: revision,
        }))
        .await
        .unwrap()
    else {
        panic!("installed memory package expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    plugin::Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package: package.summary.reference(),
        worktree: Some(worktree),
        session: None,
    }
}

pub async fn context(client: &Client) -> plugin::Context {
    let Output::Plugins(plugins) = client
        .execute(client.prepare(Command::ListPlugins))
        .await
        .unwrap()
    else {
        panic!("plugin inventory expected")
    };
    let package = plugins
        .iter()
        .find(|package| package.name == "memory")
        .expect("memory package")
        .reference();
    plugin::Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package,
        worktree: None,
        session: None,
    }
}

pub fn request(client: &Client, context: &plugin::Context, input: Value) -> Request {
    client
        .prepare(Command::CallPlugin {
            handler: "fixture".into(),
            input,
        })
        .with_plugin(context.clone())
}

pub fn output<T: DeserializeOwned>(output: Output) -> Result<T, Fault> {
    let Output::PluginResult(value) = output else {
        return Err(Fault::new(
            ErrorCode::Internal,
            "memory fixture result expected",
        ));
    };
    if let Some(error) = value.get("Err") {
        return Err(serde_json::from_value(error.clone()).expect("fixture fault"));
    }
    serde_json::from_value(value["Ok"].clone())
        .map_err(|error| Fault::new(ErrorCode::Internal, error.to_string()))
}

pub async fn run<T: DeserializeOwned>(
    client: &Client,
    context: &plugin::Context,
    input: Value,
) -> Result<T, Fault> {
    output(client.execute(request(client, context, input)).await?)
}

pub async fn read(client: &Client, id: MemoryId) -> Result<Entry, Fault> {
    run(
        client,
        &context(client).await,
        json!({"action":"read","id":id}),
    )
    .await
}
pub async fn put(client: &Client, entry: Entry, expected: u64) -> Result<Entry, Fault> {
    run(
        client,
        &context(client).await,
        json!({"action":"put","entry":entry,"expected_revision":expected}),
    )
    .await
}
pub async fn list(client: &Client) -> Result<Vec<Summary>, Fault> {
    run(client, &context(client).await, json!({"action":"list"})).await
}
pub async fn settings(client: &Client) -> Result<Settings, Fault> {
    run(client, &context(client).await, json!({"action":"settings"})).await
}
pub async fn save_settings(client: &Client, settings: Settings) -> Result<Settings, Fault> {
    run(
        client,
        &context(client).await,
        json!({"action":"saveSettings","settings":settings}),
    )
    .await
}
