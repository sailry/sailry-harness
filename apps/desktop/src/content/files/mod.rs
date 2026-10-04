//! File outputs share one card and dispatch previews through declared package renderers.
use crate::{
    conversation::live::{Binding, Event, View as Chat},
    tr,
};
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        menu::{DropdownMenu, PopupMenuItem},
        *,
    },
    *,
};
use sailry_protocol::{WorktreeId, tool::File};
mod card;
mod panel;
mod reference;
pub(crate) use card::Card;
pub(crate) use panel::Panel;
pub(crate) use reference::from_link;
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[path = "preview.rs"]
mod html;

fn download(binding: &Binding, file: &File, window: &mut Window, cx: &mut App) {
    if let Some(worktree) = binding.worktree {
        crate::content::images::ImageSource::File {
            context: None,
            worktree,
            path: file.path.clone(),
        }
        .download(binding.client.clone(), binding.runtime.clone(), window, cx);
    }
}

fn kind(file: &File) -> SharedString {
    match file
        .path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "docx" | "doc" => "Word".into(),
        "xlsx" | "xls" => "Excel".into(),
        "pptx" | "ppt" => "PowerPoint".into(),
        "pdf" => "PDF".into(),
        "html" | "htm" => "HTML".into(),
        _ => file.mime.clone().into(),
    }
}

#[cfg(all(feature = "workload-tests", target_os = "macos"))]
pub(crate) fn verify_native(output: &std::path::Path) {
    html::check::run(output);
}

/// Native document surfaces keep platform handles and bytes outside plugin JavaScript.
pub(crate) fn preview(
    binding: Binding,
    file: File,
    source: WeakEntity<Chat>,
    context: sailry_protocol::plugin::Context,
    cx: &mut App,
) -> Option<AnyView> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        html::create(binding, file, source, Some(context), cx)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (binding, file, source, context, cx);
        None
    }
}

/// Formats that use the controller's registered system viewer by default.
pub(crate) fn external(path: &str) -> bool {
    mime_guess::from_path(path).first().is_some_and(|mime| {
        matches!(mime.type_().as_str(), "audio" | "video" | "font" | "model")
            || mime.essence_str() == "application/pdf"
            || (mime.type_().as_str() == "image" && mime.essence_str() != "image/svg+xml")
    })
}
