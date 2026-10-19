//! Connect each execution Node to this controller's session-scoped browser views.
use super::Browser;
use crate::{
    backend::Services,
    shell::{Shell, session_scope::Key},
};
use gpui_kit::*;
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, NodeId, SessionId, Update, WorktreeId, browser::Call};
use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Owner {
    Session(NodeId, SessionId),
    Draft(NodeId, Option<WorktreeId>),
    Preview(Key),
}

#[derive(Default)]
pub(crate) struct State {
    views: BTreeMap<Owner, Entity<Browser>>,
    connections: BTreeMap<NodeId, Connection>,
}
impl State {
    fn owns_composer(
        &self,
        key: Key,
        source: &Entity<crate::conversation::live::View>,
        browser: &Entity<Browser>,
        cx: &App,
    ) -> bool {
        let binding = source.read(cx).binding();
        let owner = match key {
            Key::Session(node, session)
                if binding.client.target() == node
                    && source.read(cx).session() == Some(session) =>
            {
                Owner::Session(node, session)
            }
            Key::Draft if source.read(cx).session().is_none() => {
                Owner::Draft(binding.client.target(), binding.worktree)
            }
            _ => return false,
        };
        self.views.get(&owner) == Some(browser)
    }

    pub(crate) fn owns_session(&self, key: Key, browser: &Entity<Browser>) -> bool {
        let Key::Session(node, session) = key else {
            return false;
        };
        self.views.get(&Owner::Session(node, session)) == Some(browser)
    }
    pub(crate) fn close(&mut self) -> Vec<Entity<Browser>> {
        self.connections.clear();
        std::mem::take(&mut self.views).into_values().collect()
    }
}
struct Connection {
    stop: CancellationToken,
    _task: Task<()>,
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Shell {
    pub(crate) fn bind_browsers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let Some(live) = &self.live else {
                return;
            };
            let services = cx.global::<Services>().clone();
            self.browsers
                .connections
                .retain(|node, _| live.hosts.contains_key(node));
            for node in live.hosts.keys().copied() {
                if self.browsers.connections.contains_key(&node) {
                    continue;
                }
                let Some(transport) = live.transport_for(node) else {
                    continue;
                };
                let stop = CancellationToken::new();
                let cancellation = stop.clone();
                let (send, mut receive) = tokio::sync::mpsc::channel::<(
                    Call,
                    tokio::sync::oneshot::Sender<sailry_protocol::browser::Result>,
                )>(16);
                services.runtime.spawn(async move {
                    let client = Client::new(transport);
                    let work = async {
                        loop {
                            if let Ok(mut subscription) = client.subscribe_browser().await {
                                while let Ok(Update::BrowserCall(call)) = subscription.next().await
                                {
                                    let id = call.id;
                                    let (reply, result) = tokio::sync::oneshot::channel();
                                    if send.send((call, reply)).await.is_err() {
                                        return;
                                    }
                                    let Ok(result) = result.await else {
                                        return;
                                    };
                                    // A lost completion is not permission to replay a browser action.
                                    if client
                                        .execute(
                                            client.prepare(Command::CompleteBrowser { id, result }),
                                        )
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                            }
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                    };
                    tokio::select! { _ = cancellation.cancelled() => {}, _ = work => {} }
                });
                let cancellation = stop.clone();
                let task = cx.spawn_in(window, async move |shell, cx| {
                    while let Some((call, reply)) = receive.recv().await {
                        let expired = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64
                            >= call.expires_at_ms;
                        if expired || cancellation.is_cancelled() {
                            let _ = reply.send(Err(sailry_protocol::Fault::new(
                                sailry_protocol::ErrorCode::Cancelled,
                                "browser call expired",
                            )));
                            continue;
                        }
                        let task = shell.update_in(cx, |shell, window, cx| {
                            shell.execute_browser_call(node, call, window, cx)
                        });
                        match task {
                            Ok(task) => {
                                let _ = reply.send(task.await);
                            }
                            Err(_) => break,
                        }
                    }
                });
                self.browsers
                    .connections
                    .insert(node, Connection { stop, _task: task });
            }
        }
    }

    pub(crate) fn execute_browser_call(
        &mut self,
        node: NodeId,
        call: Call,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<sailry_protocol::browser::Result> {
        let key = Key::Session(node, call.session);
        let browser = self.browser_for(key, window, cx);
        let foreground = matches!(
            call.action,
            sailry_protocol::browser::Action::Open {
                background: false,
                ..
            } | sailry_protocol::browser::Action::Navigate {
                background: false,
                ..
            } | sailry_protocol::browser::Action::Focus { .. }
        );
        if self.session_scope.active == key && foreground {
            self.reveal_browser(browser.clone(), window, cx);
        }
        browser.update(cx, |browser, cx| browser.execute(call.action, window, cx))
    }

    pub(crate) fn promote_browser(
        &mut self,
        node: NodeId,
        worktree: Option<WorktreeId>,
        session: SessionId,
    ) {
        if let Some(browser) = self
            .browsers
            .views
            .remove(&Owner::Draft(node, worktree))
            .or_else(|| self.browsers.views.remove(&Owner::Draft(node, None)))
        {
            self.browsers
                .views
                .entry(Owner::Session(node, session))
                .or_insert(browser);
        }
    }

    pub(crate) fn browser_for(
        &mut self,
        key: Key,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Browser> {
        let owner = match key {
            Key::Session(node, session) => Owner::Session(node, session),
            Key::Draft => self
                .current_chat()
                .map(|source| {
                    let binding = source.read(cx).binding();
                    Owner::Draft(binding.client.target(), binding.worktree)
                })
                .unwrap_or(Owner::Preview(key)),
            _ => Owner::Preview(key),
        };
        if let Some(browser) = self.browsers.views.get(&owner) {
            if browser.read(cx).tabs.is_empty() {
                browser.update(cx, |browser, cx| browser.add(window, cx));
            }
            return browser.clone();
        }
        let browser = cx.new(|cx| Browser::new(self.live.is_none(), window, cx));
        browser.update(cx, |browser, cx| browser.visibility(false, window, cx));
        cx.subscribe_in(
            &browser,
            window,
            |shell, browser, request: &super::annotation::Transfer, window, cx| {
                let result = if request.stop.is_cancelled() {
                    Err("browser_annotation_stale")
                } else if let Some(source) = shell.current_chat().cloned().filter(|source| {
                    shell
                        .browsers
                        .owns_composer(shell.session_scope.active, source, browser, cx)
                }) {
                    source.update(cx, |source, cx| {
                        source.insert_browser_reference(
                            &request.reference,
                            request.image.clone(),
                            window,
                            cx,
                        )
                    })
                } else {
                    Err("browser_annotation_stale")
                };
                if let Some(reply) = request.reply.borrow_mut().take() {
                    let _ = reply.send(result);
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &browser,
            window,
            |shell, browser, _: &DismissEvent, window, cx| {
                if shell
                    .side_resource
                    .as_ref()
                    .and_then(|panel| panel.browser(cx))
                    .as_ref()
                    == Some(browser)
                {
                    shell.close_resource_panel(cx);
                    shell.focus.focus(window, cx);
                }
            },
        )
        .detach();
        self.browsers.views.insert(owner, browser.clone());
        browser
    }
}
