//! A settings view can retain native drafts only across its own confirmed save.
//! The receipt changes provenance; scripts cannot adopt arbitrary package revisions.
use super::*;
use crate::plugins::credentials;
use gpui_kit::Entity;
use sailry_protocol::{Output, Secret, plugin};
#[cfg(test)]
mod tests;

struct Pending {
    id: RequestId,
    source: plugin::Reference,
}

pub(super) struct Settings {
    current: plugin::Reference,
    surface: bool,
    pending: Option<Pending>,
    live: Option<plugin::Summary>,
    connected: bool,
    invalid: bool,
}

impl Settings {
    pub fn reference(&self) -> plugin::Reference {
        self.current.clone()
    }

    pub fn new(context: &Context) -> Self {
        Self {
            current: context.package.clone(),
            surface: context.surface == plugin::desktop::Surface::Settings,
            pending: None,
            live: None,
            connected: false,
            invalid: false,
        }
    }

    fn observe(&mut self, summaries: &[plugin::Summary], connected: bool) {
        if !self.surface || self.invalid {
            return;
        }
        self.connected = connected;
        let live = summaries
            .iter()
            .find(|summary| summary.name == self.current.name);
        if !connected || live.is_none_or(|summary| summary.digest != self.current.digest) {
            self.invalid = true;
            return;
        }
        let live = live.unwrap();
        if live.settings_revision < self.current.settings_revision
            || self
                .live
                .as_ref()
                .is_some_and(|previous| previous.revision > live.revision)
        {
            return;
        }
        let changed_enable = self
            .live
            .as_ref()
            .is_some_and(|previous| previous.enabled != live.enabled);
        if let Some(pending) = &self.pending {
            if changed_enable
                || live.settings_revision > pending.source.settings_revision.saturating_add(1)
            {
                self.invalid = true;
            }
        } else if live.settings_revision > self.current.settings_revision {
            self.invalid = true;
        }
        self.live = Some(live.clone());
    }

    fn begin(&mut self, id: RequestId, source: &plugin::Reference) -> Result<(), HostError> {
        if !self.surface
            || self.invalid
            || !self.connected
            || self.pending.is_some()
            || source != &self.current
        {
            return Err(HostError::new(
                "settings save is unavailable or already pending",
            ));
        }
        self.pending = Some(Pending {
            id,
            source: source.clone(),
        });
        Ok(())
    }

    fn finish(&mut self, id: RequestId, saved: &plugin::Reference, live: &plugin::Summary) -> bool {
        let Some(pending) = &self.pending else {
            return false;
        };
        if pending.id != id
            || self.invalid
            || !self.connected
            || pending.source.name != saved.name
            || pending.source.digest != saved.digest
            || pending.source.settings_revision.checked_add(1) != Some(saved.settings_revision)
            || live.reference() != *saved
            || self
                .live
                .as_ref()
                .is_some_and(|previous| previous.enabled != live.enabled)
        {
            self.invalid = true;
            return false;
        }
        self.current = saved.clone();
        self.live = Some(live.clone());
        self.pending = None;
        true
    }

    fn failed(&mut self, id: RequestId) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.id == id)
        {
            self.pending = None;
            if self
                .live
                .as_ref()
                .is_some_and(|live| live.reference() != self.current)
            {
                self.invalid = true;
            }
        }
    }
}

impl Host {
    pub(in crate::plugins) fn observe_settings(
        &self,
        summaries: &[plugin::Summary],
        connected: bool,
    ) {
        let invalid = if let Ok(mut settings) = self.settings.lock() {
            settings.observe(summaries, connected);
            settings.invalid
        } else {
            true
        };
        if invalid {
            self.close();
        }
    }

    pub(in crate::plugins) fn retain_settings(&self) -> bool {
        self.settings.lock().is_ok_and(|settings| {
            settings.surface
                && !settings.invalid
                && settings.connected
                && (settings.pending.is_some() || settings.current != self.context.package)
        })
    }

    pub(super) fn prepare_settings(
        &self,
        id: RequestId,
        command: &Command,
    ) -> Result<(), HostError> {
        let mut settings = self.settings.lock().map_err(lock_error)?;
        if let Command::SavePluginSettings { package, .. } = command {
            settings.begin(id, package)
        } else if settings.pending.is_some() {
            Err(HostError::new("settings save is pending"))
        } else {
            Ok(())
        }
    }

    pub(super) fn settings_readable(&self) -> Result<(), HostError> {
        if self.settings.lock().map_err(lock_error)?.pending.is_some() {
            Err(HostError::new("settings save is pending"))
        } else {
            Ok(())
        }
    }

    pub(super) fn settings_forgettable(&self, id: RequestId) -> Result<(), HostError> {
        if self
            .settings
            .lock()
            .map_err(lock_error)?
            .pending
            .as_ref()
            .is_some_and(|pending| pending.id == id)
        {
            Err(HostError::new(
                "unconfirmed settings save cannot be forgotten",
            ))
        } else {
            Ok(())
        }
    }

