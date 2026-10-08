use serde_json::{Value, json};

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

fn tagged(kind: &str, data: Value) -> Value {
    object(
        json!({"kind":{"type":"string","enum":[kind]},"data":data}),
        &["kind", "data"],
    )
}

pub(super) fn target() -> Value {
    json!({"anyOf":[{"type":"null"},
        tagged("worktree", object(json!({"scope":{"type":"string","enum":["all","staged","unstaged"]}}), &["scope"])),
        tagged("commit", object(json!({"revision":{"type":"string"}}), &["revision"])),
        tagged("range", object(json!({"base":{"type":"string"},"head":{"type":"string"}}), &["base", "head"]))]})
}

pub(super) fn snapshot() -> Value {
    let string = json!({"type":"string"});
    let state = json!({"anyOf":[
        tagged("worktree", object(json!({"scope":{"type":"string","enum":["all","staged","unstaged"]},"head":{"type":["string","null"]},"index_revision":string}), &["scope", "head", "index_revision"])),
        tagged("commit", object(json!({"revision":string,"parent":{"type":["string","null"]}}), &["revision", "parent"])),
        tagged("range", object(json!({"base":string,"head":string}), &["base", "head"]))]});
    object(
        json!({"target":state,"metadata_revision":string}),
        &["target", "metadata_revision"],
    )
}

pub(super) fn cursor() -> Value {
    json!({"anyOf":[{"type":"null"}, object(json!({"snapshot":snapshot(),"after_path":{"type":["string","null"]}}), &["snapshot", "after_path"])]})
}
