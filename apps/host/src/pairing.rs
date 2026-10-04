//! Opt-in service management output, not an interactive Agent or ticket log.
use sailry_link::{
    CancellationToken, LinkHandle,
    rendezvous::{Relay, ShareState},
};
use tokio::sync::watch;

pub(crate) async fn share(relay: Relay, link: LinkHandle, stop: CancellationToken) {
    let (sender, mut receiver) = watch::channel(ShareState::Preparing);
    let sharing = relay.share(&link, sender, stop);
    tokio::pin!(sharing);
    loop {
        tokio::select! {
            biased;
            changed = receiver.changed() => {
                if changed.is_err() {
                    return;
                }
                match &*receiver.borrow_and_update() {
                    ShareState::Ready { code, expires_at_ms } => {
                        println!("Pairing PIN: {code} (expires at Unix ms {expires_at_ms}); keep private");
                    }
                    ShareState::Retrying(_) => eprintln!("Pairing service unavailable; retrying"),
                    ShareState::Paired => println!("Pairing complete; sharing stopped"),
                    ShareState::Closed => println!("Pairing closed"),
                    ShareState::Preparing => {},
                }
            }
            result = &mut sharing => {
                if result.is_err() {
                    eprintln!("Pairing stopped: service unavailable or request rejected");
                }
                return;
            }
        }
    }
}
