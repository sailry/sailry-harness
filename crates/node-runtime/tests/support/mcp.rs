//! Isolated native MCP peer launched from a filtered copy of the Rust test binary.
use serde_json::{Value, json};
use std::{fs, io::Write, path::Path};

pub fn alias(server: &str, name: &str) -> String {
    package_alias("example", server, name)
}

pub fn package_alias(package: &str, server: &str, name: &str) -> String {
    let mut hash = blake3::Hasher::new_derive_key("Sailry MCP tool name v1");
    for part in [package, server, name] {
        hash.update(&(part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    let label: String = format!("{package}_{server}_{name}")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .take(32)
        .collect();
    format!("mcp_{label}_{}", &hash.finalize().to_hex()[..24])
}

pub fn config(test: &str, mode: &str) -> Value {
    let executable = std::env::current_exe().unwrap();
    json!({"type": "stdio", "command": executable.file_name().unwrap().to_str().unwrap(),
    "args": ["--exact", test, "--ignored", "--nocapture", "--test-threads=1", "--skip=${PLUGIN_ROOT}/unused"],
    "cwd": "${PLUGIN_DATA}", "env": {
        "PATH": executable.parent().unwrap().to_str().unwrap(),
        "SAILRY_MCP_FIXTURE": "1", "SAILRY_MCP_MODE": mode,
        "SAILRY_MCP_TEXT": "${PLUGIN_ROOT}|${PLUGIN_DATA}|${HOME}|中文 🙂"
    }})
}

pub fn descriptor(name: &str) -> Value {
    json!({"name": name, "description": format!("Fixture {name}"),
        "inputSchema": {"type": "object", "properties": {"value": {"type": "string"}}},
        "annotations": {"readOnlyHint": name == "read"}})
}

pub fn listed(cursor: &Value) -> Value {
    if cursor == "second" {
        json!({"tools": [descriptor("hold"), descriptor("fail")]})
    } else {
        json!({"tools": [descriptor("read"), descriptor("write")], "nextCursor": "second"})
    }
}

fn append(path: &Path, value: &str) {
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap()
        .write_all(value.as_bytes())
        .unwrap();
}

#[test]
#[ignore = "launched only as an isolated MCP subprocess"]
fn stdio_peer() {
    use std::io::BufRead;
    assert_eq!(
        std::env::var("SAILRY_MCP_FIXTURE").as_deref(),
        Ok("1"),
        "MCP fixture must be launched by its isolated subprocess harness"
    );
    // Terminate the test harness's status prefix before emitting protocol lines.
    println!();
    let data = std::env::var("PLUGIN_DATA").unwrap();
    let data = Path::new(&data);
    fs::write(data.join("pid"), std::process::id().to_string()).unwrap();
    append(&data.join("launches"), "x");
    let root = std::env::var("PLUGIN_ROOT").unwrap();
    let mode = std::env::var("SAILRY_MCP_MODE").unwrap();
    let mut pending = None;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else {
            break;
        };
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        append(&data.join("requests.jsonl"), &format!("{request}\n"));
        let id = &request["id"];
        if request["method"].is_null() && id == "form-request" {
            if let Some(call) = pending.take() {
                println!(
                    "{}",
                    json!({"jsonrpc":"2.0","id":call,"result":{"content":[{"type":"text","text":"Input received"}],"structuredContent":request["result"]}})
                );
                std::io::stdout().flush().unwrap();
            }
            continue;
        }
        if request["method"] == "initialize" && mode == "bad_handshake" {
            println!(
                "{}",
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": "fixture rejects initialization"}})
            );
            std::io::stdout().flush().unwrap();
            continue;
        }
        let result = match request["method"].as_str().unwrap_or("") {
            "initialize" if mode == "slow_handshake" => continue,
            "initialize" => {
                if mode.starts_with("elicitation") {
                    assert!(request["params"]["capabilities"]["elicitation"]["form"].is_object());
                    assert!(request["params"]["capabilities"]["elicitation"]["url"].is_object());
                }
                json!({"protocolVersion": request["params"]["protocolVersion"], "capabilities": {"tools": {}}, "serverInfo": {"name": "fixture", "version": "1"}})
            }
            "tools/list" => listed(&request["params"]["cursor"]),
            "tools/call" => {
                if mode.starts_with("elicitation") {
                    pending = Some(id.clone());
                    let schema = Path::new(&root).join("form.json");
                    let schema = if schema.exists() {
                        serde_json::from_slice(&fs::read(schema).unwrap()).unwrap()
                    } else {
                        form_schema()
                    };
                    println!(
                        "{}",
                        json!({"jsonrpc":"2.0","id":"form-request","method":"elicitation/create","params":{"mode":"form","message":"Choose how to prepare the report","requestedSchema":schema}})
                    );
                    std::io::stdout().flush().unwrap();
                    if mode == "elicitation_cancel" {
                        let data = data.to_owned();
                        std::thread::spawn(move || {
                            while !data.join("cancel_input").exists() {
                                std::thread::sleep(std::time::Duration::from_millis(10));
                            }
                            println!(
                                "{}",
                                json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"form-request","reason":"fixture cancelled input"}})
                            );
                            std::io::stdout().flush().unwrap();
                        });
                    }
                    continue;
                }
                let name = request["params"]["name"].as_str().unwrap();
                if matches!(name, "write" | "fail") {
                    append(
                        &data.join("effects"),
                        request["params"]["arguments"]["value"].as_str().unwrap(),
                    );
                }
                if name == "fail" {
                    std::process::exit(29);
                }
                if name == "hold" {
                    fs::write(data.join("holding"), "ready").unwrap();
                    continue;
                }
                if mode == "settings" {
                    let path = std::env::var("SAILRY_PLUGIN_SETTINGS_FILE").unwrap();
                    let settings: Value =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    let secret = std::env::var("SAILRY_MCP_TOKEN").unwrap();
                    let output = json!({"settings": settings, "settings_path": path,
                        "secret_hash": blake3::hash(secret.as_bytes()).to_hex().to_string()});
                    println!(
                        "{}",
                        json!({"jsonrpc":"2.0", "id":id, "result":{
                            "content":[{"type":"text", "text":"Configured MCP result"}], "structuredContent":output
                        }})
                    );
                    std::io::stdout().flush().unwrap();
                    continue;
                }
                json!({"content": [{"type": "text", "text": "Complete MCP result 中文 🙂"}], "structuredContent": {
                    "version": fs::read_to_string(Path::new(&root).join("version.txt")).unwrap(),
                    "root": root, "data": data, "cwd": std::env::current_dir().unwrap(),
                    "text": std::env::var("SAILRY_MCP_TEXT").unwrap(), "args": std::env::args().collect::<Vec<_>>(),
                    "provider_key_present": std::env::var_os("SAILRY_MODEL_API_KEY").is_some(),
                    "value": request["params"]["arguments"]["value"]
                }})
            }
            "ping" => json!({}),
            _ => continue,
        };
        println!("{}", json!({"jsonrpc": "2.0", "id": id, "result": result}));
        std::io::stdout().flush().unwrap();
    }
    std::process::exit(0);
}

pub fn form_schema() -> Value {
    json!({"type":"object","properties":{
        "title":{"type":"string","title":"Report title","minLength":2,"maxLength":32},
        "copies":{"type":"integer","title":"Copies","minimum":1,"maximum":5},
        "ratio":{"type":"number","title":"Scale","minimum":0.5,"maximum":2},
        "publish":{"type":"boolean","title":"Publish"},
        "format":{"type":"string","title":"Format","oneOf":[{"const":"md","title":"Markdown"},{"const":"txt","title":"Plain text"}]},
        "sections":{"type":"array","title":"Sections","items":{"type":"string","enum":["summary","details"]},"minItems":1,"maxItems":2},
        "email":{"type":"string","title":"Email","format":"email"},
        "notes":{"type":"string","title":"Notes","default":"Do not submit this automatically"}
    },"required":["title","copies","ratio","publish","format","sections"]})
}
