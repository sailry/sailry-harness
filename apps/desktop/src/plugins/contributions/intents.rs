//! Semantic entry points select captured declarations, never hardcoded packages.
use super::*;
use serde_json::Value;
use std::time::{Duration, Instant};

pub(crate) fn request(
    registry: Entity<Registry>,
    intent: impl AsRef<str>,
    value: Option<Value>,
    window: &mut Window,
    cx: &mut App,
) -> Task<Result<Value, String>> {
    let intent = intent.as_ref().to_owned();
    let scope = {
        let registry = registry.read(cx);
        (registry.node, registry.worktree, registry.session)
    };
    window.spawn(cx, async move |cx| {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let result = registry.update(cx, |registry, cx| {
                if (registry.node, registry.worktree, registry.session) != scope {
                    return Some(Err("contribution context changed".to_owned()));
                }
                registry.ready(cx).then(|| match &value {
                    Some(value) => registry
                        .invoke_intent(&intent, value.clone(), cx)
                        .map(|_| Value::Bool(true)),
                    None => Ok(Value::Bool(registry.intent(&intent, cx).is_some())),
                })
            });
            if let Some(result) = result {
                return result;
            }
            if Instant::now() >= deadline {
                return Err("contribution did not become ready".into());
            }
            cx.background_executor()
                .timer(Duration::from_millis(20))
                .await;
        }
    })
}
