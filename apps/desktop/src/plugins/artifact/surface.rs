//! A native offline surface; all document layout and controls come from its package.
use super::*;
use gpui_kit::component::{
    ActiveTheme,
    input::{Editor, EditorState},
};
use sailry_link::CancellationToken;
use serde_json::{Value, json};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::rc::Rc;

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct PreparedNative {
    native: Rc<crate::content::files::html::surface::Native>,
    theme: Option<Value>,
    writable: Option<bool>,
    dirty: bool,
    events: Option<tokio::sync::mpsc::Receiver<String>>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct PendingNative {
    generation: u64,
    content: Content,
    prepared: PreparedNative,
    ready: bool,
    mounted: bool,
}

pub(super) struct View {
    resource: Resource,
    host: Arc<Host>,
    revision: Option<u32>,
    content: Option<Content>,
    source: Option<Entity<EditorState>>,
    dirty: bool,
    loading: bool,
    reload: bool,
    generation: u64,
    error: Option<&'static str>,
    events: tokio::sync::mpsc::Sender<Value>,
    labels: std::collections::BTreeMap<String, String>,
    theme: Option<Value>,
    writable: std::cell::Cell<Option<bool>>,
    epoch: u64,
    export: Option<(u64, tokio::sync::oneshot::Sender<Result<String, HostError>>)>,
    capture_id: u64,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    native: Option<Rc<crate::content::files::html::surface::Native>>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pending_native: Option<PendingNative>,
    stop: CancellationToken,
    load: Option<Task<()>>,
    _watch: Task<()>,
    _source: Option<Subscription>,
}

impl Drop for View {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl View {
    pub(super) fn new(
        resource: Resource,
        props: Props,
        host: Arc<Host>,
        events: tokio::sync::mpsc::Sender<Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let node = resource.binding.client.target();
        let revision = props.revision.or_else(|| {
            // A newly published card must not reopen an older draft. Keep that
            // draft stored for explicit historical browsing instead.
            crate::preferences::artifacts::revision(node, resource.artifact.id, cx)
                .filter(|revision| *revision >= resource.artifact.revision)
        });
        let dirty = revision.is_some_and(|revision| {
            !crate::preferences::artifacts::drafts(node, resource.artifact.id, revision, cx)
                .is_empty()
        });
        if dirty {
            let _ = events.try_send(json!({
                "kind":"edited","artifact":resource.artifact.id,"revision":revision.unwrap()
            }));
        }
        let stop = host.stop_token().child_token();
        let mut updates = host.artifact_updates().ok();
        let cancellation = stop.clone();
        let watch = cx.spawn_in(window, async move |owner, cx| {
            let mut cursor = None;
            loop {
                let Some(updates) = &mut updates else { return };
                let view = updates.borrow_and_update().clone();
                let changed = cursor != Some((view.artifact_cursor, view.connected));
                cursor = Some((view.artifact_cursor, view.connected));
                if changed
                    && owner
                        .update_in(cx, |owner, window, cx| {
                            if let Some(content) = &mut owner.content
                                && let Some(latest) = view.snapshot.as_ref().and_then(|snapshot| {
                                    snapshot
                                        .artifacts
                                        .items
                                        .iter()
                                        .find(|artifact| artifact.id == content.artifact.id)
                                })
                            {
                                content.latest_revision = latest.revision;
                            }
                            owner.writable(window, cx);
                            if view.connected {
                                if owner.dirty
                                    && let Some(content) = &owner.content
                                {
                                    owner.revision = Some(content.artifact.revision);
                                }
                                owner.refresh(window, cx);
                            }
                        })
                        .is_err()
                {
                    return;
                }
                tokio::select! {biased; _=cancellation.cancelled()=>return,
                result=updates.changed()=>if result.is_err(){return},}
            }
        });
        let source = resource
            .source
            .upgrade()
            .map(|source| cx.observe(&source, |_: &mut Self, _, cx| cx.notify()));
        let mut view = Self {
            resource,
            host,
            revision,
            content: None,
            source: None,
            dirty,
            loading: false,
            reload: false,
            generation: 0,
            error: None,
            events,
            labels: props.labels,
            theme: None,
            writable: std::cell::Cell::new(None),
            epoch: 0,
            export: None,
            capture_id: 0,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            native: None,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            pending_native: None,
            stop,
            load: None,
            _watch: watch,
            _source: source,
        };
        crate::feedback::observe(window, cx, |view: &Self, _| {
            view.error.into_iter().collect()
        });
        view.refresh(window, cx);
        view
    }

    pub(super) fn select(
        &mut self,
        revision: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A package may still request follow mode before consuming the edit
        // event. Preserve the pinned draft until an explicit version is chosen.
        if self.revision == revision || (self.dirty && revision.is_none()) {
            return;
        }
        self.revision = revision;
        self.generation += 1;
        self.load = None;
        self.loading = false;
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.pending_native = None;
        }
        if revision.is_some_and(|revision| {
            self.content
                .as_ref()
                .is_some_and(|content| content.artifact.revision == revision)
        }) {
            self.writable(window, cx);
            return;
        }
        self.dirty = false;
        self.refresh(window, cx);
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading {
            self.reload = true;
            return;
        }
        let Ok(result) = self.host.read_artifact(
            self.resource.artifact.id,
            self.revision,
            self.resource.artifact.session,
        ) else {
            self.error = Some("artifact_preview_failed");
            cx.notify();
            return;
        };
        self.loading = true;
        self.generation += 1;
        let generation = self.generation;
        self.load = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = result.await;
            let _ = owner.update_in(cx, |owner, window, cx| {
                if owner.generation != generation || owner.stop.is_cancelled() {
                    return;
                }
                owner.loading = false;
                match result {
                    Ok(content) => {
                        if owner.content.as_ref().is_some_and(|old| {
                            owner.dirty || old.artifact.revision == content.artifact.revision
                        }) {
                            if let Some(old) = &mut owner.content {
                                old.latest_revision = content.latest_revision;
                            }
                            owner.writable(window, cx);
                            if owner.reload {
                                owner.reload = false;
                                owner.refresh(window, cx);
                            }
                            return;
                        }
                        #[cfg(any(target_os = "macos", target_os = "windows"))]
                        if owner.native.is_some() {
                            let (ready, receiver) = tokio::sync::mpsc::channel(1);
                            match owner.prepare_native(&content, Some(ready), window, cx) {
                                Ok(prepared) => {
                                    owner.loading = true;
                                    owner.pending_native = Some(PendingNative {
                                        generation,
                                        content,
                                        prepared,
                                        ready: false,
                                        mounted: false,
                                    });
                                    owner.wait_until_ready(generation, receiver, window, cx);
                                    cx.notify();
                                    return;
                                }
                                Err(()) => {
                                    owner.error = Some("artifact_preview_failed");
                                    cx.notify();
                                    return;
                                }
                            }
                        }
                        owner.replace_content(content);
                    }
                    Err(_) => owner.error = Some("artifact_preview_failed"),
                }
                owner.writable(window, cx);
                cx.notify();
                if owner.reload {
                    owner.reload = false;
                    owner.refresh(window, cx);
                }
            });
        }));
        cx.notify();
    }

    fn writable(&self, _: &mut Window, cx: &mut App) {
        if self.content.as_ref().is_some_and(|content| {
            content.artifact.kind == sailry_protocol::artifact::Kind::Preview
        }) {
            return;
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(native) = &self.native {
            let available = self
                .content
                .as_ref()
                .is_some_and(|content| content.artifact.revision == content.latest_revision)
                && self.host.check().is_ok()
                && self.resource.source.upgrade().is_some_and(|source| {
                    source
                        .read(cx)
                        .document_writable(self.resource.artifact.session)
                });
            if self.writable.get() != Some(available) {
                self.writable.set(Some(available));
                let _ = native
                    .page
                    .raw()
                    .evaluate_script(&format!("window.sailryDocument?.setWritable({available});"));
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = cx;
    }

    fn replace_content(&mut self, content: Content) {
        self.content = Some(content);
        self.source = None;
        self.error = None;
        if let Some((_, sender)) = self.export.take() {
            let _ = sender.send(Err(HostError::new("artifact revision changed")));
        }
        self.theme = None;
        self.writable.set(None);
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn prepare_native(
        &self,
        content: &Content,
        ready: Option<tokio::sync::mpsc::Sender<bool>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<PreparedNative, ()> {
        if content.artifact.kind == sailry_protocol::artifact::Kind::Preview {
            let builder = wry::WebViewBuilder::new()
                .with_html(crate::content::files::html::surface::document(
                    &content.html,
                    cx,
                ))
                .with_navigation_handler(|url| url == "about:blank" || url == "about:srcdoc");
            let builder = if let Some(ready) = ready {
                builder.with_on_page_load_handler(move |event, _| {
                    if matches!(event, wry::PageLoadEvent::Finished) {
                        let _ = ready.try_send(true);
                    }
                })
            } else {
                builder
            };
            let native = crate::content::files::html::surface::Native::build(builder, window, cx)?;
            return Ok(PreparedNative {
                native: Rc::new(native),
                theme: None,
                writable: None,
                dirty: false,
                events: None,
            });
        }
        let theme = page::theme(cx);
        let writable =
            content.artifact.revision == content.latest_revision
                && self.host.check().is_ok()
                && self.resource.source.upgrade().is_some_and(|source| {
                    source.read(cx).document_writable(content.artifact.session)
                });
        let mut labels = page::labels();
        if let Some(labels) = labels.as_object_mut() {
            labels.extend(
                self.labels
                    .iter()
                    .map(|(key, value)| (key.clone(), Value::String(value.clone()))),
            );
        }
        let drafts = crate::preferences::artifacts::drafts(
            self.resource.binding.client.target(),
            content.artifact.id,
            content.artifact.revision,
            cx,
        );
        let dirty = !drafts.is_empty();
        let html =
            page::document(content, theme.clone(), labels, writable, drafts).map_err(|_| ())?;
        let (sender, receiver) = tokio::sync::mpsc::channel::<String>(4);
        let builder = wry::WebViewBuilder::new()
            .with_html(html)
            .with_navigation_handler(|url| url == "about:blank")
            .with_ipc_handler(move |request| {
                if request.body().len() <= 16 * 1024 * 1024 {
                    let value = serde_json::from_str::<Value>(request.body()).ok();
                    let kind = value
                        .as_ref()
                        .and_then(|value| value.get("kind"))
                        .and_then(Value::as_str);
                    if kind == Some("surface_ready") {
                        if let Some(ready) = &ready {
                            let _ = ready.try_send(true);
                        }
                        return;
                    }
                    if kind == Some("surface_failed") {
                        if let Some(ready) = &ready {
                            let _ = ready.try_send(false);
                        }
                        return;
                    }
                    let export = kind.is_some_and(|kind| matches!(kind, "export" | "export_error"));
                    if export || request.body().len() <= 32 * 1024 {
                        let _ = sender.try_send(request.body().clone());
                    }
                }
            });
        let native = crate::content::files::html::surface::Native::build(builder, window, cx)?;
        Ok(PreparedNative {
            native: Rc::new(native),
            theme: Some(theme),
            writable: Some(writable),
            dirty,
            events: Some(receiver),
        })
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn install_native(
        &mut self,
        mut prepared: PreparedNative,
        revision: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.native = Some(prepared.native);
        self.theme = prepared.theme;
        self.writable.set(prepared.writable);
        self.dirty |= prepared.dirty;
        self.epoch += 1;
        let Some(mut receiver) = prepared.events.take() else {
            return;
        };
        let stop = self.stop.clone();
        let epoch = self.epoch;
        cx.spawn_in(window, async move |owner, cx| {
            loop {
                let event =
                    tokio::select! {biased;_=stop.cancelled()=>return,event=receiver.recv()=>event};
                let Some(event) = event else { return };
                let _ = owner.update_in(cx, |owner, window, cx| {
                    if owner.epoch == epoch
                        && owner
                            .content
                            .as_ref()
                            .is_some_and(|content| content.artifact.revision == revision)
                    {
                        owner.event(event, window, cx);
                    }
                });
            }
        })
        .detach();
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn wait_until_ready(
        &self,
        generation: u64,
        mut receiver: tokio::sync::mpsc::Receiver<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |owner, cx| {
            let Some(ready) = receiver.recv().await else {
                return;
            };
            let _ = owner.update_in(cx, |owner, window, cx| {
                if owner.generation != generation
                    || owner
                        .pending_native
                        .as_ref()
                        .is_none_or(|pending| pending.generation != generation)
                {
                    return;
                }
                if ready {
                    owner.pending_native.as_mut().unwrap().ready = true;
                } else {
                    owner.pending_native = None;
                    owner.loading = false;
                    owner.error = Some("artifact_preview_failed");
                    if owner.reload {
                        owner.reload = false;
                        owner.refresh(window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn publish_pending(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .pending_native
            .as_ref()
            .is_none_or(|pending| !pending.ready || !pending.mounted)
        {
            return;
        }
        let pending = self.pending_native.take().unwrap();
        if pending.prepared.native.set_visible(true, cx).is_err() {
            self.loading = false;
            self.error = Some("artifact_preview_failed");
            cx.notify();
            return;
        }
        if let Some(native) = &self.native {
            let _ = native.set_visible(false, cx);
        }
        let revision = pending.content.artifact.revision;
        self.replace_content(pending.content);
        self.install_native(pending.prepared, revision, window, cx);
        self.loading = false;
        cx.notify();
        if self.reload {
            self.reload = false;
            self.refresh(window, cx);
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.native.is_some() {
            return;
        }
        let Some(content) = &self.content else { return };
        let revision = content.artifact.revision;
        match self.prepare_native(content, None, window, cx) {
            Ok(prepared) => self.install_native(prepared, revision, window, cx),
            Err(()) => self.error = Some("artifact_preview_failed"),
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn event(&mut self, event: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.stop.is_cancelled() || self.host.check().is_err() {
            return;
        }
        let Some(content) = &self.content else { return };
        if content.artifact.kind != sailry_protocol::artifact::Kind::Document {
            return;
        }
        let Ok(event) = serde_json::from_str::<Value>(&event) else {
            return;
        };
        if event
            .get("kind")
            .and_then(Value::as_str)
            .is_some_and(|kind| matches!(kind, "export" | "export_error"))
        {
            if self.export.as_ref().is_some_and(|(capture, _)| {
                event.get("capture").and_then(Value::as_u64) == Some(*capture)
            }) {
                let (_, sender) = self.export.take().unwrap();
                let value = event
                    .get("html")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| HostError::new("artifact export failed"));
                let _ = sender.send(value);
            }
            return;
        }
        if event.get("artifact") != Some(&json!(content.artifact.id))
            || event.get("revision") != Some(&json!(content.artifact.revision))
            || event.get("session") != Some(&json!(content.artifact.session))
        {
            return;
        }
        match event.get("kind").and_then(Value::as_str) {
            Some("draft") => {
                let payload = json!({"form":event.get("form"),"fields":event.get("fields")});
                let valid = serde_json::from_value::<crate::preferences::artifacts::Draft>(payload)
                    .map_err(|_| ())
                    .and_then(|draft| {
                        crate::preferences::artifacts::remember(
                            self.resource.binding.client.target(),
                            content.artifact.id,
                            content.artifact.revision,
                            draft,
                            cx,
                        )
                    });
                if valid.is_err() {
                    self.report("invalid_form", "artifact_form_invalid", window, cx);
                } else {
                    self.dirty = true;
                }
            }
            Some("edited") => {
                self.dirty = true;
                let _=self.events.try_send(json!({"kind":"edited","artifact":content.artifact.id,"revision":content.artifact.revision}));
            }
            Some("submit") => {
                if content.artifact.revision != content.latest_revision {
                    crate::feedback::error("", &crate::tr("artifact_form_stale"), window, cx);
                    self.ack(
                        event
                            .get("form")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                        false,
                    );
                    return;
                }
                let Ok(message) = form_message(content, &event) else {
                    self.report("invalid_form", "artifact_form_invalid", window, cx);
                    return;
                };
                let Some(source) = self.resource.source.upgrade() else {
                    return;
                };
                let form = event
                    .get("form")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let (status, mut receiver) = tokio::sync::watch::channel(None::<bool>);
                let sent = source.update(cx, |source, cx| {
                    source.send_document(content.artifact.session, message, status, window, cx)
                });
                if !sent {
                    crate::feedback::error("", &crate::tr("chat_action_failed"), window, cx);
                    self.ack(&form, false);
                    cx.notify();
                    return;
                }
                self.dirty = true;
                let node = self.resource.binding.client.target();
                let artifact = content.artifact.id;
                let revision = content.artifact.revision;
                let epoch = self.epoch;
                // Admission outlives the surface; only its visible acknowledgement is scoped.
                cx.spawn(async move |owner, cx| {
                    loop {
                        let status = *receiver.borrow_and_update();
                        if let Some(accepted) = status {
                            if accepted {
                                cx.update(|cx| {
                                    crate::preferences::artifacts::forget(
                                        node, artifact, revision, &form, cx,
                                    )
                                });
                            }
                            let _ = owner.update(cx, |owner, cx| {
                                if owner.epoch == epoch
                                    && owner.content.as_ref().is_some_and(|content| {
                                        content.artifact.revision == revision
                                    })
                                {
                                    owner.ack(&form, accepted);
                                    cx.notify();
                                }
                            });
                            return;
                        }
                        if receiver.changed().await.is_err() {
                            return;
                        }
                    }
                })
                .detach();
            }
            Some("required") => {
                self.report("required", "artifact_form_required", window, cx);
            }
            Some("invalid_form") => {
                self.report("invalid_form", "artifact_form_invalid", window, cx);
            }
            Some("render_error") => {
                let mut entry = sailry_node_runtime::logging::Entry::new(
                    sailry_node_runtime::logging::Area::Artifact,
                    sailry_node_runtime::logging::Phase::Failed,
                );
                entry.context.artifact = Some(content.artifact.id);
                entry.revision = Some(content.artifact.revision.into());
                entry.reason = Some("render");
                entry.emit_debug();
            }
            _ => (),
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn report(&self, label: &str, fallback: &str, window: &mut Window, cx: &mut App) {
        let message = self
            .labels
            .get(label)
            .map(|message| SharedString::from(message.clone()))
            .unwrap_or_else(|| crate::tr(fallback));
        crate::feedback::error("", &message, window, cx);
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn ack(&self, form: &str, accepted: bool) {
        if let Some(native) = &self.native {
            let form = serde_json::to_string(form).unwrap();
            let _ = native.page.raw().evaluate_script(&format!(
                "window.sailryDocument?.submitted({form},{accepted});"
            ));
        }
    }

    pub(super) fn capture(
        &mut self,
        revision: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<String, HostError>> {
        if let Some(content) = &self.content
            && content.artifact.kind == sailry_protocol::artifact::Kind::Preview
        {
            if self.stop.is_cancelled() || content.artifact.revision != revision {
                return Task::ready(Err(HostError::new("artifact revision is unavailable")));
            }
            return page::capture(content.clone(), window, cx);
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            if self
                .export
                .as_ref()
                .is_some_and(|(_, sender)| sender.is_closed())
            {
                self.export = None;
            }
            if self.export.is_some()
                || self.stop.is_cancelled()
                || self
                    .content
                    .as_ref()
                    .is_none_or(|content| content.artifact.revision != revision)
            {
                return Task::ready(Err(HostError::new("artifact revision is unavailable")));
            }
            self.mount(window, cx);
            let Some(native) = &self.native else {
                return Task::ready(Err(HostError::new("artifact view is unavailable")));
            };
            let (sender, receiver) = tokio::sync::oneshot::channel();
            self.capture_id += 1;
            let capture = self.capture_id;
            if native
                .page
                .raw()
                .evaluate_script(&page::serialize_capture(
                    Some(capture),
                    cx.theme().foreground,
                ))
                .is_err()
            {
                return Task::ready(Err(HostError::new("artifact export failed")));
            }
            self.export = Some((capture, sender));
            window.spawn(cx,async move|cx|{
                let timer=cx.background_executor().timer(std::time::Duration::from_secs(60));
                tokio::select!{result=receiver=>result.map_err(|_|HostError::new("artifact view is closed"))?,
                    _=timer=>Err(HostError::new("artifact export timed out")),}
            })
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (revision, window, cx);
            Task::ready(Err(HostError::new("native artifact export is unavailable")))
        }
    }
}

impl Render for View {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.publish_pending(window, cx);
            self.mount(window, cx);
            if let Some(native) = &self.native {
                let theme = page::theme(cx);
                if self.theme.as_ref() != Some(&theme) {
                    let script = if self.content.as_ref().is_some_and(|content| {
                        content.artifact.kind == sailry_protocol::artifact::Kind::Preview
                    }) {
                        // Only fixed wrapper colors change; authored code stays in srcdoc.
                        format!(
                            "for(const element of [document.documentElement,document.body])Object.assign(element.style,{{background:{},color:{}}});",
                            theme["background"], theme["foreground"]
                        )
                    } else {
                        format!("window.sailryDocument?.retheme({theme});")
                    };
                    let _ = native.page.raw().evaluate_script(&script);
                    self.theme = Some(theme);
                }
                self.writable(window, cx);
                let current =
                    crate::content::files::html::surface::render(native.clone(), window, cx);
                if let Some(pending) = &mut self.pending_native {
                    let replacement = crate::content::files::html::surface::render_hidden(
                        pending.prepared.native.clone(),
                        cx,
                    );
                    pending.mounted = true;
                    if pending.ready {
                        cx.notify();
                    }
                    return div()
                        .relative()
                        .size_full()
                        .child(div().absolute().inset_0().child(current))
                        .child(div().absolute().inset_0().child(replacement))
                        .into_any_element();
                }
                return current;
            }
        }
        if self.source.is_none()
            && let Some(content) = &self.content
        {
            self.source = Some(cx.new(|cx| {
                EditorState::new(window, cx)
                    .language("html")
                    .default_value(content.html.clone())
            }));
        }
        if let Some(source) = &self.source {
            return Editor::new(source)
                .readonly(true)
                .appearance(false)
                .bordered(false)
                .h_full()
                .into_any_element();
        }
        div().size_full().into_any_element()
    }
}

pub(super) fn form_message(content: &Content, event: &Value) -> Result<String, ()> {
    let valid_name = |value: &str| {
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    };
    let form = event
        .get("form")
        .and_then(Value::as_str)
        .filter(|name| valid_name(name))
        .ok_or(())?;
    let fields = event
        .get("fields")
        .and_then(Value::as_array)
        .filter(|fields| !fields.is_empty() && fields.len() <= 32)
        .ok_or(())?;
    let mut message = rust_i18n::t!(
        "artifact_form_reply",
        title = content.artifact.title,
        id = content.artifact.id,
        revision = content.artifact.revision,
        form = form
    )
    .to_string();
    let mut names = std::collections::HashSet::new();
    for field in fields {
        let name = field
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| valid_name(name) && names.insert(*name))
            .ok_or(())?;
        let label = field
            .get("label")
            .and_then(Value::as_str)
            .filter(|label| label.len() <= 512)
            .ok_or(())?;
        let values = field
            .get("values")
            .and_then(Value::as_array)
            .filter(|values| values.len() <= 64)
            .ok_or(())?;
        message.push_str(&format!(
            "\n\n{}:",
            if label.is_empty() { name } else { label }
        ));
        for value in values {
            let label = value
                .get("label")
                .and_then(Value::as_str)
                .filter(|value| {
                    value.len() <= 4096 && !value.chars().any(|character| character == '\0')
                })
                .ok_or(())?;
            let raw = value
                .get("value")
                .and_then(Value::as_str)
                .filter(|value| {
                    value.len() <= 4096 && !value.chars().any(|character| character == '\0')
                })
                .ok_or(())?;
            message.push_str(&if raw == label {
                format!("\n- {label}")
            } else {
                format!("\n- {label} ({raw})")
            });
        }
    }
    if message.len() > 32 * 1024 {
        return Err(());
    }
    Ok(message)
}
