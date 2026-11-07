//! Reply links to sessions: `sailry://session/<id>`.
//!
//! The `session` host keeps these apart from Desktop's registered
//! `sailry://return` browser link. Controllers open them in place and never
//! hand them to the operating system.
use super::children;
use sailry_protocol::SessionId;
use serde::Serialize;

const SCHEME: &str = "sailry";
const HOST: &str = "session";

/// A linked session, classified for the conversation showing the link.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reference {
    /// A child this conversation delegated.
    Child(SessionId),
    /// Any other session; its Node decides whether it can be opened.
    Session(SessionId),
}

impl Reference {
    pub fn new(session: SessionId, children: &children::View) -> Self {
        if children.sessions.contains_key(&session) {
            Self::Child(session)
        } else {
            Self::Session(session)
        }
    }

    pub fn session(self) -> SessionId {
        match self {
            Self::Child(session) | Self::Session(session) => session,
        }
    }
}

/// The link a reply uses for `session`.
pub fn link(session: SessionId) -> String {
    format!("{SCHEME}://{HOST}/{session}")
}

/// The session a link names. Scheme and host ignore case as in any URL; the ID
/// is one hyphenated UUID in either case, with nothing after it.
pub fn session(link: &str) -> Option<SessionId> {
    let url = url::Url::parse(link.trim()).ok()?;
    if url.scheme() != SCHEME
        || !url.host_str()?.eq_ignore_ascii_case(HOST)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    // Braced, URN and bare-hex spellings parse as UUIDs but are not reply links.
    let id = url.path().strip_prefix('/').filter(|id| id.len() == 36)?;
    id.parse().ok()
}

/// Resolve a reply link against the conversation showing it.
pub fn resolve(link: &str, children: &children::View) -> Option<Reference> {
    session(link).map(|session| Reference::new(session, children))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const ID: &str = "0b7c3e1a-5d2f-4c6b-9a8e-1f2d3c4b5a69";

    fn id() -> SessionId {
        ID.parse().unwrap()
    }

    fn delegated(sessions: &[SessionId]) -> children::View {
        children::View {
            sessions: sessions
                .iter()
                .map(|session| {
                    (
                        *session,
                        children::Reference {
                            index: 0,
                            target: String::new(),
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        }
    }

    #[test]
    fn round_trips_its_own_links() {
        let link = link(id());
        assert_eq!(link, format!("sailry://session/{ID}"));
        assert_eq!(session(&link), Some(id()));
        assert_eq!(session(&format!("  {link}\n")), Some(id()));
    }

    #[test]
    fn ignores_case() {
        for link in [
            format!("sailry://session/{}", ID.to_uppercase()),
            format!("SAILRY://Session/{ID}"),
        ] {
            assert_eq!(session(&link), Some(id()), "{link}");
        }
    }

    #[test]
    fn rejects_malformed_links() {
        for link in [
            String::new(),
            ID.to_owned(),
            "sailry://session/".to_owned(),
            "sailry://session/not-a-session".to_owned(),
            "sailry://session/00000000-0000-0000-0000-000000000000".to_owned(),
            format!("sailry://session/{}", ID.replace('-', "")),
            format!("sailry://session/{{{ID}}}"),
            format!("sailry://session/urn:uuid:{ID}"),
            format!("sailry://session/{ID}/"),
            format!("sailry://session/{ID}/turn"),
            format!("sailry://session/{ID}?turn=1"),
            format!("sailry://session/{ID}#latest"),
            format!("sailry://user@session/{ID}"),
            format!("sailry://session:80/{ID}"),
            format!("sailry:session/{ID}"),
            format!("sailry://return/{ID}"),
            "sailry://return".to_owned(),
            format!("sailry-session://session/{ID}"),
            format!("https://session/{ID}"),
            format!("[child](sailry://session/{ID})"),
        ] {
            assert_eq!(session(&link), None, "{link}");
        }
    }

    #[test]
    fn classifies_children_of_the_conversation() {
        let other = SessionId::new();
        let children = delegated(&[id()]);
        assert_eq!(
            resolve(&link(id()), &children),
            Some(Reference::Child(id()))
        );
        assert_eq!(
            resolve(&link(other), &children),
            Some(Reference::Session(other))
        );
        assert_eq!(
            resolve(&link(id()), &children::View::default()),
            Some(Reference::Session(id()))
        );
        assert_eq!(resolve("https://example.com", &children), None);
        assert_eq!(Reference::Child(id()).session(), id());
    }
}
