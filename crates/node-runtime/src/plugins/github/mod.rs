//! GitHub imports become ordinary immutable packages owned by the execution Node.
use super::{invalid, io_error};
use sailry_link::CancellationToken;
#[cfg(any(test, feature = "test-support"))]
use sailry_protocol::plugin::skills::Resolved;
use sailry_protocol::{ErrorCode, Fault};
use std::{fs::File, io::Write, time::Duration};

pub(super) mod source;

#[derive(Clone, Default)]
pub(crate) struct Github {
    #[cfg(any(test, feature = "test-support"))]
    pub(super) endpoint: Option<url::Url>,
}

impl Github {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn fixture(endpoint: &str) -> Self {
        let endpoint = url::Url::parse(endpoint).expect("skill fixture URL");
        assert_eq!(endpoint.scheme(), "http");
        assert!(matches!(
            endpoint.host_str(),
            Some("127.0.0.1" | "localhost" | "[::1]")
        ));
        Self {
            endpoint: Some(endpoint),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    fn url(&self, archive: bool, parts: &[&str]) -> url::Url {
        #[cfg(any(test, feature = "test-support"))]
        let fixture = self.endpoint.clone();
        #[cfg(not(any(test, feature = "test-support")))]
        let fixture: Option<url::Url> = None;
        let mut url = fixture.unwrap_or_else(|| {
            url::Url::parse(if archive {
                "https://codeload.github.com"
            } else {
                "https://api.github.com"
            })
            .unwrap()
        });
        url.path_segments_mut().unwrap().clear().extend(parts);
        url
    }

    pub(super) async fn download(
        &self,
        url: url::Url,
        accept: &str,
        limit: u64,
        stop: &CancellationToken,
    ) -> Result<File, Fault> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .user_agent("Sailry")
            .build()
            .map_err(|_| unavailable())?;
        let download = async {
            let mut response = client
                .get(url)
                .header(reqwest::header::ACCEPT, accept)
                .send()
                .await
                .map_err(|_| unavailable())?;
            if !response.status().is_success() {
                return Err(Fault::new(
                    if response.status() == reqwest::StatusCode::NOT_FOUND {
                        ErrorCode::NotFound
                    } else {
                        ErrorCode::Unavailable
                    },
                    "GitHub source could not be downloaded",
                ));
            }
            if response.content_length().is_some_and(|size| size > limit) {
                return Err(invalid("plugin download exceeds its size limit"));
            }
            let mut file = tempfile::tempfile().map_err(io_error)?;
            let mut size = 0u64;
            while let Some(bytes) = response.chunk().await.map_err(|_| unavailable())? {
                size = size
                    .checked_add(bytes.len() as u64)
                    .filter(|size| *size <= limit)
                    .ok_or_else(|| invalid("plugin download exceeds its size limit"))?;
                file.write_all(&bytes).map_err(io_error)?;
            }
            Ok(file)
        };
        tokio::select! { _ = stop.cancelled() => Err(cancelled()), result = download => result }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) async fn archive(
        &self,
        selected: &source::Selection,
        commit: &str,
        stop: &CancellationToken,
    ) -> Result<File, Fault> {
        self.download(
            self.url(true, &[&selected.owner, &selected.repo, "zip", commit]),
            "application/zip",
            sailry_protocol::plugin::MAX_PACKAGE_BYTES,
            stop,
        )
        .await
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) async fn resolve(
        &self,
        selected: &source::Selection,
        stop: &CancellationToken,
    ) -> Result<Resolved, Fault> {
        use std::io::{Read, Seek};
        let mut file = self
            .download(
                self.url(
                    false,
                    &[
                        "repos",
                        &selected.owner,
                        &selected.repo,
                        "commits",
                        &selected.git_ref,
                    ],
                ),
                "application/vnd.github.sha",
                1024,
                stop,
            )
            .await?;
        file.rewind().map_err(io_error)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(io_error)?;
        let commit = std::str::from_utf8(&bytes)
            .map_err(|_| invalid("invalid GitHub commit response"))?
            .trim();
        if !source::commit(commit) {
            return Err(invalid("invalid GitHub commit identifier"));
        }
        Ok(Resolved {
            repository: selected.repository.clone(),
            git_ref: selected.git_ref.clone(),
            commit: commit.into(),
        })
    }
}

pub(super) fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "GitHub source is unavailable")
}
pub(super) fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "repository request cancelled")
}
