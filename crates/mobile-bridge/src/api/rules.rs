//! Small pure Client decisions; these helpers do not perform Node or network IO.
use flutter_rust_bridge::frb;
use sailry_client::conversation::modes;

#[frb(sync)]
pub fn project_modes(config: String, modes: String) -> Result<String, String> {
    let config = serde_json::from_str(&config).map_err(super::error)?;
    let modes: Vec<modes::Mode> = serde_json::from_str(&modes).map_err(super::error)?;
    serde_json::to_string(&modes::project(&config, &modes)).map_err(super::error)
}

#[frb(sync)]
pub fn select_mode(config: String, modes: String, key: String) -> Result<Option<String>, String> {
    let config = serde_json::from_str(&config).map_err(super::error)?;
    let modes: Vec<modes::Mode> = serde_json::from_str(&modes).map_err(super::error)?;
    modes::select(&config, &modes, &key)
        .map(|config| serde_json::to_string(&config).map_err(super::error))
        .transpose()
}
