use std::{
    io::{self, BufRead, Write},
    net::{Ipv4Addr, SocketAddr},
};

use iroh_relay::server::{CertConfig, Server, ServerConfig, TlsConfig, testing};
use serde::Deserialize;
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize)]
struct Command {
    op: String,
}

async fn start(
    http: SocketAddr,
    https: SocketAddr,
    certificates: rustls::ServerConfig,
) -> Result<Server> {
    let mut relay = iroh_relay::server::RelayConfig::new(http);
    relay.tls = Some(TlsConfig::new(
        https,
        CertConfig::Manual {
            server_config: certificates,
        },
    ));
    let mut config = ServerConfig::default();
    config.relay = Some(relay);
    Ok(Server::spawn(config).await?)
}

fn publish(value: Value) -> Result<()> {
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, &value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let (roots, certificates) = testing::self_signed_tls_certs_and_config();
    let initial = (Ipv4Addr::LOCALHOST, 0).into();
    let server = start(initial, initial, certificates.clone()).await?;
    let http = server.http_addr().ok_or("relay has no HTTP address")?;
    let https = server.https_addr().ok_or("relay has no HTTPS address")?;
    let roots = roots
        .into_iter()
        .map(|certificate| certificate.as_ref().to_vec())
        .collect::<Vec<_>>();
    let mut server = Some(server);
    publish(json!({
        "url": format!("https://{https}"),
        "roots_der": roots,
        "running": true,
    }))?;
    for line in io::stdin().lock().lines() {
        let command: Command = serde_json::from_str(&line?)?;
        match command.op.as_str() {
            "stop" => {
                if let Some(server) = server.take() {
                    server.shutdown().await?;
                }
                publish(json!({ "running": false }))?;
            }
            "start" => {
                if server.is_none() {
                    server = Some(start(http, https, certificates.clone()).await?);
                }
                publish(json!({
                    "url": format!("https://{https}"),
                    "roots_der": roots,
                    "running": true,
                }))?;
            }
            "close" => break,
            _ => publish(json!({ "error": "unknown relay fixture operation" }))?,
        }
    }
    if let Some(server) = server.take() {
        server.shutdown().await?;
    }
    publish(json!({ "closed": true }))?;
    Ok(())
}
