//! Retry identity for one controller's current short-code pairing intent.
use sailry_link::rendezvous::{Relay, RequestId};

#[derive(Default)]
pub struct Claim {
    current: Option<Attempt>,
}

struct Attempt {
    origin: String,
    code: String,
    request: RequestId,
}

impl Claim {
    /// Retain the same operation after a lost response; another code or service
    /// begins a different pairing intent.
    pub fn request(&mut self, relay: &Relay, code: &str) -> RequestId {
        if self
            .current
            .as_ref()
            .is_none_or(|attempt| attempt.origin != relay.origin() || attempt.code != code)
        {
            self.current = Some(Attempt {
                origin: relay.origin().to_owned(),
                code: code.to_owned(),
                request: RequestId::default(),
            });
        }
        self.current.as_ref().unwrap().request.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(request: RequestId) -> serde_json::Value {
        serde_json::to_value(request).unwrap()
    }

    #[test]
    fn retains_retry_identity() {
        let mut claim = Claim::default();
        let relay = Relay::new("https://link.example").unwrap();
        let first = identity(claim.request(&relay, "123456"));
        assert_eq!(first, identity(claim.request(&relay, "123456")));
        let canonical = Relay::new("https://LINK.example:443/").unwrap();
        assert_eq!(first, identity(claim.request(&canonical, "123456")));
    }

    #[test]
    fn resets_for_other_pairing_intents() {
        let mut claim = Claim::default();
        let relay = Relay::new("https://link.example").unwrap();
        let first = identity(claim.request(&relay, "123456"));
        let changed = identity(claim.request(&relay, "654321"));
        assert_ne!(first, changed);
        let other = Relay::new("https://other.example").unwrap();
        assert_ne!(changed, identity(claim.request(&other, "654321")));
    }
}
