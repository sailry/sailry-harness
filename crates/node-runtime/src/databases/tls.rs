//! SQLx supplies WebPKI roots; retain the execution Node's native trust roots too.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::sync::OnceLock;

pub(super) fn roots() -> Option<&'static [u8]> {
    static ROOTS: OnceLock<Vec<u8>> = OnceLock::new();
    let roots = ROOTS.get_or_init(|| {
        let mut roots = Vec::new();
        for certificate in rustls_native_certs::load_native_certs().certs {
            roots.extend_from_slice(b"-----BEGIN CERTIFICATE-----\n");
            roots.extend_from_slice(STANDARD.encode(certificate).as_bytes());
            roots.extend_from_slice(b"\n-----END CERTIFICATE-----\n");
        }
        roots
    });
    (!roots.is_empty()).then_some(roots.as_slice())
}
