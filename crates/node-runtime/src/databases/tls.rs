//! Trust roots and TLS policy follow the committed sailry-connections adapter.
use rustls::{ClientConfig, RootCertStore, pki_types::CertificateDer};
use std::sync::OnceLock;

fn roots() -> &'static [CertificateDer<'static>] {
    static ROOTS: OnceLock<Vec<CertificateDer<'static>>> = OnceLock::new();
    ROOTS.get_or_init(|| rustls_native_certs::load_native_certs().certs)
}
pub(super) fn postgres() -> ClientConfig {
    let mut store = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    store.add_parsable_certificates(roots().iter().cloned());
    ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("TLS versions")
    .with_root_certificates(store)
    .with_no_client_auth()
}
pub(super) fn mysql() -> mysql_async::SslOpts {
    let _ = rustls::crypto::ring::default_provider().install_default();
    mysql_async::SslOpts::default().with_root_certs(
        roots()
            .iter()
            .map(|cert| cert.as_ref().to_vec().into())
            .collect(),
    )
}
