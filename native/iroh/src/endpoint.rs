use std::{sync::Arc, time::Duration};

use iroh::{
    Endpoint, EndpointAddr, RelayConfig, RelayMap, RelayMode, RelayUrl, SecretKey, Watcher,
    endpoint::{QuicTransportConfig, presets},
    tls::CaTlsConfig,
};
use rustls::pki_types::CertificateDer;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::runtime::{Output, Owner, Result, copy_input, error, spawn, state};

#[derive(Deserialize)]
struct Config {
    secret_key: [u8; 32],
    mode: String,
    #[serde(default)]
    bind: String,
    #[serde(default)]
    relays: Vec<String>,
    #[serde(default)]
    roots_der: Vec<Vec<u8>>,
    #[serde(default)]
    relay_only: bool,
}

fn endpoint_owner(endpoint: u64) -> Owner {
    Owner {
        endpoint,
        ..Owner::default()
    }
}

pub fn relay_url(value: &str) -> Result<RelayUrl> {
    let url: RelayUrl = value.parse().map_err(error)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("relay URLs require HTTPS without credentials".into());
    }
    Ok(url)
}

async fn create(config: Vec<u8>) -> Result<Output> {
    #[cfg(target_os = "android")]
    crate::android::ready()?;
    let config: Config = serde_json::from_slice(&config).map_err(error)?;
    let builder = match config.mode.as_str() {
        "direct" => Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Disabled)
            .clear_address_lookup()
            .clear_ip_transports()
            .bind_addr(if config.bind.is_empty() {
                "127.0.0.1:0"
            } else {
                config.bind.as_str()
            })
            .map_err(error)?,
        "internet" => Endpoint::builder(presets::N0).relay_mode(RelayMode::Default),
        "custom" => {
            if config.relays.is_empty() || config.relays.len() > 8 {
                return Err("expected one to eight relay URLs".into());
            }
            let urls = config
                .relays
                .iter()
                .map(|value| relay_url(value))
                .collect::<Result<Vec<_>>>()?;
            Endpoint::builder(presets::Minimal)
                .relay_mode(RelayMode::Custom(RelayMap::from_iter(urls)))
        }
        _ => return Err("unknown iroh network mode".into()),
    };
    let builder = if config.roots_der.is_empty() {
        builder
    } else {
        builder.ca_tls_config(CaTlsConfig::custom_roots(
            config.roots_der.into_iter().map(CertificateDer::from),
        ))
    };
    let builder = if config.relay_only {
        builder.clear_ip_transports()
    } else {
        builder
    };
    let endpoint = builder
        .secret_key(SecretKey::from_bytes(&config.secret_key))
        .alpns(vec![b"sailry/node/1".to_vec(), b"sailry/pair/1".to_vec()])
        .transport_config(
            QuicTransportConfig::builder()
                .max_concurrent_bidi_streams(64u32.into())
                .max_concurrent_uni_streams(0u32.into())
                .stream_receive_window((512u32 * 1024).into())
                .receive_window((4u32 * 1024 * 1024).into())
                .send_window((4u32 * 1024 * 1024).into())
                .build(),
        )
        .bind()
        .await
        .map_err(error)?;
    Ok(Output::Endpoint(endpoint))
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_default_relay_urls() -> usize {
    spawn(Owner::default(), async move {
        let urls = RelayMode::Default
            .relay_map()
            .urls::<Vec<_>>()
            .into_iter()
            .map(|url| url.to_string())
            .collect::<Vec<_>>();
        Ok(Output::Json(serde_json::to_value(urls).map_err(error)?))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sailry_iroh_endpoint_new(config: *const u8, length: usize) -> usize {
    let config = unsafe { copy_input(config, length) };
    spawn(Owner::default(), create(config))
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_endpoint_addr(endpoint: u64) -> usize {
    spawn(endpoint_owner(endpoint), async move {
        let address = state().endpoint(endpoint)?.addr();
        Ok(Output::Json(serde_json::to_value(address).map_err(error)?))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_endpoint_close(endpoint: u64) -> usize {
    spawn(Owner::default(), async move {
        state().close_endpoint(endpoint).await?;
        Ok(Output::Empty)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_endpoint_online(endpoint: u64) -> usize {
    spawn(endpoint_owner(endpoint), async move {
        state().endpoint(endpoint)?.online().await;
        Ok(Output::Empty)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_endpoint_network_change(endpoint: u64) -> usize {
    spawn(endpoint_owner(endpoint), async move {
        state().endpoint(endpoint)?.network_change().await;
        Ok(Output::Empty)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sailry_iroh_endpoint_insert_relay(
    endpoint: u64,
    url: *const u8,
    length: usize,
) -> usize {
    let url = unsafe { copy_input(url, length) };
    spawn(endpoint_owner(endpoint), async move {
        let url = relay_url(std::str::from_utf8(&url).map_err(error)?)?;
        state()
            .endpoint(endpoint)?
            .insert_relay(url.clone(), Arc::new(RelayConfig::from(url)))
            .await;
        Ok(Output::Empty)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sailry_iroh_endpoint_remove_relay(
    endpoint: u64,
    url: *const u8,
    length: usize,
) -> usize {
    let url = unsafe { copy_input(url, length) };
    spawn(endpoint_owner(endpoint), async move {
        let url = relay_url(std::str::from_utf8(&url).map_err(error)?)?;
        state().endpoint(endpoint)?.remove_relay(&url).await;
        Ok(Output::Empty)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_endpoint_relay_status(endpoint: u64) -> usize {
    spawn(endpoint_owner(endpoint), async move {
        let endpoint = state().endpoint(endpoint)?;
        Ok(Output::Json(relay_status(
            endpoint.home_relay_status().get(),
        )))
    })
}

fn relay_status(statuses: Vec<iroh::endpoint::RelayStatus>) -> Value {
    Value::Array(
        statuses
            .into_iter()
            .map(|status| {
                json!({
                    "url": status.url().to_string(),
                    "connected": status.is_connected(),
                    "error": status.last_error().map(ToString::to_string).unwrap_or_default(),
                    "auth_denied_reason": status.auth_denied_reason().unwrap_or_default(),
                })
            })
            .collect(),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sailry_iroh_endpoint_wait_relay_status(
    endpoint: u64,
    previous: *const u8,
    length: usize,
) -> usize {
    let previous = unsafe { copy_input(previous, length) };
    spawn(endpoint_owner(endpoint), async move {
        let previous: Value = serde_json::from_slice(&previous).map_err(error)?;
        let endpoint = state().endpoint(endpoint)?;
        let mut statuses = endpoint.home_relay_status();
        loop {
            let current = relay_status(statuses.get());
            if current != previous {
                return Ok(Output::Json(current));
            }
            statuses.updated().await.map_err(error)?;
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sailry_iroh_endpoint_wait_address(
    endpoint: u64,
    previous: *const u8,
    length: usize,
) -> usize {
    let previous = unsafe { copy_input(previous, length) };
    spawn(endpoint_owner(endpoint), async move {
        let previous: EndpointAddr = serde_json::from_slice(&previous).map_err(error)?;
        let endpoint = state().endpoint(endpoint)?;
        let mut address = endpoint.watch_addr();
        loop {
            let current = address.get();
            if current != previous {
                return Ok(Output::Json(serde_json::to_value(current).map_err(error)?));
            }
            address.updated().await.map_err(error)?;
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sailry_iroh_connect(
    endpoint: u64,
    address: *const u8,
    address_length: usize,
    alpn: *const u8,
    alpn_length: usize,
) -> usize {
    let address = unsafe { copy_input(address, address_length) };
    let alpn = unsafe { copy_input(alpn, alpn_length) };
    spawn(endpoint_owner(endpoint), async move {
        let address: EndpointAddr = serde_json::from_slice(&address).map_err(error)?;
        let connection = state()
            .endpoint(endpoint)?
            .connect(address, &alpn)
            .await
            .map_err(error)?;
        Ok(Output::Connection(endpoint, connection))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_accept(endpoint: u64) -> usize {
    spawn(endpoint_owner(endpoint), async move {
        let endpoint_handle = state().endpoint(endpoint)?;
        let incoming = endpoint_handle
            .accept()
            .await
            .ok_or_else(|| "iroh endpoint is closed".to_string())?;
        let connection = tokio::time::timeout(Duration::from_secs(10), incoming)
            .await
            .map_err(error)?
            .map_err(error)?;
        Ok(Output::Connection(endpoint, connection))
    })
}
