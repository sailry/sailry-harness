//! Inline package composition uses an isolated Kit script runtime and narrow services.
mod authorization;
pub(crate) mod browser;
mod host;
pub(crate) mod qr;
mod runtime;
mod settings;
#[cfg(test)]
mod tests;

use crate::conversation::live::{Binding, View as Chat};
use gpui_kit::component::button::Button;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{component::*, *};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, Output,
    plugin::{self, card},
};
use std::sync::Arc;

pub(crate) struct Card {
    pub(crate) source: WeakEntity<Chat>,
    pub(crate) summary: card::Summary,
    binding: Binding,
    declaration: Option<card::Declaration>,
    mounted: Option<runtime::Mounted>,
    authorization: authorization::Login,
    loading: bool,
    attempted: bool,
    error: Option<&'static str>,
    stop: CancellationToken,
    load: Option<Task<()>>,
    clock: Option<Task<()>>,
    browser_task: Option<Task<()>>,
    settings_task: Option<Task<()>>,
    observed: bool,
    pending_toast: Option<(String, bool)>,
    painted: Arc<std::sync::atomic::AtomicBool>,
    visible: Arc<std::sync::atomic::AtomicBool>,
    _source: Subscription,
}

impl Card {
    pub(crate) fn new(
        source: WeakEntity<Chat>,
        binding: Binding,
        summary: card::Summary,
        cx: &mut Context<Self>,
    ) -> Self {
        let observer = cx.observe(
            &source.upgrade().expect("card source exists"),
            |card, _, cx| {
                card.sync(cx);
                cx.notify();
            },
        );
        Self {
            source,
            summary,
            binding,
            declaration: None,
            mounted: None,
            authorization: Default::default(),
            loading: false,
            attempted: false,
            error: None,
            stop: CancellationToken::new(),
            load: None,
            clock: None,
            browser_task: None,
            settings_task: None,
            observed: false,
            pending_toast: None,
            painted: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            visible: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            _source: observer,
        }
    }

    pub(crate) fn active(&self, cx: &App) -> bool {
        self.visible.load(std::sync::atomic::Ordering::Acquire)
            && self.reference(cx).is_some()
            && self
                .mounted
                .as_ref()
                .is_some_and(|mounted| mounted.host.active())
    }

    fn reference(&self, cx: &App) -> Option<plugin::Reference> {
        self.source
            .upgrade()?
            .read(cx)
            .card_reference(&self.summary)
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        let Some(reference) = self.reference(cx) else {
            self.suspend();
            return;
        };
        if let Some(mounted) = &self.mounted {
            mounted.host.reference(reference);
        }
    }

