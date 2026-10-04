use super::*;
use gpui_wry::WebView;
use raw_window_handle::HasWindowHandle;

pub(super) enum Event {
    Title(String),
    Load(String, bool),
    Open(String),
    Navigating,
    #[cfg(target_os = "macos")]
    Failed {
        cancelled: bool,
    },
    Close,
    #[cfg(target_os = "macos")]
    Inspect,
}

impl Browser {
    pub(super) fn focus_selected(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self
            .tabs
            .get(self.selected)
            .filter(|tab| tab.page.is_some())
        else {
            return;
        };
        let id = tab.id;
        let lease = self.lease;
        let browser = cx.entity().downgrade();
        // A closed WebView's Drop returns native focus to its parent. Let that
        // release finish before handing both GPUI and native focus to the survivor.
        window.defer(cx, move |window, cx| {
            if window.has_active_dialog(cx) {
                return;
            }
            let Some(browser) = browser.upgrade() else {
                return;
            };
            let page = {
                let browser = browser.read(cx);
                if browser.lease != lease || !browser.mounted {
                    return;
                }
                let Some(tab) = browser
                    .tabs
                    .get(browser.selected)
                    .filter(|tab| tab.id == id)
                else {
                    return;
                };
                let Some(page) = tab.page.as_ref().filter(|page| page.read(cx).visible()) else {
                    return;
                };
                page.clone()
            };
            page.read(cx).focus_handle(cx).focus(window, cx);
            if let Err(error) = page.read(cx).raw().focus() {
                eprintln!("sailry-desktop: browser focus failed: {error}");
            }
        });
    }

    pub(super) fn load(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &mut self.tabs[self.selected];
        tab.error = None;
        if let Some(page) = &tab.page {
            if let Err(error) = page.read(cx).raw().load_url(&url) {
                eprintln!("sailry-desktop: browser navigation failed: {error}");
                tab.error = Some("browser_load_failed");
            }
        } else {
            let id = tab.id;
            let titles = self.events.clone();
            let loads = self.events.clone();
            let popups = self.events.clone();
            #[cfg(not(target_os = "macos"))]
            let navigation = self.events.clone();
            let shortcuts = self.events.clone();
            let shortcut = super::shortcuts::script(cx);
            let page = window
                .window_handle()
                .map_err(|error| error.to_string())
                .and_then(|handle| {
                    let builder = wry::WebViewBuilder::new()
                        .with_url(&url)
                        .with_initialization_script(&shortcut)
                        .with_ipc_handler(move |request| {
                            #[cfg(target_os = "macos")]
                            if request.body() == "inspect" {
                                let _ = shortcuts.send((id, Event::Inspect));
                            }
                            if request.body() == "close-tab" {
                                let _ = shortcuts.send((id, Event::Close));
                            }
                        })
                        .with_incognito(true)
                        .with_visible(false)
                        .with_document_title_changed_handler(move |title| {
                            let _ = titles.send((id, Event::Title(title)));
                        })
                        .with_on_page_load_handler(move |event, url| {
                            let _ = loads.send((
                                id,
                                Event::Load(url, matches!(event, wry::PageLoadEvent::Started)),
                            ));
                        })
                        .with_navigation_handler(move |url| {
                            let allowed = url == "about:blank"
                                || url == "about:srcdoc"
                                || url::Url::parse(&url)
                                    .is_ok_and(|url| matches!(url.scheme(), "http" | "https"));
                            #[cfg(not(target_os = "macos"))]
                            if allowed {
                                let _ = navigation.send((id, Event::Navigating));
                            }
                            allowed
                        })
                        .with_new_window_req_handler(move |url, _| {
                            let _ = popups.send((id, Event::Open(url)));
                            wry::NewWindowResponse::Deny
                        });
                    #[cfg(target_os = "macos")]
                    let builder = {
                        use wry::WebViewBuilderExtMacos as _;
                        let thread =
                            objc2::MainThreadMarker::new().expect("browser runs on the UI thread");
                        let store = self.store.get_or_insert_with(|| super::profile::store(cx));
                        let configuration =
                            unsafe { objc2_web_kit::WKWebViewConfiguration::new(thread) };
                        // Share local browser data. Each tab keeps its own
                        // script controller, IPC handlers and navigation delegate.
                        unsafe {
                            configuration.setWebsiteDataStore(store);
                            // WKWebView's bare UA makes Baidu return an HTTPS-to-HTTP
                            // redirect page. Keep the OS-supplied UA and append the
                            // desktop WebKit compatibility token plus our own identity.
                            configuration.setApplicationNameForUserAgent(Some(
                                &objc2_foundation::NSString::from_str(concat!(
                                    "Safari/605.1.15 Sailry/",
                                    env!("CARGO_PKG_VERSION")
                                )),
                            ));
                        }
                        builder.with_webview_configuration(configuration)
                    };
                    builder
                        .build_as_child(&handle)
                        .map_err(|error| error.to_string())
                });
            match page {
                Ok(page) => {
                    #[cfg(target_os = "macos")]
                    {
                        use wry::WebViewExtMacOS as _;
                        tab.navigation = Some(super::navigation::Observer::attach(
                            &page.webview(),
                            id,
                            self.events.clone(),
                        ));
                    }
                    tab.page = Some(cx.new(|cx| {
                        let mut page = WebView::new(page, window, cx);
                        page.hide();
                        // Kit initializes zero bounds and has no layout for unmounted tabs.
                        // Background browser tools still need a viewport; visible layout takes over.
                        let viewport = window.viewport_size();
                        let _ = page.raw().set_bounds(wry::Rect {
                            position: wry::dpi::LogicalPosition::new(0., 0.).into(),
                            size: wry::dpi::LogicalSize::new(
                                f32::from(viewport.width) as f64,
                                f32::from(viewport.height) as f64,
                            )
                            .into(),
                        });
                        page
                    }));
                }
                Err(error) => {
                    eprintln!("sailry-desktop: browser creation failed: {error}");
                    tab.error = Some("browser_unavailable");
                }
            }
        }
        tab.url = url.clone();
        tab.loading = tab.error.is_none();
        self.visibility(self.mounted, window, cx);
        self.changed(cx);
    }

