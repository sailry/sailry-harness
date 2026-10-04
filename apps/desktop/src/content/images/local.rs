use crate::resources::file_source;
use gpui_kit::ImageFormat;
use sailry_link::CancellationToken;
use sailry_protocol::attachment::MAX_BYTES;
use std::{io::Read, path::PathBuf, sync::Arc};

#[derive(Clone)]
pub(crate) enum Local {
    Path(PathBuf),
    Image {
        name: String,
        format: ImageFormat,
        bytes: Arc<[u8]>,
    },
}

impl Local {
    pub(crate) fn image_format(&self) -> Option<image::ImageFormat> {
        let mime = match self {
            Self::Path(path) => mime_guess::from_path(path).first()?.to_string(),
            Self::Image { format, .. } => format.mime_type().into(),
        };
        match mime.as_str() {
            "image/png" => Some(image::ImageFormat::Png),
            "image/jpeg" => Some(image::ImageFormat::Jpeg),
            "image/gif" => Some(image::ImageFormat::Gif),
            "image/webp" => Some(image::ImageFormat::WebP),
            "image/tiff" => Some(image::ImageFormat::Tiff),
            "image/bmp" => Some(image::ImageFormat::Bmp),
            _ => None,
        }
    }

    pub(crate) fn image_bytes(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Arc<[u8]>, &'static str> {
        if cancel.is_cancelled() {
            return Err("chat_image_unavailable");
        }
        match self {
            Self::Path(path) => {
                let source = file_source::Source::open_limited(path, cancel, MAX_BYTES)?;
                let mut bytes = Vec::new();
                source
                    .file
                    .take(MAX_BYTES)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "chat_image_unavailable")?;
                Ok(bytes.into())
            }
            Self::Image { bytes, .. } if bytes.len() as u64 <= MAX_BYTES => Ok(bytes.clone()),
            _ => Err("chat_image_too_large"),
        }
    }

    pub fn name(&self) -> Option<String> {
        match self {
            Self::Path(path) => path.file_name()?.to_str().map(str::to_owned),
            Self::Image { name, .. } => Some(name.clone()),
        }
    }
}
