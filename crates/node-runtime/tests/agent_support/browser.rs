//! Deterministic model fixture; actions execute in the real embedded WebView.
use serde_json::{Value, json};

pub(super) fn next(body: &Value, url: &str, capture: bool) -> Option<(String, Value)> {
    let results: Vec<Value> = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| serde_json::from_str(message["content"].as_str().unwrap()).unwrap())
        .collect();
    let page = results
        .iter()
        .rev()
        .find(|value| value["elements"].is_array());
    let tab = page.map_or(json!(0), |page| page["tab"].clone());
    let target = |label: &str| {
        let page = page.expect("page result required");
        let element = page["elements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["label"] == label)
            .unwrap_or_else(|| {
                panic!(
                    "fixture element {label} missing at step {}: {page}",
                    results.len()
                )
            });
        json!({"tab":page["tab"],"snapshot":page["snapshot"],"element":element["id"]})
    };
    let (name, args) = match results.len() {
        0 => ("navigate", json!({"url":url})),
        1 => (
            "wait",
            json!({"tab":tab,"condition":{"kind":"visible","selector":"#ready"},"timeout_ms":5000}),
        ),
        2 => {
            let mut args = target("Message");
            args["text"] = "Browser fixture".into();
            ("input", args)
        }
        3 => {
            let mut args = target("Choice");
            args["value"] = "b".into();
            ("select", args)
        }
        4 => ("hover", target("Hover")),
        5 => {
            let mut args = target("Message");
            args["key"] = "Enter".into();
            ("key", args)
        }
        6 => (
            "wait",
            json!({"tab":tab,"condition":{"kind":"text","text":"Submitted Browser fixture b hovered"},"timeout_ms":5000}),
        ),
        7 => {
            let mut args = target("Hover");
            args["snapshot"] = "stale".into();
            ("click", args)
        }
        8 => ("frame", target("Same origin")),
        9 => {
            let mut args = target("Frame input");
            args["text"] = "Frame value".into();
            ("input", args)
        }
        10 => (
            "frame",
            json!({"tab":tab,"snapshot":page.unwrap()["snapshot"]}),
        ),
        11 => ("navigate", json!({"tab":tab,"url":format!("{url}second")})),
        12 => ("back", json!({"tab":tab})),
        13 => ("forward", json!({"tab":tab})),
        14 => ("refresh", json!({"tab":tab})),
        15 => ("open", json!({"url":url})),
        16 => ("focus", json!({"tab":results[0]["tab"]})),
        17 => ("close", json!({"tab":results[15]["tab"]})),
        18 => (
            "wait",
            json!({"tab":tab,"condition":{"kind":"hidden","selector":"#missing"},"timeout_ms":500}),
        ),
        19 => (
            "wait",
            json!({"tab":tab,"condition":{"kind":"text","text":"never appears"},"timeout_ms":100}),
        ),
        20 => (
            "wait",
            json!({"tab":tab,"condition":{"kind":"visible","selector":"["},"timeout_ms":100}),
        ),
        21 => ("navigate", json!({"tab":tab,"url":url})),
        22 => ("frame", target("Cross origin")),
        23 => ("read", json!({"tab":tab})),
        24 if capture => ("screenshot", json!({"tab":tab})),
        _ => return None,
    };
    eprintln!("Browser fixture step {}: {name}", results.len());
    Some((
        super::plugin_tool("browser", &format!("browser_{name}")),
        args,
    ))
}
