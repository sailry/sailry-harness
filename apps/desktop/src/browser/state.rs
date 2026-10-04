//! Controller actions address stable tab IDs within one captured browser.
use super::*;
use serde::Deserialize;
use serde_json::{Value, json};
use std::cell::RefCell;

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    Add {},
    Select { id: usize },
    Close { id: usize },
    Navigate { url: String },
    Back {},
    Forward {},
    Reload {},
    Stop {},
}

pub(crate) struct Request {
    pub action: Action,
    pub stop: sailry_link::CancellationToken,
    pub reply: RefCell<Option<tokio::sync::oneshot::Sender<Result<Value, String>>>>,
}
impl EventEmitter<Request> for Browser {}

impl Browser {
    pub(crate) fn snapshot(&self, cx: &App) -> Value {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let history = self.history(cx);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let history = [false; 2];
        json!({
            "cursor": self.cursor.to_string(),
            "tabs": self.tabs.iter().map(|tab| json!({
                "id":tab.id,"title":tab.title.as_str(),"url":tab.url,
                "loading":tab.loading,"loaded":tab.loaded(),"error":tab.error,
            })).collect::<Vec<_>>(),
            "selected": self.tabs.get(self.selected).map(|tab| tab.id),
            "history": history,
            "supported": cfg!(any(target_os="macos",target_os="windows")) && !self.preview,
        })
    }

    pub(crate) fn changes(&self, cx: &App) -> tokio::sync::watch::Receiver<Value> {
        self.changes.send_replace(self.snapshot(cx));
        self.changes.subscribe()
    }

    pub(super) fn changed(&mut self, cx: &mut Context<Self>) {
        self.cursor += 1;
        self.changes.send_replace(self.snapshot(cx));
        cx.notify();
    }

    pub(super) fn control(
        &mut self,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        match action {
            Action::Add {} => self.add(window, cx),
            Action::Select { id } | Action::Close { id } => {
                let index = self
                    .tabs
                    .iter()
                    .position(|tab| tab.id == id)
                    .ok_or_else(|| "browser tab is closed".to_string())?;
                if matches!(action, Action::Close { .. }) {
                    self.close(id, window, cx);
                } else {
                    self.select(index, window, cx);
                    #[cfg(any(target_os = "macos", target_os = "windows"))]
                    self.focus_selected(window, cx);
                }
            }
            Action::Navigate { url } => {
                if self.tabs.is_empty() {
                    self.add(window, cx);
                }
                self.navigate(&url, window, cx);
            }
            Action::Back {} | Action::Forward {} | Action::Reload {} | Action::Stop {} => {
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                {
                    let Some(tab) = self.tabs.get(self.selected) else {
                        return Err("browser tab is closed".into());
                    };
                    let index = match action {
                        Action::Back {} => 0,
                        Action::Forward {} => 1,
                        Action::Stop {} if !tab.loading => return Ok(()),
                        Action::Reload {} if tab.loading => return Ok(()),
                        _ => 2,
                    };
                    self.action(index, cx);
                }
            }
        }
        Ok(())
    }
}
