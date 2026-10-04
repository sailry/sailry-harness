use super::*;
use sailry_protocol::{SessionId, WorktreeId, conversation::Image};

/// Image metadata selects an existing authenticated download source.
#[derive(Clone)]
pub(crate) enum ImageSource {
    Attachment(Attachment),
    Image {
        session: SessionId,
        image: Image,
    },
    File {
        worktree: WorktreeId,
        path: String,
        context: Option<sailry_protocol::plugin::Context>,
    },
    Local {
        key: usize,
        source: Local,
    },
}

impl From<Attachment> for ImageSource {
    fn from(attachment: Attachment) -> Self {
        Self::Attachment(attachment)
    }
}

impl From<&Attachment> for ImageSource {
    fn from(attachment: &Attachment) -> Self {
        Self::Attachment(attachment.clone())
    }
}

impl ImageSource {
    pub(super) fn attachment(&self) -> Option<&Attachment> {
        match self {
            Self::Attachment(attachment) => Some(attachment),
            Self::Image { image, .. } => Some(&image.attachment),
            Self::File { .. } | Self::Local { .. } => None,
        }
    }

    pub(crate) fn name(&self) -> String {
        match self {
            Self::Local { source, .. } => source.name().unwrap_or_default(),
            Self::File { path, .. } => path.rsplit('/').next().unwrap_or(path).to_owned(),
            _ => self.attachment().unwrap().spec.name.clone(),
        }
    }

    pub(crate) fn description(&self) -> SharedString {
        let name = self.name();
        let kind = std::path::Path::new(&name)
            .extension()
            .and_then(|extension| extension.to_str())
            .filter(|extension| !extension.is_empty())
            .map(str::to_ascii_uppercase)
            .unwrap_or_else(|| {
                self.attachment()
                    .map(|attachment| attachment.spec.media_type.clone())
                    .unwrap_or_else(|| tr("chat_attachment_file").to_string())
            });
        let size = self
            .attachment()
            .map(|attachment| attachment.spec.size)
            .or_else(|| match self {
                Self::Local {
                    source: Local::Image { bytes, .. },
                    ..
                } => Some(bytes.len() as u64),
                _ => None,
            });
        match size {
            Some(size) => rust_i18n::t!(
                "chat_attachment_metadata",
                kind = kind,
                size = byte_size(size),
            )
            .to_string()
            .into(),
            None => kind.into(),
        }
    }

    pub(crate) fn format(&self) -> Option<image::ImageFormat> {
        match self {
            Self::Local { source, .. } => source.image_format(),
            Self::File { path, .. } => file_format(path),
            _ => decode::format(self.attachment().unwrap()),
        }
    }

    pub(crate) fn id(&self) -> String {
        match self {
            Self::Local { key, .. } => format!("local-{key}"),
            Self::File { worktree, path, .. } => format!("file-{worktree}-{path}"),
            _ => self.attachment().unwrap().id.to_string(),
        }
    }

    pub(super) fn key(&self) -> Key {
        match self {
            Self::Local { key, .. } => Key::Local(*key),
            Self::File { worktree, path, .. } => Key::File(*worktree, path.clone()),
            _ => Key::Published(self.attachment().unwrap().id),
        }
    }

    pub(super) fn command(&self) -> Option<Command> {
        match self {
            Self::Attachment(attachment) => Some(Command::DownloadAttachment {
                worktree: attachment.spec.worktree,
                attachment: attachment.id,
            }),
            Self::Image { session, image } => Some(Command::DownloadImage {
                session: *session,
                image: image.clone(),
            }),
            Self::File { worktree, path, .. } => Some(Command::DownloadFile {
                worktree: *worktree,
                path: path.clone(),
            }),
            Self::Local { .. } => None,
        }
    }

    pub(crate) fn download(
        &self,
        client: Arc<Client>,
        runtime: Arc<tokio::runtime::Runtime>,
        window: &mut Window,
        cx: &mut App,
    ) {
        match self {
            Self::Attachment(attachment) => crate::downloads::attachment(
                client.clone(),
                runtime.clone(),
                attachment.clone(),
                window,
                cx,
            ),
            Self::Image { session, image } => crate::downloads::image(
                client.clone(),
                runtime.clone(),
                *session,
                image.clone(),
                window,
                cx,
            ),
            Self::File {
                worktree,
                path,
                context,
            } => crate::downloads::local(
                client.clone(),
                runtime.clone(),
                match context {
                    Some(context) => crate::downloads::transfer::Source::ScopedFile {
                        context: context.clone(),
                        path: path.clone(),
                    },
                    None => crate::downloads::transfer::Source::File {
                        worktree: *worktree,
                        path: path.clone(),
                    },
                },
                self.name(),
                window,
                cx,
            ),
            Self::Local { source, .. } => {
                let download = match source {
                    Local::Path(path) => {
                        crate::downloads::transfer::Source::LocalPath(path.clone())
                    }
                    Local::Image { bytes, .. } => {
                        crate::downloads::transfer::Source::Bytes(bytes.clone())
                    }
                };
                crate::downloads::local(
                    client.clone(),
                    runtime.clone(),
                    download,
                    self.name(),
                    window,
                    cx,
                );
            }
        }
    }
}

fn byte_size(size: u64) -> String {
    let (value, key) = if size >= 1024 * 1024 {
        (
            format!("{:.1}", size as f64 / (1024. * 1024.)),
            "chat_attachment_mebibytes",
        )
    } else if size >= 1024 {
        (
            format!("{:.1}", size as f64 / 1024.),
            "chat_attachment_kibibytes",
        )
    } else {
        (size.to_string(), "chat_attachment_bytes")
    };
    rust_i18n::t!(key, value = value).to_string()
}

pub(super) fn file_format(path: &str) -> Option<image::ImageFormat> {
    let extension = std::path::Path::new(path).extension()?.to_str()?;
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some(image::ImageFormat::Png),
        "jpg" | "jpeg" => Some(image::ImageFormat::Jpeg),
        "gif" => Some(image::ImageFormat::Gif),
        "webp" => Some(image::ImageFormat::WebP),
        "tif" | "tiff" => Some(image::ImageFormat::Tiff),
        "bmp" => Some(image::ImageFormat::Bmp),
        _ => None,
    }
}
