use futures::StreamExt;
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault};
use sha2::{Digest, Sha256};
use std::{fs::File, time::Duration};
use tokio::io::AsyncWriteExt;

#[derive(Default)]
pub(super) struct Network {
    #[cfg(any(test, feature = "test-support"))]
    endpoint: std::sync::RwLock<Option<url::Url>>,
}

impl Network {
    #[cfg(any(test, feature = "test-support"))]
    pub fn fixture(&self, endpoint: &str) {
        let endpoint = url::Url::parse(endpoint).expect("code fixture URL");
        assert_eq!(endpoint.scheme(), "http");
        assert!(matches!(
            endpoint.host_str(),
            Some("127.0.0.1" | "localhost" | "[::1]")
        ));
        *self.endpoint.write().expect("code fixture lock is healthy") = Some(endpoint);
    }

    fn address(&self, address: &str) -> Result<url::Url, Fault> {
        let url =
            url::Url::parse(address).map_err(|_| Fault::invalid("invalid code download URL"))?;
        if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
            return Err(Fault::invalid(
                "code downloads require HTTPS without embedded credentials",
            ));
        }
        #[cfg(any(test, feature = "test-support"))]
        if let Some(mut endpoint) = self
            .endpoint
            .read()
            .expect("code fixture lock is healthy")
            .clone()
        {
            endpoint.set_path(&format!(
                "/{}{}",
                url.host_str().unwrap_or_default(),
                url.path()
            ));
            endpoint.set_query(url.query());
            return Ok(endpoint);
        }
        Ok(url)
    }

    pub async fn fetch(
        &self,
        address: &str,
        limit: u64,
        stop: &CancellationToken,
    ) -> Result<(File, String), Fault> {
        let address = self.address(address)?;
        let initial_scheme = address.scheme().to_owned();
        let client = reqwest::Client::builder()
            .user_agent("Sailry-code/1")
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= 5
                    || attempt.url().scheme() != initial_scheme
                    || !attempt.url().username().is_empty()
                    || attempt.url().password().is_some()
                {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(unavailable)?;
        let download = async {
            let response = client.get(address).send().await.map_err(unavailable)?;
            if !response.status().is_success() {
                return Err(Fault::new(
                    if response.status() == reqwest::StatusCode::NOT_FOUND {
                        ErrorCode::NotFound
                    } else {
                        ErrorCode::Unavailable
                    },
                    "code source could not be downloaded",
                ));
            }
            if response.content_length().is_some_and(|size| size > limit) {
                return Err(Fault::invalid("code download exceeds the size limit"));
            }
            let file = tempfile::tempfile().map_err(unavailable)?;
            let mut output = tokio::fs::File::from_std(file.try_clone().map_err(unavailable)?);
            let mut stream = response.bytes_stream();
            let mut size = 0u64;
            let mut digest = Sha256::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(unavailable)?;
                size = size
                    .checked_add(chunk.len() as u64)
                    .filter(|size| *size <= limit)
                    .ok_or_else(|| Fault::invalid("code download exceeds the size limit"))?;
                digest.update(&chunk);
                output.write_all(&chunk).await.map_err(unavailable)?;
            }
            output.flush().await.map_err(unavailable)?;
            drop(output);
            Ok((file, format!("{:x}", digest.finalize())))
        };
        tokio::select! {biased;_ = stop.cancelled()=>Err(Fault::new(ErrorCode::Cancelled,"code download was cancelled")),result=download=>result}
    }

    pub async fn bytes(
        &self,
        address: &str,
        limit: u64,
        stop: &CancellationToken,
    ) -> Result<Vec<u8>, Fault> {
        let (mut file, _) = self.fetch(address, limit, stop).await?;
        tokio::task::spawn_blocking(move || {
            use std::io::{Read, Seek};
            file.rewind().map_err(unavailable)?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).map_err(unavailable)?;
            Ok(bytes)
        })
        .await
        .map_err(unavailable)?
    }

    pub async fn json(
        &self,
        address: &str,
        stop: &CancellationToken,
    ) -> Result<serde_json::Value, Fault> {
        serde_json::from_slice(&self.bytes(address, 16 * 1024 * 1024, stop).await?)
            .map_err(|error| Fault::invalid(format!("invalid code source metadata: {error}")))
    }
}

fn unavailable(error: impl std::fmt::Display) -> Fault {
    Fault::new(ErrorCode::Unavailable, error.to_string())
}
