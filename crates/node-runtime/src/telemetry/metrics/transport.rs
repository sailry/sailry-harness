//! Best-effort async delivery. No upstream body or error text is observed.
use super::aggregate::Batch;
use reqwest::{
    Client, StatusCode,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use std::time::Duration;
use tokio::time::Instant;

const ENDPOINT: &str = "https://api.telemetry.sh/log";
const ATTEMPTS: u32 = 4;

pub(super) struct Transport {
    client: Client,
    endpoint: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Delivery {
    Accepted,
    Retry,
    Discard,
}

impl Transport {
    pub(super) fn new(key: &str) -> Option<Self> {
        Self::at(key, ENDPOINT)
    }

    fn at(key: &str, endpoint: &str) -> Option<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut authorization = HeaderValue::from_str(&format!("Bearer {key}")).ok()?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, authorization);
        let client = Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .build()
            .ok()?;
        Some(Self {
            client,
            endpoint: endpoint.into(),
        })
    }

    pub(super) async fn send(&self, batch: &Batch, deadline: Option<Instant>) -> bool {
        // Serialize once: retries reuse the exact same rows and event identities.
        let Ok(body) = serde_json::to_vec(batch) else {
            return false;
        };
        for attempt in 0..ATTEMPTS {
            let request = self
                .client
                .post(&self.endpoint)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.clone());
            let delivery = if let Some(deadline) = deadline {
                match tokio::time::timeout_at(deadline, request.send()).await {
                    Ok(result) => classify(result),
                    Err(_) => return false,
                }
            } else {
                classify(request.send().await)
            };
            if delivery != Delivery::Retry || attempt + 1 == ATTEMPTS {
                return delivery == Delivery::Accepted;
            }
            let next = Instant::now() + backoff(attempt);
            if deadline.is_some_and(|deadline| next >= deadline) {
                return false;
            }
            tokio::time::sleep_until(next).await;
        }
        false
    }
}

fn classify(result: Result<reqwest::Response, reqwest::Error>) -> Delivery {
    match result {
        Ok(response) if response.status().is_success() => Delivery::Accepted,
        Ok(response) if retryable(response.status()) => Delivery::Retry,
        Err(error) if error.is_connect() => Delivery::Retry,
        _ => Delivery::Discard,
    }
}

fn retryable(status: StatusCode) -> bool {
    matches!(status.as_u16(), 429 | 502 | 503 | 504)
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(250 * 2_u64.pow(attempt.min(3)))
}

#[cfg(test)]
mod tests;
