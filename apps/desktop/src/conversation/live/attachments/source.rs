use crate::resources::file_source;
use gpui_kit::ImageFormat;
use sailry_link::CancellationToken;
use sailry_protocol::{
    WorktreeId,
    attachment::{MAX_BYTES, Spec},
};
use std::{io::Cursor, sync::Arc};
use tokio::io::AsyncRead;

pub(in crate::conversation::live) use crate::content::images::Local as Source;

pub(super) struct Prepared {
    pub spec: Spec,
    pub reader: Box<dyn AsyncRead + Send + Unpin>,
}

impl Source {
    pub(super) async fn prepare(
        self,
        worktree: WorktreeId,
        cancel: CancellationToken,
    ) -> Result<Prepared, &'static str> {
        let name = self.name().ok_or("files_upload_name")?;
        tokio::task::spawn_blocking(move || {
            if cancel.is_cancelled() {
                return Err("files_upload_cancelled");
            }
            let (media_type, size, revision, reader): (_, _, _, Box<dyn AsyncRead + Send + Unpin>) =
                match self {
                    Self::Path(path) => {
                        let media_type = mime_guess::from_path(&path)
                            .first_or_octet_stream()
                            .to_string();
                        let source = file_source::Source::open_limited(&path, &cancel, MAX_BYTES)?;
                        (
                            media_type,
                            source.size,
                            source.revision,
                            Box::new(tokio::fs::File::from_std(source.file)),
                        )
                    }
                    Self::Image { format, bytes, .. } => {
                        if bytes.len() as u64 > MAX_BYTES {
                            return Err("files_upload_too_large");
                        }
                        // Native clipboards commonly supply TIFF or BMP; publish PNG
                        // so model providers can consume the pasted image directly.
                        let (format, bytes) = match format {
                            ImageFormat::Tiff | ImageFormat::Bmp => {
                                let decoded = super::images::decode::png(
                                    &bytes,
                                    if format == ImageFormat::Tiff {
                                        image::ImageFormat::Tiff
                                    } else {
                                        image::ImageFormat::Bmp
                                    },
                                )?;
                                (ImageFormat::Png, Arc::<[u8]>::from(decoded))
                            }
                            _ => (format, bytes),
                        };
                        let size = bytes.len() as u64;
                        if size > MAX_BYTES {
                            return Err("files_upload_too_large");
                        }
                        let revision = blake3::hash(&bytes).to_hex().to_string();
                        (
                            format.mime_type().to_owned(),
                            size,
                            revision,
                            Box::new(Cursor::new(bytes)),
                        )
                    }
                };
            Ok(Prepared {
                spec: Spec {
                    worktree,
                    name,
                    media_type,
                    size,
                    revision,
                },
                reader,
            })
        })
        .await
        .map_err(|_| "files_upload_source")?
    }
}
