use super::*;
use gpui_kit::component::searchable_list::SearchableListItem;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) enum Method {
    #[default]
    All,
    Oauth,
    Application,
    Secret,
    LocalCli,
}

impl Method {
    pub const ALL: [Self; 5] = [
        Self::All,
        Self::Oauth,
        Self::Application,
        Self::Secret,
        Self::LocalCli,
    ];

    pub fn matches(self, kind: connector::Kind) -> bool {
        match self {
            Self::All => true,
            Self::Oauth => matches!(kind, connector::Kind::Oauth2 | connector::Kind::McpOauth),
            Self::Application => kind == connector::Kind::Application,
            Self::Secret => kind == connector::Kind::Secret,
            Self::LocalCli => kind == connector::Kind::LocalCli,
        }
    }
}

impl SearchableListItem for Method {
    type Value = Self;

    fn title(&self) -> SharedString {
        tr(match self {
            Self::All => "connector_all_methods",
            Self::Oauth => "connector_oauth",
            Self::Application => "connector_application",
            Self::Secret => "connector_token",
            Self::LocalCli => "connector_cli",
        })
    }

    fn value(&self) -> &Self {
        self
    }
}

pub(super) fn key(kind: connector::Kind) -> &'static str {
    match kind {
        connector::Kind::Secret => "connector_token",
        connector::Kind::LocalCli => "connector_cli",
        connector::Kind::Oauth2 | connector::Kind::McpOauth => "connector_oauth",
        connector::Kind::Application => "connector_application",
    }
}

#[cfg(test)]
mod tests {
    use super::{Method, key};
    use sailry_protocol::plugin::connector::Kind;

    #[test]
    fn groups_both_oauth_methods() {
        for kind in [
            Kind::Oauth2,
            Kind::McpOauth,
            Kind::Application,
            Kind::Secret,
            Kind::LocalCli,
        ] {
            assert!(Method::All.matches(kind));
            assert_eq!(
                Method::ALL[1..]
                    .iter()
                    .filter(|method| method.matches(kind))
                    .count(),
                1,
            );
            assert_eq!(
                Method::Oauth.matches(kind),
                matches!(kind, Kind::Oauth2 | Kind::McpOauth)
            );
        }
        assert_eq!(key(Kind::Oauth2), key(Kind::McpOauth));
    }
}
