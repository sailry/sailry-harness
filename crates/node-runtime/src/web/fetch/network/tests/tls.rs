use super::proxy;
use sailry_link::CancellationToken;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::{io::AsyncWriteExt, net::TcpListener};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer},
};

pub const HOST: &str = "secure.sailry.test";

pub struct Request {
    pub server_name: Option<String>,
    pub headers: String,
}

pub struct Server {
    pub address: SocketAddr,
    pub url: String,
    pub root: reqwest::Certificate,
    pub requests: Arc<Mutex<Vec<Request>>>,
    stop: CancellationToken,
}

impl Server {
    pub async fn start() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec![HOST.to_owned()]).unwrap();
        let root = reqwest::Certificate::from_der(cert.der()).unwrap();
        let key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert.der().clone()], key.into())
            .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = CancellationToken::new();
        let server = Self {
            address,
            url: format!("https://{HOST}:{}/secure", address.port()),
            root,
            requests: requests.clone(),
            stop: stop.clone(),
        };
        tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                let (stream, _) = tokio::select! {
                    _ = stop.cancelled() => break,
                    result = listener.accept() => result.unwrap(),
                };
                let acceptor = acceptor.clone();
                let requests = requests.clone();
                connections.spawn(async move {
                    let mut stream = acceptor.accept(stream).await.unwrap();
                    let name = stream.get_ref().1.server_name().map(str::to_owned);
                    let headers = proxy::header(&mut stream).await;
                    requests.lock().unwrap().push(Request {server_name: name, headers});
                    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 13\r\nConnection: close\r\n\r\nSecure origin").await.unwrap();
                    stream.shutdown().await.unwrap();
                });
            }
            connections.abort_all();
            while connections.join_next().await.is_some() {}
        });
        server
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
