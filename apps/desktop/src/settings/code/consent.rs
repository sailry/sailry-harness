use super::*;
use crate::{theme::DialogStyle as _, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    scroll::ScrollableElement,
};
use sailry_protocol::Command;

pub(super) fn install(
    owner: Entity<Panel>,
    install: code::Install,
    window: &mut Window,
    cx: &mut App,
) {
    let details = vec![
        ("code_package", install.package.clone()),
        ("code_version", install.version.clone()),
        ("code_source", install.purl.clone()),
        ("code_downloads", install.urls.join("\n")),
        ("code_licenses", install.licenses.join(", ")),
        (
            "code_install_command",
            install
                .command
                .as_ref()
                .map(|command| serde_json::to_string(command).expect("command serialization"))
                .unwrap_or_else(|| tr("code_direct_download").to_string()),
        ),
    ];
    open(
        owner,
        "code_install",
        "code_install",
        details,
        Command::InstallCodeServer { install },
        window,
        cx,
    );
}

pub(super) fn uninstall(
    owner: Entity<Panel>,
    package: String,
    version: String,
    window: &mut Window,
    cx: &mut App,
) {
    let details = vec![
        ("code_package", package.clone()),
        ("code_version", version.clone()),
    ];
    open(
        owner,
        "code_uninstall",
        "code_uninstall",
        details,
        Command::UninstallCodeServer { package, version },
        window,
        cx,
    );
}

fn open(
    owner: Entity<Panel>,
    title: &'static str,
    action: &'static str,
    details: Vec<(&'static str, String)>,
    command: Command,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let submit = owner.clone();
        let command = command.clone();
        let content = v_flex()
            .gap_3()
            .id("code-consent-details")
            .debug_selector(|| "code-consent-details".into())
            .max_h((window.viewport_size().height - px(260.)).max(px(160.)))
            .overflow_y_scrollbar()
            .children(details.iter().map(|(key, value)| {
                let key = *key;
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr(key)),
                    )
                    .child(
                        div()
                            .debug_selector(move || format!("code-consent-{key}"))
                            .whitespace_normal()
                            .child(value.clone()),
                    )
            }));
        dialog
            .form_title(tr(title))
            .w((window.viewport_size().width - px(48.)).min(px(600.)))
            .overlay_closable(false)
            .child(content)
            .footer(
                DialogFooter::new()
                    .w_full()
                    .gap_2()
                    .child(
                        Button::new("code-consent-cancel")
                            .debug_selector(|| "code-consent-cancel".into())
                            .label(tr("settings_cancel"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("code-consent-submit")
                            .primary()
                            .debug_selector(|| "code-consent-submit".into())
                            .label(tr(action))
                            .on_click(move |_, window, cx| {
                                submit.update(cx, |panel, cx| {
                                    panel.submit(command.clone(), window, cx)
                                });
                                window.close_dialog(cx);
                            }),
                    ),
            )
    });
}
