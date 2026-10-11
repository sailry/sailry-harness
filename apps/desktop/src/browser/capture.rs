use super::*;
use sailry_protocol::{ErrorCode, Fault, browser};

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
        cx.spawn_in(window, async move |_, cx| {
            let timer = cx
                .background_executor()
                .timer(std::time::Duration::from_secs(5));
            let buffer = tokio::select! {
                result = receive => result.map_err(|_| failed())?.map_err(|_| failed())?,
                _ = timer => return Err(failed()),
            };
            cx.background_executor()
                .spawn(async move { encode(tab, &buffer.bytes, buffer.format) })
                .await
        })
    }
}

fn failed() -> Fault {
    Fault::new(ErrorCode::Unavailable, "browser capture failed")
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
