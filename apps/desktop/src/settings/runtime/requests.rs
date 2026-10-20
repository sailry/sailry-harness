use super::*;
use sailry_protocol::{ErrorCode, Fault, Output};

impl Panel {
    pub(super) fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.active {
            return;
        }
        if self.loading || self.locked() {
            self.refresh = true;
            return;
        }
        self.loading = true;
        self.refresh = false;
        let binding = self.binding.clone();
        let generation = self.generation;
        let job = self.binding.runtime.spawn(async move {
            let python = binding
                .client
                .execute(binding.client.prepare(Command::ReadPythonSettings))
                .await?;
            Ok::<_, Fault>(python)
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.loading = false;
                if !panel.active {
                    return;
                }
                if generation != panel.generation {
                    panel.load(window, cx);
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Ok(Output::PythonSettings(python))) => {
                        panel.accept_index(&python, window, cx);
                        panel.python = Some(python);
                        panel.error = None;
                        panel.fault = None;
                    }
                    Ok(Err(error)) => panel.fail(error, false),
                    Ok(Ok(_)) => panel.fail(Fault::invalid("unexpected runtime response"), false),
                    Err(error) => {
                        panel.fail(Fault::new(ErrorCode::Unavailable, error.to_string()), false)
                    }
                }
                if panel.refresh {
                    panel.load(window, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(in crate::settings) fn submit(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.locked() {
            return;
        }
        self.request = Some(self.binding.client.prepare(command));
        self.retry(window, cx);
    }

    pub(super) fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || !self.active {
            return;
        }
        let Some(request) = self.request.clone() else {
            return;
        };
        let binding = self.binding.clone();
        self.pending = true;
        self.error = None;
        self.fault = None;
        let origin = window.window_handle();
        let job = self
            .binding
            .runtime
            .spawn(async move { super::super::requests::execute(&binding, request).await });
        cx.spawn(async move |owner, cx| {
            let result = job.await;
            let Some(target) = cx.update(|cx| crate::installations::target(origin, cx)) else {
                return;
            };
            let _ = target.update(cx, |_, window, cx| {
                _ = owner.update(cx, |panel, cx| {
                    panel.pending = false;
                    match result {
                        Ok((Ok(output), _)) => {
                            panel.request = None;
                            if let Output::PythonSettings(settings) = output {
                                panel.python = Some(settings);
                            }
                            crate::feedback::info("", &crate::tr("knowledge_saved"), window, cx);
                            panel.load(window, cx);
                        }
                        Ok((Err(error), confirmed)) => {
                            let uncertain = !confirmed || error.code == ErrorCode::OutcomeUnknown;
                            if !uncertain {
                                panel.request = None;
                            }
                            if error.code != ErrorCode::Cancelled {
                                panel.fail(error, uncertain);
                            }
                        }
                        Err(error) => panel.fail(
                            Fault::new(ErrorCode::OutcomeUnknown, error.to_string()),
                            true,
                        ),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
        cx.notify();
    }

    fn fail(&mut self, error: Fault, uncertain: bool) {
        eprintln!("sailry-desktop: runtime operation failed: {error}");
        self.error = Some(if uncertain {
            "runtime_unknown"
        } else {
            super::error_key(error.code)
        });
        self.fault = Some(error);
    }
}
