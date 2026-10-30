use sentry::{Client, Hub};
use std::sync::{Arc, Mutex, Once, Weak};

static INSTALL: Once = Once::new();
static CLIENT: Mutex<Weak<Client>> = Mutex::new(Weak::new());

pub(super) fn install(client: &Arc<Client>) {
    *CLIENT.lock().unwrap_or_else(|error| error.into_inner()) = Arc::downgrade(client);
    // Sentry's hook resolves only the current thread's Hub. Existing workers
    // may have cloned an empty Hub before the process client was installed.
    INSTALL.call_once(|| {
        let next = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let client = CLIENT.lock().ok().and_then(|client| client.upgrade());
            if Hub::current().client().is_none()
                && let Some(client) = client
            {
                Hub::run(
                    Arc::new(Hub::new(Some(client), Arc::new(Default::default()))),
                    || {
                        next(info);
                    },
                );
                return;
            }
            next(info);
        }));
    });
}
