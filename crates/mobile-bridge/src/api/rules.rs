//! Small pure Client decisions; these helpers do not perform Node or network IO.
use flutter_rust_bridge::frb;
use sailry_client::conversation::{links, modes};

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

/// The session ID a reply link names, in the form Client projections use.
#[frb(sync)]
pub fn session_link(link: String) -> Option<String> {
    links::session(&link).map(|session| session.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_links_use_projection_ids() {
        let session = sailry_protocol::SessionId::new();
        let link = links::link(session);
        let projected = serde_json::to_value(session).unwrap();
        assert_eq!(
            session_link(link.to_uppercase()).as_deref(),
            projected.as_str()
        );
        assert_eq!(session_link(format!("{link}/turn")), None);
        assert_eq!(session_link("https://example.com".into()), None);
    }
}
