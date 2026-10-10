use super::*;
use sailry_protocol::{ErrorCode, Fault, Output};

impl Panel {
    pub(in crate::settings) fn update_model(
        &mut self,
        key: [u8; 32],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.locked() {
            return;
        }
        let binding = self.binding.clone();
        let generation = self.generation;
        self.pending = true;
        let job = binding.runtime.spawn(async move {
            binding
                .client
                .execute(binding.client.prepare(Command::RefreshOcrModel {
                    id: super::identifier(key),
                }))
                .await
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.pending = false;
                if !panel.active || generation != panel.generation {
                    return;
                }
                match result {
                    Ok(Ok(Output::ModelDefinition(definition))) => {
                        if matches!(
                            panel.model_status(&definition),
                            super::super::model_library::ModelStatus::Installed
                        ) {
                            crate::feedback::info(
                                "",
                                &crate::tr("knowledge_up_to_date"),
                                window,
                                cx,
                            );
                        } else {
                            panel.submit(Command::RegisterOcrModel { definition }, window, cx);
                        }
                    }
                    Ok(Err(error)) => panel.error = Some(super::error_key(error.code)),
                    _ => panel.error = Some("runtime_failed"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

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
            let settings = binding
                .client
                .execute(binding.client.prepare(Command::ReadOcrSettings))
                .await?;
            let models = binding
                .client
                .execute(binding.client.prepare(Command::ReadOcrModels))
                .await?;
            Ok::<_, Fault>((python, settings, models))
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
                    Ok(Ok((
                        Output::PythonSettings(python),
                        Output::OcrSettings(settings),
                        Output::OcrModels(models),
                    ))) => {
                        panel.accept_index(&python, window, cx);
                        panel.python = Some(python);
                        panel.settings = Some(settings);
                        panel.models = models;
                        panel.choices(window, cx);
                        panel.error = None;
                    }
                    Ok(Err(error)) => panel.error = Some(super::error_key(error.code)),
                    _ => panel.error = Some("runtime_failed"),
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
        let registering = matches!(request.command, Command::RegisterOcrModel { .. });
        let binding = self.binding.clone();
        self.pending = true;
        self.error = None;
        let job = self.binding.runtime.spawn(async move {
            super::super::knowledge::requests::execute(&binding, request).await
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                panel.pending = false;
                match result {
                    Ok((Ok(output), _)) => {
                        panel.request = None;
                        match output {
                            Output::PythonSettings(settings) => panel.python = Some(settings),
                            Output::OcrSettings(settings) => panel.settings = Some(settings),
                            Output::OcrModel { id } if registering => {
                                panel.submit(Command::InstallOcrModel { id }, window, cx);
                                return;
                            }
                            _ => (),
                        }
                        crate::feedback::info("", &crate::tr("knowledge_saved"), window, cx);
                        panel.load(window, cx);
                    }
                    Ok((Err(error), confirmed)) => {
                        if confirmed && error.code != ErrorCode::OutcomeUnknown {
                            panel.request = None;
                        }
                        if error.code != ErrorCode::Cancelled {
                            panel.error = Some(if panel.request.is_some() {
                                "runtime_unknown"
                            } else {
                                super::error_key(error.code)
                            });
                        }
                    }
                    Err(_) => panel.error = Some("runtime_unknown"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn cancel_transfer(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active {
            return;
        }
        let binding = self.binding.clone();
        let job = self.binding.runtime.spawn(async move {
            binding
                .client
                .execute(binding.client.prepare(Command::CancelOcrModel { id }))
                .await
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |panel, window, cx| {
                match result {
                    Ok(Ok(_)) => crate::feedback::info(
                        "",
                        &crate::tr("knowledge_cancel_requested"),
                        window,
                        cx,
                    ),
                    _ => panel.error = Some("runtime_failed"),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
