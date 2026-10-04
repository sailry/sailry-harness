//! Native process menus capture the sampled identity, independent of sorting and refreshes.
use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{native_menu::NativeMenu, notification::Notification};
use sailry_protocol::{Command, ErrorCode, Output, host::metrics::Process};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Operation {
    Details,
    CopyPid,
    CopyName,
    Stop,
    ForceStop,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = host_process, no_json)]
pub(super) struct Dispatch {
    pub node: NodeId,
    pub process: Process,
    pub operation: Operation,
}

impl Monitor {
    pub(super) fn process_menu(
        &self,
        process: &Process,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.focus.focus(window, cx);
        if !cfg!(any(target_os = "macos", target_os = "windows")) {
            crate::feedback::toast(
                window,
                tr("workspace_native_menu_platform"),
                Notification::error(tr("workspace_native_menu_platform")),
                cx,
            );
            return;
        }
        let mut menu = NativeMenu::new();
        for (operation, label) in [
            (Operation::Details, "host_process_details"),
            (Operation::CopyPid, "host_process_copy_pid"),
            (Operation::CopyName, "host_process_copy_name"),
            (Operation::Stop, "host_process_stop"),
            (Operation::ForceStop, "host_process_force_stop"),
        ] {
            let stop = matches!(operation, Operation::Stop | Operation::ForceStop);
            if operation == Operation::Stop {
                menu = menu.separator();
            }
            menu = menu.menu_with_disabled(
                tr(label),
                stop && (self.stopping || process.pid <= 1 || process.started_at_secs == 0),
                Box::new(Dispatch {
                    node: self.node,
                    process: process.clone(),
                    operation,
                }),
            );
        }
        menu.show(event.position, window, cx);
    }

    pub(super) fn process_action(
        &mut self,
        action: &Dispatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action.node != self.node {
            return;
        }
        cx.stop_propagation();
        let process = action.process.clone();
        match action.operation {
            Operation::CopyPid => {
                cx.write_to_clipboard(ClipboardItem::new_string(process.pid.to_string()))
            }
            Operation::CopyName => cx.write_to_clipboard(ClipboardItem::new_string(process.name)),
            Operation::Details => details(process, window, cx),
            Operation::Stop | Operation::ForceStop => {
                if self.stopping {
                    return;
                }
                let force = action.operation == Operation::ForceStop;
                let label = tr(if force {
                    "host_process_force_stop"
                } else {
                    "host_process_stop"
                });
                let detail = format!(
                    "{} · PID {}\n{}",
                    process.name,
                    process.pid,
                    tr(if force {
                        "host_process_force_confirm"
                    } else {
                        "host_process_stop_confirm"
                    })
                );
                let owner = cx.entity().downgrade();
                crate::prompts::confirm(
                    &label,
                    &detail,
                    label.clone(),
                    window,
                    cx,
                    move |window, cx| {
                        let _ = owner
                            .update(cx, |this, cx| this.stop_process(process, force, window, cx));
                    },
                );
            }
        }
    }

    fn stop_process(
        &mut self,
        process: Process,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.stopping {
            return;
        }
        self.stopping = true;
        let client = self.client.clone();
        let task = self.runtime.spawn(async move {
            client
                .execute(client.prepare(Command::StopHostProcess {
                    pid: process.pid,
                    started_at_secs: process.started_at_secs,
                    force,
                }))
                .await
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update_in(cx, |this, window, cx| {
                this.stopping = false;
                match result {
                    Ok(Ok(Output::HostProcessSignalled { .. })) => {
                        crate::feedback::info("", &tr("host_process_stop_sent"), window, cx)
                    }
                    result => {
                        let key = match result {
                            Ok(Err(error)) => match error.code {
                                ErrorCode::NotFound => "host_process_gone",
                                ErrorCode::Conflict => "host_process_changed",
                                ErrorCode::PermissionDenied => "host_process_protected",
                                ErrorCode::OutcomeUnknown => "host_process_uncertain",
                                _ => "host_process_stop_failed",
                            },
                            _ => "host_process_uncertain",
                        };
                        crate::feedback::error("", &tr(key), window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

fn details(process: Process, window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    window.open_dialog(cx, move |dialog, window, cx| {
        dialog
            .form_title(tr("host_process_details"))
            .w((window.viewport_size().width - px(48.)).min(px(420.)))
            .child(
                v_flex()
                    .gap_1p5()
                    .text_sm()
                    .line_height(relative(1.3))
                    .children(
                        [
                            ("host_process_name", process.name.clone()),
                            ("host_pid", process.pid.to_string()),
                            (
                                "metrics_cpu",
                                super::resources::percent(process.cpu_basis_points),
                            ),
                            (
                                "metrics_memory",
                                super::resources::bytes(process.memory_bytes),
                            ),
                        ]
                        .into_iter()
                        .map(|(label, value)| {
                            h_flex()
                                .gap_3()
                                .items_start()
                                .child(
                                    div()
                                        .w_12()
                                        .flex_shrink_0()
                                        .debug_selector(move || {
                                            format!("process-detail-label-{label}")
                                        })
                                        .text_color(cx.theme().muted_foreground)
                                        .child(tr(label)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .debug_selector(move || {
                                            format!("process-detail-value-{label}")
                                        })
                                        .child(value),
                                )
                        }),
                    ),
            )
    });
}
