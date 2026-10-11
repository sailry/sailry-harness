use super::*;
use base64::Engine;
use std::time::Duration;

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
        self.annotation.target = Some(target.clone());
        self.annotation.task = Some(cx.spawn_in(window, async move |browser, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(120))
                .await;
            let _ = browser.update_in(cx, |browser, _, cx| {
                if browser.annotation_matches(&target) {
                    browser.cancel_annotation(cx);
                    browser.changed(cx);
                }
            });
        }));
        self.changed(cx);
        Ok(())
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
            Message::Element { token, element }
                if token == target.token && !self.annotation.captured =>
            {
                if element.url != target.url || !element.validate() {
                    self.cancel_annotation(cx);
                    self.annotation.error = Some("browser_annotation_invalid");
                    self.changed(cx);
                    return;
                }
                self.annotation.captured = true;
                let capture = self.screenshot(id as u64, window, cx);
                self.annotation.task = Some(cx.spawn_in(window, async move |browser, cx| {
                    let result = capture.await.and_then(image);
                    let _ = browser.update_in(cx, |browser, _, cx| {
                        if !browser.annotation_matches(&target) {
                            return;
                        }
                        browser.annotation.task = None;
                        match result {
                            Ok(bytes) => {
                                browser.annotation.capture = Some(Capture {
                                    element,
                                    preview: Arc::new(Image::from_bytes(
                                        ImageFormat::Jpeg,
                                        bytes.to_vec(),
                                    )),
                                    image: bytes,
                                });
                                browser.changed(cx);
                            }
                            Err(_) => {
                                browser.cancel_annotation(cx);
                                browser.annotation.error = Some("browser_annotation_unavailable");
                                browser.changed(cx);
                            }
                        }
                    });
                }));
                self.changed(cx);
            }
            _ => {}
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

fn image(value: serde_json::Value) -> Result<Arc<[u8]>, sailry_protocol::Fault> {
    let failed = || sailry_protocol::Fault::invalid("Browser screenshot is invalid");
    if value["mime_type"] != "image/jpeg" {
        return Err(failed());
    }
    let encoded = value["base64_image"]
        .as_str()
        .filter(|value| value.len() <= 256 * 1024)
        .ok_or_else(failed)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| failed())?;
    let dimensions =
        image::ImageReader::with_format(std::io::Cursor::new(&bytes), image::ImageFormat::Jpeg)
            .into_dimensions()
            .map_err(|_| failed())?;
    if dimensions.0 == 0 || dimensions.1 == 0 || dimensions.0 > 960 || dimensions.1 > 960 {
        return Err(failed());
    }
    Ok(bytes.into())
}
