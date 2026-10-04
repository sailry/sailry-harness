//! Session-scoped native WebViews, navigation, and controller profile ownership.
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod automation;
pub(crate) mod bridge;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod capture;
use gpui_kit::{component::*, *};

#[cfg(target_os = "macos")]
pub(crate) mod chrome;
#[cfg(target_os = "macos")]
pub(crate) mod inspector;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native;
#[cfg(target_os = "macos")]
mod navigation;
#[cfg(target_os = "macos")]
pub(crate) mod profile;
pub(crate) mod settings;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod shortcuts;
pub(crate) mod state;
#[cfg(test)]
mod tests;
mod view;

struct Tab {
    id: usize,
    title: SharedString,
    url: String,
    loading: bool,
    error: Option<&'static str>,
    #[cfg(target_os = "macos")]
    navigation: Option<objc2::rc::Retained<navigation::Observer>>,
    #[cfg(target_os = "macos")]
    surface: Option<std::rc::Rc<inspector::Surface>>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    page: Option<Entity<gpui_wry::WebView>>,
}

pub(crate) struct Browser {
    tabs: Vec<Tab>,
    #[cfg(target_os = "macos")]
    store: Option<objc2::rc::Retained<objc2_web_kit::WKWebsiteDataStore>>,
    persistent: bool,
    reload: bool,
    selected: usize,
    serial: usize,
    changes: tokio::sync::watch::Sender<serde_json::Value>,
    cursor: u64,
    pub(crate) lease: u64,
    mounted: bool,
    preview: bool,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    events: tokio::sync::mpsc::UnboundedSender<(usize, native::Event)>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    _events: Task<()>,
}
impl EventEmitter<DismissEvent> for Browser {}

impl Browser {
    pub(crate) fn close_all(&mut self) {
        self.tabs.clear();
    }

    pub(crate) fn new(preview: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        shortcuts::init(cx);
        cx.subscribe_in(
            &cx.entity(),
            window,
            |this, _, request: &state::Request, window, cx| {
                let result = if request.stop.is_cancelled() {
                    Err("plugin view is closed".to_string())
                } else {
                    this.control(request.action.clone(), window, cx)
                        .map(|_| this.snapshot(cx))
                };
                if let Some(reply) = request.reply.borrow_mut().take() {
                    let _ = reply.send(result);
                }
            },
        )
        .detach();
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let task = cx.spawn_in(window, async move |this, cx| {
            while let Some((id, event)) = receiver.recv().await {
                if this
                    .update_in(cx, |this, window, cx| this.accept(id, event, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        if let Some(Some(root)) = window.root::<Root>() {
            cx.observe_in(&root, window, |this, _, window, cx| {
                this.visibility(this.mounted, window, cx);
            })
            .detach();
        }
        #[cfg(target_os = "macos")]
        cx.observe_global::<crate::preferences::Preferences>(|this, cx| {
            let persistent = profile::persistent(cx);
            if this.persistent != persistent {
                this.persistent = persistent;
                this.store = None;
                for tab in &mut this.tabs {
                    tab.surface = None;
                    tab.navigation = None;
                    tab.page = None;
                    // Drop callbacks queued by the old website data store.
                    tab.id = this.serial;
                    this.serial += 1;
                    tab.loading = false;
                }
                this.reload = true;
                this.changed(cx);
            }
        })
        .detach();
        Self {
            reload: false,
            persistent: crate::preferences::data(cx)
                .browser_persistent
                .unwrap_or(true),
            tabs: vec![Tab::empty(0)],
            #[cfg(target_os = "macos")]
            store: None,
            selected: 0,
            serial: 1,
            changes: tokio::sync::watch::channel(serde_json::Value::Null).0,
            cursor: 0,
            lease: 0,
            mounted: true,
            preview,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            events,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            _events: task,
        }
    }

    pub(crate) fn open(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.tabs.iter().position(|tab| tab.url == url) {
            self.select(index, window, cx);
            return;
        }
        if self.tabs.is_empty() {
            self.add(window, cx);
        }
        if !self.tabs[self.selected].url.is_empty() {
            self.add(window, cx);
        }
        self.navigate(url, window, cx);
    }

    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.push(Tab::empty(self.serial));
        self.serial += 1;
        self.select(self.tabs.len() - 1, window, cx);
    }

    fn select(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = index;
        let url = self.tabs[index].url.clone();
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if self.tabs[index].page.is_none() && !url.is_empty() && !self.preview {
            self.load(url, window, cx);
            return;
        }
        self.visibility(self.mounted, window, cx);
        self.changed(cx);
    }

    pub(crate) fn close_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.selected) {
            self.close(tab.id, window, cx);
        }
    }

    fn close(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let active = index == self.selected;
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            cx.emit(DismissEvent);
            self.changed(cx);
            return;
        }
        let selected = if index < self.selected {
            self.selected - 1
        } else {
            self.selected.min(self.tabs.len() - 1)
        };
        if active {
            self.select(selected, window, cx);
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            self.focus_selected(window, cx);
        } else {
            self.selected = selected;
            self.changed(cx);
        }
    }

    pub(crate) fn visibility(
        &mut self,
        mounted: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.mounted = mounted;
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let visible = mounted && !window.has_active_dialog(cx);
            for (index, tab) in self.tabs.iter().enumerate() {
                if let Some(page) = &tab.page {
                    page.update(cx, |page, _| {
                        let show = visible && index == self.selected;
                        #[cfg(target_os = "macos")]
                        if let Some(surface) = &tab.surface {
                            surface.visible(show);
                        }
                        if show != page.visible() {
                            if show {
                                page.show();
                            } else {
                                page.hide();
                            }
                        }
                    });
                }
            }
        }
    }

    fn navigate(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(url) = destination(value) else {
            self.tabs[self.selected].error = Some("browser_invalid_address");
            self.changed(cx);
            return;
        };
        if self.preview {
            self.tabs[self.selected].error = Some("resource_browser_preview");
            self.changed(cx);
            return;
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        self.load(url, window, cx);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (url, window);
            self.tabs[self.selected].error = Some("browser_unavailable");
            self.changed(cx);
        }
    }
}

impl Tab {
    fn loaded(&self) -> bool {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.page.is_some()
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            false
        }
    }

    fn empty(id: usize) -> Self {
        Self {
            id,
            title: "".into(),
            url: String::new(),
            loading: false,
            error: None,
            #[cfg(target_os = "macos")]
            navigation: None,
            #[cfg(target_os = "macos")]
            surface: None,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            page: None,
        }
    }
}

fn destination(value: &str) -> Option<String> {
    let parsed = url::Url::parse(value.trim()).ok()?;
    matches!(parsed.scheme(), "http" | "https").then(|| parsed.to_string())
}
