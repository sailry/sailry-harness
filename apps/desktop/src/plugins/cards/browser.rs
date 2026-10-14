//! Protected targets use isolated Kit WebViews, never an Agent browser resource.
//! Kit provides native attachment, focus and composition; no IPC or DOM bridge exists.
use super::*;
#[cfg(target_os = "macos")]
use raw_window_handle::HasWindowHandle as _;
use url::Url;

pub(crate) struct Browser {
    pub(crate) source: WeakEntity<Chat>,
    owner: WeakEntity<Card>,
    #[cfg(target_os = "macos")]
    native: Entity<gpui_webview::WebView>,
    _owner: Subscription,
}

pub(super) fn allowed(value: &str, target: &card::Target, redirect: Option<&str>) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    if url.scheme() == "https" && target.origins.contains(&url.origin().ascii_serialization()) {
        return true;
    }
    redirect
        .and_then(|redirect| Url::parse(redirect).ok())
        .is_some_and(|expected| {
            expected.scheme() == "http"
                && expected.host_str() == Some("127.0.0.1")
                && url.origin() == expected.origin()
                && url.path() == expected.path()
                && url.fragment().is_none()
        })
}

impl Browser {
    pub(crate) fn new(
        owner: WeakEntity<Card>,
        source: WeakEntity<Chat>,
        url: String,
        target: card::Target,
        redirect: Option<String>,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<Entity<Self>, sailry_protocol::Fault> {
        if !allowed(&url, &target, None) {
            return Err(sailry_protocol::Fault::invalid(
                "browser target is outside its declared origins",
            ));
        }
        #[cfg(target_os = "macos")]
        {
            let builder = wry::WebViewBuilder::new()
                .with_url(&url)
                .with_incognito(true)
                .with_visible(false)
                .with_navigation_handler(move |url| allowed(&url, &target, redirect.as_deref()))
                .with_download_started_handler(|_, _| false)
                .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny);
            let handle = window.window_handle().map_err(|_| {
                sailry_protocol::Fault::invalid("isolated browser could not be created")
            })?;
            let page = builder.build_as_child(&handle).map_err(|_| {
                sailry_protocol::Fault::invalid("isolated browser could not be created")
            })?;
            let native = cx.new(|cx| {
                let mut page = gpui_webview::WebView::new(page, window, cx);
                page.hide();
                page
            });
            let observed = owner
                .upgrade()
                .ok_or_else(|| sailry_protocol::Fault::invalid("card is unavailable"))?;
            Ok(cx.new(|cx| {
                let subscription = cx.observe(&observed, |browser: &mut Self, _, cx| {
                    if !browser.active(cx) {
                        browser.hide(cx);
                    }
                    cx.notify();
                });
                Self {
                    source,
                    owner,
                    native,
                    _owner: subscription,
                }
            }))
        }
        #[cfg(not(target_os = "macos"))]
        {
            // Windows incognito is ignored by older WebView2 runtimes. Until
            // isolation is verified, the independently declared external action remains available.
            let _ = (owner, source, url, target, redirect, window, cx);
            Err(sailry_protocol::Fault::new(
                sailry_protocol::ErrorCode::NotConfigured,
                "isolated embedded browser is unavailable",
            ))
        }
    }

    pub(crate) fn active(&self, cx: &App) -> bool {
        self.owner
            .upgrade()
            .is_some_and(|owner| owner.read(cx).active(cx))
    }

    pub(crate) fn hide(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        self.native.update(cx, |native, _| native.hide());
        #[cfg(not(target_os = "macos"))]
        let _ = cx;
    }
}

impl Render for Browser {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.active(cx) {
            self.hide(cx);
            return div().into_any_element();
        }
        #[cfg(target_os = "macos")]
        {
            self.native.update(cx, |native, _| native.show());
            return div()
                .size_full()
                .min_w_0()
                .min_h_0()
                .child(self.native.clone())
                .into_any_element();
        }
        #[cfg(not(target_os = "macos"))]
        div().into_any_element()
    }
}

impl Card {
    pub(super) fn open_browser(
        &mut self,
        name: String,
        mode: card::Mode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active(cx) {
            return;
        }
        let Some(host) = self.mounted.as_ref().map(|mounted| mounted.host.clone()) else {
            return;
        };
        let Ok(target) = host.target(&name, mode) else {
            return;
        };
        if matches!(target.source, card::Source::McpAuthorization) && !self.authorization.can_open()
        {
            self.toast(crate::tr("plugins_failed").to_string(), true, window, cx);
            return;
        }
        let url = match &target.source {
            card::Source::Props { pointer } => host
                .props()
                .pointer(pointer)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            card::Source::McpAuthorization => self
                .authorization
                .url
                .as_ref()
                .filter(|(_, expiry)| *expiry > chrono::Utc::now().timestamp_millis().max(0) as u64)
                .map(|(url, _)| url.clone()),
        };
        let Some(url) = url.filter(|url| allowed(url, &target, None)) else {
            self.toast(crate::tr("plugins_failed").to_string(), true, window, cx);
            return;
        };
        let redirect = self.authorization.redirect.clone();
        let source = self.source.clone();
        let client = self.binding.client.clone();
        let request = client.prepare(Command::ReadPluginView {
            package: self.summary.package.clone(),
            surface: plugin::desktop::Surface::Card(self.summary.receipt),
        });
        let job = self
            .binding
            .runtime
            .spawn(async move { client.execute(request).await });
        self.browser_task = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |card, window, cx| {
                if !card.active(cx) {
                    return;
                }
                let auth_matches = !matches!(target.source, card::Source::McpAuthorization)
                    || card
                        .authorization
                        .url
                        .as_ref()
                        .is_some_and(|(current, expiry)| {
                            current == &url
                                && *expiry > chrono::Utc::now().timestamp_millis().max(0) as u64
                        });
                if !matches!(result, Ok(Ok(Output::PluginView(_)))) || !auth_matches {
                    card.toast(crate::tr("plugins_failed").to_string(), true, window, cx);
                    return;
                }
                match mode {
                    card::Mode::External => cx.open_url(&url),
                    card::Mode::Embedded => crate::resources::cards::open(
                        cx.weak_entity(),
                        source,
                        url,
                        target,
                        redirect,
                        window,
                        cx,
                    ),
                }
            });
        }));
    }
}
