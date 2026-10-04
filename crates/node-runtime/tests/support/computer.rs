//! Model-free fixtures for the upstream private-worker boundary.
use cua_driver_sdk::{CuaDriver, DriverHostOptions};
use sailry_node_runtime::ComputerWorker;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn catalog() -> Vec<Value> {
    let inventory = CuaDriver::inspect_host_tools(DriverHostOptions {
        cursor: Default::default(),
        host_owns_permission_ux: true,
        host_bundle_id: None,
        claude_code_compatibility: false,
        prepare_desktop_environment: true,
        register_host_tools: None,
        authorization_host: None,
        activity_observer: None,
    });
    inventory["tools"]
        .as_array()
        .expect("native catalog")
        .clone()
}

pub fn result(value: Value) -> Value {
    json!({"content":[{"type":"text","text":value.to_string()}],
        "structuredContent":value,"isError":false})
}

pub fn action() -> Value {
    result(
        json!({"effect":"unverifiable","route":"synthetic_events","delivery":{"mode":"background"}}),
    )
}

pub struct Worker {
    directory: PathBuf,
    pub configuration: ComputerWorker,
}

#[cfg(unix)]
impl Worker {
    pub fn new(root: &Path, outcomes: Value) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let directory = root.join("computer-worker");
        fs::create_dir(&directory).unwrap();
        let executable = directory.join("worker.py");
        fs::write(&executable, include_str!("computer_worker.py")).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(directory.join("outcomes.json"), outcomes.to_string()).unwrap();
        Self {
            directory,
            configuration: ComputerWorker {
                executable,
                bundle_id: "ai.sailry.host".into(),
            },
        }
    }

    pub fn records(&self) -> Vec<Value> {
        match fs::read_to_string(self.directory.join("requests.jsonl")) {
            Ok(records) => records
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(error) => panic!("worker records: {error}"),
        }
    }

    pub fn calls(&self) -> Vec<Value> {
        let calls: Vec<_> = self
            .records()
            .into_iter()
            .filter(|record| record["operation"] == "call" && record["name"] != "end_session")
            .collect();
        let definitions = catalog();
        for call in &calls {
            let definition = definitions
                .iter()
                .find(|tool| tool["name"] == call["name"])
                .expect("native fixture tool");
            let validator =
                jsonschema::validator_for(&definition["inputSchema"]).expect("native tool schema");
            assert!(
                validator.is_valid(&call["arguments"]),
                "fixture arguments violate the pinned native schema: {call}"
            );
        }
        calls
    }
}
