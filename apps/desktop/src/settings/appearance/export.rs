//! Native ZIP destination selection keeps the installed package as the source.
use super::*;
use std::path::PathBuf;
use theme::package::Loaded;

pub(super) fn choose(id: &str, window: &mut Window, cx: &mut App) {
    let catalog = cx.global::<theme::Catalog>();
    let source = catalog
        .root
        .as_ref()
        .filter(|_| {
            catalog
                .packages
                .iter()
                .any(|package| package.id == id && package.manifest.is_some())
        })
        .map(|root| root.join(id));
    let Some(source) = source else {
        crate::feedback::error("", &tr("appearance_unavailable"), window, cx);
        return;
    };
    let name = format!("{id}.zip");
    let directory = std::env::home_dir().unwrap_or_default();
    let selected = cx.prompt_for_new_path(&directory, Some(&name));
    window
        .spawn(cx, async move |cx| {
            let result = selected.await;
            let _ = cx.update(|window, cx| match result {
                Ok(Ok(Some(path))) => save(source, path, window, cx),
                Ok(Ok(None)) => {}
                _ => crate::feedback::error("", &tr("appearance_picker_failed"), window, cx),
            });
        })
        .detach();
}

fn save(source: PathBuf, destination: PathBuf, window: &mut Window, cx: &mut App) {
    let work = cx
        .background_executor()
        .spawn(async move { Loaded::read(&source, true)?.export(&destination) });
    window
        .spawn(cx, async move |cx| {
            let result = work.await;
            let _ = cx.update(|window, cx| match result {
                Ok(()) => crate::feedback::toast(
                    window,
                    tr("appearance_export_done"),
                    notification::Notification::success(tr("appearance_export_done")),
                    cx,
                ),
                Err(error) => {
                    eprintln!("could not export theme: {error}");
                    crate::feedback::error("", &tr("appearance_export_failed"), window, cx);
                }
            });
        })
        .detach();
}
