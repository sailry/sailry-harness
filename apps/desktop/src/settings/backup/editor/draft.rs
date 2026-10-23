use super::*;

pub(super) struct Draft {
    pub tab: usize,
    pub destination: Destination,
    pub directory: String,
    pub identity: String,
    pub secret: String,
    pub session_token: String,
    pub passphrase: String,
    pub confirmation: String,
    pub schedule: Schedule,
    pub remove_credentials: bool,
}

fn schedule(settings: &Settings) -> Schedule {
    settings.schedule.clone().unwrap_or(Schedule {
        enabled: false,
        timezone: "UTC".into(),
        hour: 3,
        minute: 0,
    })
}

impl Draft {
    pub(super) fn update(&self, settings: &Settings, mode: Mode) -> Result<Update, &'static str> {
        let mut saved = settings.clone();
        if mode == Mode::Schedule {
            if self.schedule.enabled && self.schedule.timezone.is_empty() {
                return Err("backup_invalid");
            }
            saved.schedule = (settings.schedule.is_some()
                || self.schedule != schedule(&Settings::default()))
            .then(|| self.schedule.clone());
            return Ok(Update {
                settings: saved,
                identity: SecretUpdate::Keep,
                secret: SecretUpdate::Keep,
                session_token: SecretUpdate::Keep,
                passphrase: SecretUpdate::Keep,
            });
        }
        if !self.passphrase.is_empty() || !self.confirmation.is_empty() {
            sailry_protocol::backup::validate_passphrase(&self.passphrase)
                .map_err(|_| "backup_passphrase_invalid")?;
            if self.passphrase != self.confirmation {
                return Err("backup_passphrase_mismatch");
            }
        }
        storage(
            &mut saved,
            self.tab,
            self.destination.clone(),
            &self.directory,
        )?;
        let credential = |value: &str| {
            if !value.is_empty() {
                SecretUpdate::Replace(Secret::new(value.into()))
            } else if self.remove_credentials {
                SecretUpdate::Clear
            } else {
                SecretUpdate::Keep
            }
        };
        Ok(Update {
            settings: saved,
            identity: credential(&self.identity),
            secret: credential(&self.secret),
            session_token: if self.tab == 0 {
                SecretUpdate::Clear
            } else {
                credential(&self.session_token)
            },
            passphrase: if self.passphrase.is_empty() {
                SecretUpdate::Keep
            } else {
                SecretUpdate::Replace(Secret::new(self.passphrase.clone()))
            },
        })
    }

    pub(super) fn dirty(&self, settings: &Settings, mode: Mode) -> bool {
        if mode == Mode::Schedule {
            return self.schedule != schedule(settings);
        }
        let saved = destination(settings);
        self.remove_credentials
            || [
                &self.identity,
                &self.secret,
                &self.session_token,
                &self.passphrase,
                &self.confirmation,
            ]
            .into_iter()
            .any(|value| !value.is_empty())
            || self.tab != storage_tab(settings)
            || self.destination.endpoint != saved.endpoint
            || self.destination.root != saved.root
            || self.destination.bucket != saved.bucket
            || self.destination.region != saved.region
            || (self.tab == 2) != settings.local.as_ref().is_some_and(|local| local.enabled)
            || self.directory
                != settings
                    .local
                    .as_ref()
                    .map(|local| local.directory.as_str())
                    .unwrap_or_default()
            || (self.tab < 2) != settings.online_enabled
    }
}
