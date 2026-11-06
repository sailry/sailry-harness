use super::*;
use sailry_protocol::{ErrorCode, Fault, browser};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
mod geometry;

impl Browser {
    pub(super) fn screenshot(
        &self,
        tab: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<browser::Result> {
        let Some(page) = self
            .tabs
            .iter()
            .find(|value| value.id as u64 == tab)
            .and_then(|tab| tab.page.as_ref())
        else {
            return Task::ready(Err(failed()));
        };
        let Ok(receive) = crate::ui::webview_capture::capture(page.read(cx).raw(), Some(960.))
        else {
            return Task::ready(Err(failed()));
        };
        let stop = self.stop.clone();
        cx.spawn_in(window, async move |_, cx| {
            let timer = cx
                .background_executor()
                .timer(std::time::Duration::from_secs(5));
            let buffer = tokio::select! {
                biased;
                _ = stop.cancelled() => return Err(super::automation::closed()),
                result = receive => result.map_err(|_| failed())?.map_err(|_| failed())?,
                _ = timer => return Err(failed()),
            };
            let encoding = cx
                .background_executor()
                .spawn(async move { encode(tab, &buffer.bytes, buffer.format) });
            tokio::select! {
                biased;
                _ = stop.cancelled() => Err(super::automation::closed()),
                result = encoding => result,
            }
        })
    }

    /// Agent captures measure the page first so the region matches the captured pixels.
    pub(super) fn capture(
        &self,
        tab: u64,
        target: Option<browser::Target>,
        scale: browser::Scale,
        format: browser::Format,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<browser::Result> {
        use super::automation::{loaded, response, script, unavailable};
        let stop = self.stop.clone();
        cx.spawn_in(window, async move |browser, cx| {
            let work = async {
                loaded(&browser, tab, Instant::now() + Duration::from_secs(20), cx).await?;
                let measure = |scroll: bool| {
                    script(json!({"action":"target","target":target,"scroll":scroll}))
                };
                if target.is_some() {
                    let receive = browser
                        .update_in(cx, |browser, _, cx| {
                            browser.evaluate(tab, &measure(true), cx)
                        })
                        .map_err(|_| unavailable())??;
                    response(receive, cx).await?;
                    // Measure after layout and painting settle at the scrolled position.
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;
                }
                let receive = browser
                    .update_in(cx, |browser, _, cx| {
                        browser.evaluate(tab, &measure(false), cx)
                    })
                    .map_err(|_| unavailable())??;
                let facts = response(receive, cx).await?;
                let plan = measured(&facts, scale)?;
                let receive = browser
                    .update_in(cx, |browser, _, cx| browser.pixels(tab, plan.snapshot, cx))
                    .map_err(|_| unavailable())??;
                let timer = cx.background_executor().timer(Duration::from_secs(5));
                let buffer = tokio::select! {
                    result = receive => result.map_err(|_| failed())?.map_err(|_| failed())?,
                    _ = timer => return Err(failed()),
                };
                let (bytes, width, height) = cx
                    .background_executor()
                    .spawn(async move { region(&buffer, plan, format) })
                    .await?;
                use base64::Engine;
                let mut value = described(tab, &facts, target, scale, plan);
                value["mime_type"] = format.mime().into();
                value["width"] = width.into();
                value["height"] = height.into();
                value["base64_image"] = base64::engine::general_purpose::STANDARD
                    .encode(bytes)
                    .into();
                Ok(value)
            };
            tokio::select! {
                biased;
                _ = stop.cancelled() => Err(super::automation::closed()),
                result = work => result,
            }
        })
    }

    fn pixels(
        &self,
        tab: u64,
        width: f64,
        cx: &App,
    ) -> Result<crate::ui::webview_capture::Pending, Fault> {
        let page = self
            .tabs
            .iter()
            .find(|value| value.id as u64 == tab)
            .and_then(|tab| tab.page.as_ref())
            .ok_or_else(failed)?;
        crate::ui::webview_capture::capture(page.read(cx).raw(), Some(width)).map_err(|_| failed())
    }
}

fn failed() -> Fault {
    Fault::new(ErrorCode::Unavailable, "browser capture failed")
}

fn measured(facts: &Value, scale: browser::Scale) -> Result<geometry::Plan, Fault> {
    let number = |value: &Value| value.as_f64().unwrap_or(f64::NAN);
    let bounds = &facts["bounds"];
    geometry::plan(
        geometry::Rect {
            x: number(&bounds["x"]),
            y: number(&bounds["y"]),
            width: number(&bounds["width"]),
            height: number(&bounds["height"]),
        },
        (
            number(&facts["viewport"]["width"]),
            number(&facts["viewport"]["height"]),
        ),
        number(&facts["ratio"]),
        scale,
    )
    .ok_or_else(|| {
        Fault::new(
            ErrorCode::Conflict,
            "capture region is outside the visible page; read the page again",
        )
    })
}

/// Crops the measured region and encodes it within the transport limit.
fn region(
    buffer: &crate::ui::webview_capture::Buffer,
    plan: geometry::Plan,
    format: browser::Format,
) -> Result<(Vec<u8>, u32, u32), Fault> {
    let image =
        image::load_from_memory_with_format(&buffer.bytes, buffer.format).map_err(|_| failed())?;
    let (x, y, width, height) =
        geometry::crop(plan.region, plan.viewport, (image.width(), image.height()));
    let cropped = image.crop_imm(x, y, width, height);
    let mut size = plan.output;
    loop {
        let resized = if (cropped.width(), cropped.height()) == size {
            cropped.clone()
        } else {
            cropped.resize_exact(size.0, size.1, image::imageops::FilterType::Triangle)
        };
        if let Some(bytes) = encoded(&resized, format) {
            return Ok((bytes, size.0, size.1));
        }
        if size.0.max(size.1) <= 256 {
            return Err(failed());
        }
        size = geometry::shrink(size);
    }
}

fn encoded(image: &image::DynamicImage, format: browser::Format) -> Option<Vec<u8>> {
    match format {
        browser::Format::Png => {
            let mut bytes = Vec::new();
            image
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Png,
                )
                .ok()?;
            (bytes.len() <= browser::MAX_IMAGE_BYTES).then_some(bytes)
        }
        browser::Format::Jpeg => {
            let pixels = image.to_rgb8();
            [85, 70, 55].into_iter().find_map(|quality| {
                let mut bytes = Vec::new();
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality)
                    .encode_image(&pixels)
                    .ok()?;
                (bytes.len() <= browser::MAX_IMAGE_BYTES).then_some(bytes)
            })
        }
    }
}

