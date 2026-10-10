//! Protected credential inputs adapt to the shared search settings service.
use super::*;
use crate::plugins::credentials;
use gpui_kit::Entity;
use sailry_protocol::{Secret, plugin::settings::SecretUpdate, search};

#[derive(serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Edit {
    Keep,
    Clear,
    Set { secret: String },
}

#[derive(serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Basic {
    Keep,
    Clear,
    Set { username: String, password: String },
}

impl Host {
    pub(in crate::plugins) fn search_module(
        self: &Arc<Self>,
        fields: Entity<credentials::Store>,
    ) -> HostModule {
        let owner = self.clone();
        HostModule::new("sailry/search")
            .function("prepareSettings", move |args| {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input {
                    settings: search::Settings,
                    driver: search::Driver,
                    key: Edit,
                    basic: Basic,
                }
                let input: Input = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                let (key, basic) = gpui_shell::with_current_app(|cx| {
                    let fields = fields.read(cx);
                    let key = match input.key {
                        Edit::Keep => SecretUpdate::Keep,
                        Edit::Clear => SecretUpdate::Clear,
                        Edit::Set { secret } => SecretUpdate::Replace(fields.read(&secret, cx)?),
                    };
                    let basic = match input.basic {
                        Basic::Keep => SecretUpdate::Keep,
                        Basic::Clear => SecretUpdate::Clear,
                        Basic::Set { username, password } => {
                            let username = fields.read(&username, cx)?;
                            let password = fields.read(&password, cx)?;
                            if username.expose().is_empty()
                                || username.expose().contains(':')
                                || password.expose().is_empty()
                            {
                                return Err(HostError::new("invalid Basic authentication"));
                            }
                            SecretUpdate::Replace(Secret::new(format!(
                                "{}:{}",
                                username.expose(),
                                password.expose()
                            )))
                        }
                    };
                    Ok::<_, HostError>((key, basic))
                })
                .ok_or_else(|| HostError::new("credential drafts require an active view"))??;
                owner.prepare_public(Command::SaveSearchSettings(search::Update {
                    settings: input.settings,
                    credentials: [(input.driver, search::Credentials { key, basic })].into(),
                }))
            })
            .declarations(include_str!("search.d.ts"))
    }
}
