use super::*;
use base64::Engine;
use std::time::Duration;

/// Picking stays active across picks and ends after this idle period.
const IDLE: Duration = Duration::from_secs(120);

impl Browser {
    pub(super) fn begin_annotation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let tab = self
            .tabs
            .get(self.selected)
            .filter(|tab| {
                !self.preview && self.mounted.get() && !tab.loading && tab.error.is_none()
            })
            .ok_or("browser_annotation_unavailable")?;
        let page = tab.page.as_ref().ok_or("browser_annotation_unavailable")?;
        if destination(&tab.url).is_none() {
            return Err("browser_annotation_unavailable".into());
        }
        let target = Target {
            token: sailry_protocol::RequestId::new().to_string(),
            tab: tab.id,
            url: tab.url.clone(),
            lease: self.lease,
            stop: CancellationToken::new(),
        };
        let script = script(&target.token);
        page.read(cx)
            .raw()
            .evaluate_script(&script)
            .map_err(|_| "browser_annotation_unavailable")?;
        self.annotation.error = None;
        self.annotation.target = Some(target);
        self.idle_annotation(window, cx);
        // Arrow keys, Enter and Escape reach the page script without a first click.
        self.focus_selected(window, cx);
        self.changed(cx);
        Ok(())
    }

    fn idle_annotation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.annotation.target.clone() else {
            return;
        };
        self.annotation.timeout = Some(cx.spawn_in(window, async move |browser, cx| {
            cx.background_executor().timer(IDLE).await;
            let _ = browser.update_in(cx, |browser, _, cx| {
                if browser.annotation_matches(&target) {
                    browser.cancel_annotation(cx);
                    browser.changed(cx);
                }
            });
        }));
    }

    pub(in crate::browser) fn select_annotation(
        &mut self,
        id: usize,
        message: Message,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self
            .annotation
            .target
            .clone()
            .filter(|target| target.tab == id && self.annotation_matches(target))
        else {
            return;
        };
        match message {
            Message::Cancelled { token } if token == target.token => {
                self.cancel_annotation(cx);
                self.changed(cx);
            }
            Message::Element {
                token,
                element,
                review,
            } if token == target.token && !self.annotation.reviewing => {
                if element.url != target.url || !element.validate() {
                    self.cancel_annotation(cx);
                    self.annotation.error = Some("browser_annotation_invalid");
                    self.changed(cx);
                    return;
                }
                self.annotation.reviewing = review;
                self.annotation.picks.push_back((element, review));
                self.idle_annotation(window, cx);
                if !self.annotation.capturing {
                    self.capture_pick(window, cx);
                }
            }
            _ => {}
        }
    }

    /// Captures queued picks one at a time so each crop sees an unobstructed page.
    fn capture_pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self
            .annotation
            .target
            .clone()
            .filter(|target| self.annotation_matches(target))
        else {
            return;
        };
        let Some((element, review)) = self.annotation.picks.pop_front() else {
            self.annotation.capturing = false;
            return;
        };
        self.annotation.capturing = true;
        self.annotation.error = None;
        let screenshot = self.screenshot(target.tab as u64, window, cx);
        self.annotation.work = Some(cx.spawn_in(window, async move |browser, cx| {
            let (bounds, viewport) = (element.bounds, element.viewport);
            let crop = match screenshot.await.map_err(|_| ()).and_then(decode) {
                Ok(bytes) => {
                    cx.background_executor()
                        .spawn(async move { crop::encode(&bytes, bounds, viewport) })
                        .await
                }
                Err(()) => Err(()),
            };
            let reply = browser
                .update_in(cx, |browser, _, cx| {
                    browser.finish_pick(&target, *element, review, crop, cx)
                })
                .ok()
                .flatten();
            let result = match reply {
                Some(reply) => reply.await.unwrap_or(Err("browser_annotation_stale")),
                None => Ok(()),
            };
            let _ = browser.update_in(cx, |browser, window, cx| {
                if !browser.annotation_matches(&target) {
                    return;
                }
                if let Err(key) = result {
                    browser.annotation.error = Some(key);
                }
                browser.annotation.capturing = false;
                if !review {
                    browser.resume_annotation(&target, cx);
                }
                browser.capture_pick(window, cx);
                browser.changed(cx);
            });
        }));
        self.changed(cx);
    }

    /// Hands a captured pick to the composer, or keeps it for the package's review.
    fn finish_pick(
        &mut self,
        target: &Target,
        element: Element,
        review: bool,
        crop: Result<Arc<[u8]>, ()>,
        cx: &mut Context<Self>,
    ) -> Option<tokio::sync::oneshot::Receiver<Result<(), &'static str>>> {
        if !self.annotation_matches(target) {
            return None;
        }
        let Ok(image) = crop else {
            if review {
                self.cancel_annotation(cx);
            }
            self.annotation.error = Some("browser_annotation_unavailable");
            self.changed(cx);
            return None;
        };
        if review {
            self.annotation.capture = Some(Capture {
                preview: Arc::new(Image::from_bytes(ImageFormat::Jpeg, image.to_vec())),
                element,
                image,
            });
            return None;
        }
        let (reply, receive) = tokio::sync::oneshot::channel();
        cx.emit(Transfer {
            selection: element.selection(image, String::new()),
            stop: target.stop.clone(),
            reply: RefCell::new(Some(reply)),
        });
        Some(receive)
    }

    fn resume_annotation(&self, target: &Target, cx: &App) {
        if let Some(page) = self
            .tabs
            .iter()
            .find(|tab| tab.id == target.tab)
            .and_then(|tab| tab.page.as_ref())
        {
            let _ = page.read(cx).raw().evaluate_script(&format!(
                "window.__sailryElementSelection?.resume({})",
                serde_json::to_string(&target.token).expect("Selection token serializes")
            ));
        }
    }
}

pub(super) fn script(token: &str) -> String {
    format!(
        "({})({})",
        include_str!("select.js"),
        serde_json::to_string(token).expect("Selection token serializes")
    )
}

/// The shared viewport screenshot, which another owner may encode as JPEG or PNG.
fn decode(value: serde_json::Value) -> Result<Vec<u8>, ()> {
    if !matches!(
        value["mime_type"].as_str(),
        Some("image/jpeg" | "image/png")
    ) {
        return Err(());
    }
    let encoded = value["base64_image"]
        .as_str()
        .filter(|value| value.len() <= 16 * 1024 * 1024)
        .ok_or(())?;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| ())
}