    pub(super) async fn finish_settings(
        &self,
        id: RequestId,
        result: &Result<Output, sailry_protocol::Fault>,
    ) -> Result<(), HostError> {
        if !self
            .settings
            .lock()
            .map_err(lock_error)?
            .pending
            .as_ref()
            .is_some_and(|pending| pending.id == id)
        {
            return Ok(());
        }
        match result {
            Ok(Output::PluginSettings(saved)) => {
                let Output::Plugin(live) = self
                    .client
                    .execute(self.client.prepare(Command::ReadPlugin {
                        name: saved.package.name.clone(),
                    }))
                    .await
                    .map_err(|_| HostError::new("unconfirmed"))?
                else {
                    return Err(HostError::new("unconfirmed"));
                };
                if !self.settings.lock().map_err(lock_error)?.finish(
                    id,
                    &saved.package,
                    &live.summary,
                ) {
                    self.close();
                    return Err(HostError::new(
                        "plugin configuration changed after the settings save",
                    ));
                }
            }
            Err(fault)
                if !matches!(
                    fault.code,
                    sailry_protocol::ErrorCode::Unavailable
                        | sailry_protocol::ErrorCode::OutcomeUnknown
                ) =>
            {
                let invalid = {
                    let mut settings = self.settings.lock().map_err(lock_error)?;
                    settings.failed(id);
                    settings.invalid
                };
                if invalid {
                    self.close();
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(in crate::plugins) fn settings_module(
        self: &Arc<Self>,
        fields: Entity<credentials::Store>,
    ) -> HostModule {
        let load = self.clone();
        let save = self.clone();
        let loaded = fields.clone();
        HostModule::new("sailry/settings")
            .async_function("loadSettingSecret", move |args| {
                load.check()?;
                load.settings_readable()?;
                if load.context.surface != plugin::desktop::Surface::Settings {
                    return Err(HostError::new("credential editing requires the settings surface"));
                }
                let id = args.string(0)?.to_owned();
                let field = args.string(1)?.to_owned();
                let revision = sdk::values::revision(args.string(2)?)?;
                let context = load.context();
                if context.package.settings_revision != revision {
                    return Err(HostError::new("plugin configuration changed"));
                }
                let permit = load.capacity.clone().try_acquire_owned()
                    .map_err(|_| HostError::new("plugin request capacity exhausted"))?;
                let owner = load.clone();
                let job = load.runtime.spawn(async move {
                    let _permit = permit;
                    tokio::select! {
                        biased;
                        _ = owner.stop.cancelled() => Err(HostError::new("plugin view is closed")),
                        result = owner.load_setting_secret(context, field) => result,
                    }
                });
                let pending = credentials::loading::load(loaded.clone(), id, job)?;
                Ok(async move { sdk::values::encode(serde_json::json!(pending.await?)) })
            })
            .function("prepareSettings", move |args| {
                #[derive(serde::Deserialize)]
                #[serde(tag="action",rename_all="snake_case",deny_unknown_fields)]
                enum Edit { Keep, Clear, Set { secret: String } }
                let values = serde_json::from_value(sdk::values::decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                let edits: BTreeMap<String,Edit> = serde_json::from_value(sdk::values::decode(args.value(1)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                save.check()?;
                let revision = sdk::values::revision(args.string(2)?)?;
                let context = save.context();
                if context.surface != plugin::desktop::Surface::Settings || context.package.settings_revision != revision {
                    return Err(HostError::new("plugin configuration changed or settings surface required"));
                }
                let secrets = edits.into_iter().map(|(name, edit)| {
                    let update = match edit {
                        Edit::Keep => plugin::settings::SecretUpdate::Keep,
                        Edit::Clear => plugin::settings::SecretUpdate::Clear,
                        Edit::Set { secret } => plugin::settings::SecretUpdate::Replace(
                            gpui_shell::with_current_app(|cx|fields.read(cx).read(&secret,cx))
                                .ok_or_else(|| HostError::new("credential editing requires an active view"))??
                        ),
                    };
                    Ok::<_,HostError>((name,update))
                }).collect::<Result<_,_>>()?;
                save.prepare_public(Command::SavePluginSettings { package: context.package, values, secrets })
            })
            .declarations(r#"
export function loadSettingSecret(secret:string,field:string,expectedRevision:string):Promise<boolean>;
export function prepareSettings(values:Record<string,import('sailry/sdk').Json>,edits:Record<string,{action:'keep'|'clear'}|{action:'set';secret:string}>,expectedRevision:string):string;
"#)
    }

    async fn load_setting_secret(
        &self,
        context: Context,
        field: String,
    ) -> Result<Option<Secret>, HostError> {
        let check = || {
            self.client
                .prepare(Command::ReadPluginSettings {
                    package: context.package.clone(),
                })
                .with_plugin(context.clone())
        };
        self.client
            .execute(check())
            .await
            .map_err(sdk::values::fault)?;
        let output = self
            .client
            .execute(self.client.prepare(Command::ReadPluginSecret {
                package: context.package.clone(),
                field,
            }))
            .await
            .map_err(sdk::values::fault)?;
        self.client
            .execute(check())
            .await
            .map_err(sdk::values::fault)?;
        let Output::PluginSecret(secret) = output else {
            return Err(HostError::new("credential response expected"));
        };
        Ok(secret)
    }
}