/// Text facts let models without vision use the capture too.
fn described(
    tab: u64,
    facts: &Value,
    target: Option<browser::Target>,
    scale: browser::Scale,
    plan: geometry::Plan,
) -> Value {
    let round = |value: f64| (value * 10.).round() / 10.;
    let mut value = json!({
        "tab": tab,
        "url": facts["url"],
        "title": facts["title"],
        "viewport": facts["viewport"],
        "device_pixel_ratio": plan.ratio,
        "bounds": {
            "x": round(plan.region.x),
            "y": round(plan.region.y),
            "width": round(plan.region.width),
            "height": round(plan.region.height),
        },
        "scale": scale,
    });
    if let Some(target) = target {
        value["target"] = json!(target);
    }
    for key in ["tag", "text"] {
        if let Some(text) = facts[key].as_str() {
            value[key] = text.into();
        }
    }
    value
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn encode(tab: u64, bytes: &[u8], format: image::ImageFormat) -> browser::Result {
    use base64::Engine;
    let mut image = image::load_from_memory_with_format(bytes, format).map_err(|_| failed())?;
    image = image.thumbnail(960, 960);
    // Leave room for the JSON envelope within the existing browser result limit.
    for size in [960, 720, 480, 320] {
        image = image.thumbnail(size, size);
        let mut data = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, 65)
            .encode_image(&image.to_rgb8())
            .map_err(|_| failed())?;
        if data.len() <= 180 * 1024 {
            return Ok(
                serde_json::json!({"tab":tab,"mime_type":"image/jpeg","width":image.width(),"height":image.height(),"base64_image":base64::engine::general_purpose::STANDARD.encode(data)}),
            );
        }
    }
    Err(failed())
}
