//! Task responses use the same HTTP/SSE fixture as ordinary MCP input.
use super::*;
use std::sync::{Mutex, atomic::AtomicBool};

#[derive(Default)]
pub struct State {
    work: Mutex<BTreeMap<String, Work>>,
    pub updates: AtomicUsize,
    pub cancelled: AtomicUsize,
    pub ignore_cancel: AtomicBool,
}

#[derive(Default)]
struct Work {
    polls: usize,
    answer: Option<Value>,
    repeated: bool,
    cancelled: bool,
}

impl State {
    pub fn respond(&self, request: &Value, schema: &Value, calls: &AtomicUsize) -> Option<Value> {
        let method = request["method"].as_str()?;
        if method == "initialize" {
            assert!(
                request["params"]["capabilities"]["extensions"]["io.modelcontextprotocol/tasks"]
                    .is_object()
            );
            return Some(
                json!({"protocolVersion":request["params"]["protocolVersion"], "capabilities":{"tools":{},"extensions":{"io.modelcontextprotocol/tasks":{}}},"serverInfo":{"name":"task-fixture","version":"1"}}),
            );
        }
        let mut work = self.work.lock().unwrap();
        if method == "tools/call" {
            let id = format!("task-{}", calls.fetch_add(1, Ordering::SeqCst) + 1);
            work.insert(id.clone(), Work::default());
            let mut result = task(&id, "working");
            result["resultType"] = json!("task");
            return Some(result);
        }
        if !method.starts_with("tasks/") {
            return None;
        }
        let id = request["params"]["taskId"].as_str().unwrap();
        let work = work.get_mut(id).unwrap();
        match method {
            "tasks/cancel" => {
                self.cancelled.fetch_add(1, Ordering::SeqCst);
                work.cancelled = !self.ignore_cancel.load(Ordering::SeqCst);
                Some(json!({"resultType":"complete"}))
            }
            "tasks/update" => {
                self.updates.fetch_add(1, Ordering::SeqCst);
                let answer = request["params"]["inputResponses"]["input-1"].clone();
                assert!(!answer.is_null());
                if let Some(previous) = &work.answer {
                    assert_eq!(previous, &answer);
                }
                work.answer = Some(answer);
                Some(json!({"resultType":"complete"}))
            }
            "tasks/get" => {
                work.polls += 1;
                if work.cancelled {
                    return Some(task(id, "cancelled"));
                }
                if work.polls == 1 {
                    return Some(task(id, "working"));
                }
                if work.answer.is_none() || !work.repeated {
                    if work.answer.is_some() {
                        work.repeated = true;
                    }
                    let params = if schema["mode"] == "url" {
                        json!({"mode":"url","message":"Continue in the service","url":schema["url"],"elicitationId":id})
                    } else {
                        json!({"mode":"form","message":"Choose how to prepare the report","requestedSchema":schema})
                    };
                    let mut result = task(id, "input_required");
                    result["inputRequests"] =
                        json!({"input-1":{"method":"elicitation/create","params":params}});
                    return Some(result);
                }
                let mut result = task(id, "completed");
                result["result"] = json!({"content":[{"type":"text","text":"Task finished"}],"structuredContent":work.answer});
                Some(result)
            }
            _ => panic!("unexpected task operation"),
        }
    }
}

fn task(id: &str, status: &str) -> Value {
    json!({"resultType":"complete","taskId":id,"status":status,"createdAt":"2026-09-11T00:00:00Z","lastUpdatedAt":"2026-09-11T00:00:00Z","ttlMs":null,"pollIntervalMs":10})
}
