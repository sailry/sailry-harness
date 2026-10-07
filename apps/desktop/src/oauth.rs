//! Browser return links activate the existing controller without carrying credentials.
use futures::{StreamExt, channel::mpsc};
use gpui_kit::{App, Application};

pub(crate) mod page;

pub(crate) const RETURN_URL: &str = "sailry://return";

pub(crate) fn listen(application: &Application) -> mpsc::UnboundedReceiver<()> {
    let (sender, receiver) = mpsc::unbounded();
    application.on_open_urls(move |urls| {
        if urls.iter().any(|url| accepts(url)) {
            let _ = sender.unbounded_send(());
        }
    });
    receiver
}

pub(crate) fn init(mut receiver: mpsc::UnboundedReceiver<()>, cx: &mut App) {
    cx.spawn(async move |cx| {
        while receiver.next().await.is_some() {
            let _ = cx.update(activate);
        }
    })
    .detach();
}

fn accepts(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    url.scheme() == "sailry"
        && url.host_str() == Some("return")
        && matches!(url.path(), "" | "/")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

fn activate(cx: &mut App) {
    cx.activate(true);
    if let Some(window) = cx.active_window().or_else(|| cx.windows().first().copied()) {
        let _ = window.update(cx, |_, window, _| window.activate_window());
    }
}

#[cfg(test)]
mod tests;