    pub(super) fn accept(
        &mut self,
        id: usize,
        event: Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        match event {
            #[cfg(target_os = "macos")]
            Event::Inspect => {
                if index == self.selected {
                    self.inspect(cx);
                }
            }
            Event::Navigating => {
                self.tabs[index].loading = true;
                self.tabs[index].error = None;
            }
            #[cfg(target_os = "macos")]
            Event::Failed { cancelled } => {
                self.tabs[index].loading = false;
                self.tabs[index].error = (!cancelled).then_some("browser_load_failed");
            }
            Event::Close => {
                self.close(id, window, cx);
                return;
            }
            Event::Title(title) => {
                self.tabs[index].title = if title.is_empty() {
                    "".into()
                } else {
                    title.into()
                };
            }
            Event::Load(url, loading) => {
                if !loading {
                    self.sync_shortcuts(cx);
                }
                self.tabs[index].url = url.clone();
                self.tabs[index].loading = loading;
            }
            Event::Open(url) => {
                if let Some(url) = destination(&url) {
                    self.add(window, cx);
                    self.load(url, window, cx);
                }
            }
        }
        self.changed(cx);
    }

    pub(super) fn history(&self, cx: &App) -> [bool; 2] {
        let Some(page) = self
            .tabs
            .get(self.selected)
            .and_then(|tab| tab.page.as_ref())
        else {
            return [false; 2];
        };
        #[cfg(target_os = "macos")]
        {
            use wry::WebViewExtMacOS as _;
            let view = page.read(cx).raw().webview();
            // Kit exposes only back(); use wry's owned WKWebView handle for
            // native history availability without maintaining another history.
            unsafe { [view.canGoBack(), view.canGoForward()] }
        }
        #[cfg(target_os = "windows")]
        {
            let _ = (page, cx);
            [true; 2]
        }
    }

    pub(super) fn action(&mut self, action: usize, cx: &mut Context<Self>) {
        let Some(page) = self
            .tabs
            .get(self.selected)
            .and_then(|tab| tab.page.as_ref())
        else {
            return;
        };
        let stopping = action == 2 && self.tabs[self.selected].loading;
        let page = page.read(cx);
        #[cfg(target_os = "macos")]
        {
            use wry::WebViewExtMacOS as _;
            let view = page.raw().webview();
            unsafe {
                match action {
                    0 => {
                        view.goBack();
                    }
                    1 => {
                        view.goForward();
                    }
                    2 if self.tabs[self.selected].loading => {
                        view.stopLoading();
                        self.tabs[self.selected].loading = false;
                    }
                    _ => {
                        let _ = page.raw().reload();
                    }
                }
            }
        }
        #[cfg(target_os = "windows")]
        {
            let result = match action {
                0 => page.raw().evaluate_script("history.back()"),
                1 => page.raw().evaluate_script("history.forward()"),
                _ => page.raw().reload(),
            };
            if result.is_err() {
                self.tabs[self.selected].error = Some("browser_load_failed");
            }
        }
        self.tabs[self.selected].loading = !stopping;
        self.changed(cx);
    }
}
