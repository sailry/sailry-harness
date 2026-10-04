//! The native WebView is the execution adapter; authorization lives on the Node.
use super::*;
use sailry_protocol::{
    ErrorCode, Fault, RequestId,
    browser::{self, Action},
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

impl Browser {
    pub(crate) fn execute(
        &mut self,
        mut action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<browser::Result> {
        if self.preview {
            return Task::ready(Err(unavailable()));
        }
        if matches!(action, Action::Tabs) {
            return Task::ready(Ok(
                json!({"tabs": self.tabs.iter().map(|tab| json!({"id":tab.id,"url":tab.url,"title":tab.title.as_str(),"loading":tab.loading})).collect::<Vec<_>>()}),
            ));
        }
        if let Action::Navigate { url, .. } | Action::Open { url } = &action
            && !url::Url::parse(url).is_ok_and(|u| {
                matches!(u.scheme(), "http" | "https")
                    && u.username().is_empty()
                    && u.password().is_none()
            })
        {
            return Task::ready(Err(Fault::new(
                ErrorCode::InvalidRequest,
                "browser URL must use HTTP or HTTPS without embedded credentials",
            )));
        }
        if let Action::Open { url } = action {
            self.add(window, cx);
            action = Action::Navigate {
                url,
                tab: Some(self.tabs[self.selected].id as u64),
            };
        }
        if self.tabs.is_empty() {
            self.tabs.push(Tab::empty(self.serial));
            self.serial += 1;
            self.selected = 0;
        }
        let tab = match &action {
            Action::Read { tab } | Action::Navigate { tab, .. } => *tab,
            Action::Click { tab, .. }
            | Action::Input { tab, .. }
            | Action::Scroll { tab, .. }
            | Action::Back { tab }
            | Action::Forward { tab }
            | Action::Refresh { tab }
            | Action::Close { tab }
            | Action::Focus { tab }
            | Action::Screenshot { tab }
            | Action::Select { tab, .. }
            | Action::Hover { tab, .. }
            | Action::Key { tab, .. }
            | Action::Frame { tab, .. }
            | Action::Wait { tab, .. } => Some(*tab),
            Action::Tabs | Action::Open { .. } => unreachable!(),
        }
        .unwrap_or(self.tabs[self.selected].id as u64);
        let Some(index) = self.tabs.iter().position(|value| value.id as u64 == tab) else {
            return Task::ready(Err(Fault::new(
                ErrorCode::NotFound,
                "browser tab is closed",
            )));
        };
        if matches!(action, Action::Close { .. }) {
            self.close(self.tabs[index].id, window, cx);
            return Task::ready(Ok(json!({"closed":tab})));
        }
        if matches!(action, Action::Focus { .. }) && self.tabs[index].url.is_empty() {
            self.select(index, window, cx);
            return Task::ready(Ok(json!({"tab":tab,"url":""})));
        }
        if !matches!(action, Action::Navigate { .. }) && self.tabs[index].url.is_empty() {
            return Task::ready(Err(Fault::new(
                ErrorCode::NotFound,
                "browser tab is empty; navigate to a URL before reading or interacting",
            )));
        }
        if !matches!(action, Action::Navigate { .. }) {
            self.select(index, window, cx);
        }
        if let Action::Navigate { url, .. } = &action {
            self.selected = index;
            self.load(url.clone(), window, cx);
        }
        if let Action::Input { text, .. } = &action
            && text.len() > 16000
        {
            return Task::ready(Err(Fault::new(
                ErrorCode::InvalidRequest,
                "browser input is too large",
            )));
        }
        if let Action::Wait { timeout_ms, .. } = &action
            && !(1..=20000).contains(timeout_ms)
        {
            return Task::ready(Err(Fault::new(
                ErrorCode::InvalidRequest,
                "browser wait must be between 1 and 20000 ms",
            )));
        }
        if matches!(action, Action::Screenshot { .. }) {
            return self.screenshot(tab, window, cx);
        }
        cx.spawn_in(window, async move |browser, cx| {
            let deadline = Instant::now()
                + match &action {
                    Action::Wait { timeout_ms, .. } => {
                        Duration::from_millis(u64::from(*timeout_ms))
                    }
                    _ => Duration::from_secs(20),
                };
            if !matches!(
                action,
                Action::Navigate { .. }
                    | Action::Read { .. }
                    | Action::Focus { .. }
                    | Action::Wait { .. }
            ) {
                let script = script(serde_json::to_value(&action).unwrap());
                let receive = browser
                    .update_in(cx, |browser, _, cx| browser.evaluate(tab, &script, cx))
                    .map_err(|_| unavailable())??;
                response(receive, cx).await?;
                // Let WebKit publish navigation started by click/back before inspecting the document.
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
            }
            loop {
                let loading = browser
                    .update_in(cx, |browser, _, _| {
                        let tab = browser
                            .tabs
                            .iter()
                            .find(|value| value.id as u64 == tab)
                            .ok_or_else(unavailable)?;
                        if tab.error.is_some() {
                            return Err(Fault::new(
                                ErrorCode::Unavailable,
                                "browser page failed to load",
                            ));
                        }
                        Ok(tab.loading)
                    })
                    .map_err(|_| unavailable())??;
                if !loading {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(Fault::new(
                        ErrorCode::Unavailable,
                        "browser page is still loading; read it again",
                    ));
                }
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
            }
            if matches!(action, Action::Wait { .. }) {
                loop {
                    let script = script(serde_json::to_value(&action).unwrap());
                    let receive = browser
                        .update_in(cx, |browser, _, cx| browser.evaluate(tab, &script, cx))
                        .map_err(|_| unavailable())??;
                    if response(receive, cx).await?["ready"] == true {
                        break;
                    }
                    if Instant::now() >= deadline {
                        return Err(Fault::new(ErrorCode::Unavailable, "browser wait timed out"));
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;
                }
            }
            let script = script(json!({"action":"read","snapshot":RequestId::new().to_string()}));
            let receive = browser
                .update_in(cx, |browser, _, cx| browser.evaluate(tab, &script, cx))
                .map_err(|_| unavailable())??;
            let mut value = response(receive, cx).await?;
            value["tab"] = tab.into();
            Ok(value)
        })
    }
    fn evaluate(
        &self,
        tab: u64,
        script: &str,
        cx: &App,
    ) -> Result<oneshot::Receiver<String>, Fault> {
        let page = self
            .tabs
            .iter()
            .find(|value| value.id as u64 == tab)
            .and_then(|tab| tab.page.as_ref())
            .ok_or_else(unavailable)?;
        let (send, receive) = oneshot::channel();
        let send = std::sync::Mutex::new(Some(send));
        page.read(cx)
            .raw()
            .evaluate_script_with_callback(script, move |value| {
                if let Some(send) = send.lock().unwrap().take() {
                    let _ = send.send(value);
                }
            })
            .map_err(|_| unavailable())?;
        Ok(receive)
    }
}
fn script(request: Value) -> String {
    format!("({})({request})", include_str!("automation.js"))
}
fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "browser page is unavailable")
}
async fn response(receive: oneshot::Receiver<String>, cx: &AsyncWindowContext) -> browser::Result {
    let timer = cx.background_executor().timer(Duration::from_secs(5));
    let raw = tokio::select! {
        value = receive => value.map_err(|_| unavailable())?,
        _ = timer => return Err(unavailable()),
    };
    let json: String = serde_json::from_str(&raw).map_err(|_| unavailable())?;
    let value: Value = serde_json::from_str(&json).map_err(|_| unavailable())?;
    if let Some(message) = value.get("error").and_then(Value::as_str) {
        return Err(Fault::new(ErrorCode::Conflict, message));
    }
    Ok(value)
}