    pub(crate) fn suspend(&mut self) {
        self.stop.cancel();
        self.load = None;
        self.loading = false;
        self.clock = None;
        self.mounted = None;
        self.browser_task = None;
        self.settings_task = None;
        self.authorization.suspend();
        self.attempted = false;
        self.visible
            .store(false, std::sync::atomic::Ordering::Release);
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading
            || self.mounted.is_some()
            || self.attempted
            || self.reference(cx).is_none()
            || !self.visible.load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        self.attempted = true;
        self.loading = true;
        self.error = None;
        self.stop = CancellationToken::new();
        let stop = self.stop.clone();
        let client = self.binding.client.clone();
        let runtime = self.binding.runtime.handle().clone();
        let package = self.summary.package.clone();
        let surface = plugin::desktop::Surface::Card(self.summary.receipt);
        let key = self.summary.key.clone();
        let worker = runtime.clone();
        let job = runtime.spawn(async move {
            let metadata = client
                .execute(client.prepare(Command::ReadPluginVersion {
                    package: package.clone(),
                }))
                .await;
            let declaration = match metadata {
                Ok(Output::Plugin(info)) => info
                    .extension
                    .and_then(|extension| extension.cards.into_iter().find(|card| card.key == key)),
                _ => None,
            }
            .ok_or("plugins_view_read_failed")?;
            let cache = super::cache::prepare(client, worker, package, surface, stop)
                .await
                .ok_or("plugins_view_read_failed")??;
            Ok::<_, &'static str>((cache, declaration))
        });
        self.load = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |card, window, cx| {
                card.loading = false;
                if card.stop.is_cancelled() || card.reference(cx).is_none() {
                    return;
                }
                match result {
                    Ok(Ok((cache, declaration))) => {
                        let reference = card.reference(cx).expect("checked card source");
                        let host = host::Host::new(
                            card.binding.clone(),
                            &card.summary,
                            declaration.clone(),
                            reference,
                        );
                        match runtime::Mounted::new(cache, host, cx.weak_entity(), window, cx) {
                            Ok(mounted) => {
                                card.declaration = Some(declaration);
                                card.mounted = Some(mounted);
                                card.login_state(card.authorization.state.clone(), false);
                                if let Some(attempt) = card.authorization.attempt.clone() {
                                    card.observe_login(attempt, window, cx);
                                }
                                card.start_clock(window, cx);
                            }
                            Err(_) => card.error = Some("plugins_view_read_failed"),
                        }
                    }
                    _ => card.error = Some("plugins_view_read_failed"),
                }
                cx.notify();
            });
        }));
    }

    fn start_clock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stop = self.stop.clone();
        let painted = self.painted.clone();
        painted.store(true, std::sync::atomic::Ordering::Release);
        self.clock = Some(cx.spawn_in(window, async move |owner, cx| {
            loop {
                tokio::select! {
                    _ = stop.cancelled() => break,
                    _ = cx.background_executor().timer(std::time::Duration::from_secs(1)) => {},
                }
                if owner
                    .update(cx, |card, cx| {
                        card.refresh_mount(
                            painted.swap(false, std::sync::atomic::Ordering::AcqRel),
                            cx,
                        );
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn refresh_mount(&mut self, painted: bool, cx: &mut Context<Self>) {
        // Scrolling must not destroy a user-started callback or its private URL.
        // Session changes still release cards through the explicit suspend path.
        if !painted && !self.authorization.keeps_alive() {
            self.suspend();
        } else if let Some(mounted) = &self.mounted {
            if let Ok(view) = mounted
                .root
                .read(cx)
                .content()
                .clone()
                .downcast::<gpui_shell::ScriptView>()
            {
                view.update(cx, |view, cx| view.refresh(cx));
            }
        }
    }

    fn toast(&mut self, message: String, error: bool, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_kit::component::notification::Notification;
        let notification = if error {
            Notification::error(message.clone())
        } else {
            Notification::info(message.clone())
        };
        crate::feedback::toast(window, message.into(), notification, cx);
    }
}

impl Drop for Card {
    fn drop(&mut self) {
        self.stop.cancel();
        self.authorization.suspend();
    }
}

impl Render for Card {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.observed {
            self.observed = true;
            crate::feedback::observe(window, cx, |card: &Self, _| {
                card.error.into_iter().collect()
            });
        }
        self.sync(cx);
        self.mount(window, cx);
        if let Some((message, error)) = self.pending_toast.take() {
            self.toast(message, error, window, cx);
        }
        let selector = format!(
            "interactive-card-{}-{}",
            self.summary.receipt.call, self.summary.receipt.block
        );
        let painted = self.painted.clone();
        let visible = self.visible.clone();
        let owner = cx.weak_entity();
        let eligible = self.reference(cx).is_some();
        v_flex()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .relative()
            .w_full()
            .min_w_0()
            .gap_2()
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        let clipped = bounds.intersect(&window.content_mask().bounds);
                        if eligible && clipped.size.width > px(0.) && clipped.size.height > px(0.) {
                            painted.store(true, std::sync::atomic::Ordering::Release);
                            if !visible.swap(true, std::sync::atomic::Ordering::AcqRel) {
                                let owner = owner.clone();
                                window.defer(cx, move |_, cx| {
                                    let _ = owner.update(cx, |_, cx| cx.notify());
                                });
                            }
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(match &self.mounted {
                Some(mounted) if self.reference(cx).is_some() => {
                    mounted.root.read(cx).content().clone().into_any_element()
                }
                _ => div()
                    .p_3()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().muted)
                    .text_color(cx.theme().muted_foreground)
                    .child(self.summary.fallback.label(&rust_i18n::locale()).to_owned())
                    .into_any_element(),
            })
            .when(
                self.error.is_some() && self.reference(cx).is_some(),
                |body| {
                    body.child(
                        Button::new("card-retry")
                            .small()
                            .label(crate::tr("plugins_retry"))
                            .on_click(cx.listener(|card, _, window, cx| {
                                card.attempted = false;
                                card.error = None;
                                card.mount(window, cx);
                                cx.notify();
                            })),
                    )
                },
            )
    }
}
