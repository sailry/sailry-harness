//! GitHub discovery is untrusted; only the signed catalog admits an update.
use super::{Failure, Result, config::Config, manifest, transfer};
use serde::Deserialize;

const PAGE_SIZE: usize = 100;
const PAGE_LIMIT: usize = 10;
const MAX_PAGE: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    state: String,
    size: u64,
    browser_download_url: String,
}

pub(super) struct Published {
    version: semver::Version,
    pub catalog: String,
    downloads: String,
}

impl Published {
    pub fn authenticate(&self, bytes: &[u8], config: &Config) -> Result<()> {
        let releases = manifest::parse(bytes, &config.keys)?;
        if releases.is_empty()
            || releases.iter().any(|release| {
                release.version != self.version.to_string()
                    || release.name != format!("Sailry-{}.zip", release.target)
                    || release.url != format!("{}/{}", self.downloads, release.name)
            })
        {
            return Err(invalid(
                "the signed catalog does not match its exact published version",
            ));
        }
        Ok(())
    }
}

pub(super) async fn list(client: &reqwest::Client, config: &Config) -> Result<Vec<Published>> {
    let mut published = Vec::new();
    for page in 1..=PAGE_LIMIT {
        let mut url = url::Url::parse(&config.source)
            .map_err(|_| invalid("invalid release discovery URL"))?;
        url.query_pairs_mut()
            .append_pair("per_page", &PAGE_SIZE.to_string())
            .append_pair("page", &page.to_string());
        let bytes = transfer::body(client, url.as_str(), MAX_PAGE).await?;
        let releases: Vec<Release> = serde_json::from_slice(&bytes)
            .map_err(|_| invalid("invalid GitHub release discovery response"))?;
        let count = releases.len();
        for release in releases {
            if let Some(release) = release.published(config)? {
                published.push(release);
            }
        }
        if count < PAGE_SIZE {
            published.sort_by(|left, right| right.version.cmp_precedence(&left.version));
            return Ok(published);
        }
    }
    Err(invalid("release discovery exceeds its page limit"))
}

impl Release {
    fn published(self, config: &Config) -> Result<Option<Published>> {
        let Some(version) = self
            .tag_name
            .strip_prefix('v')
            .and_then(|value| semver::Version::parse(value).ok())
        else {
            return Ok(None);
        };
        let current = semver::Version::parse(&config.version)
            .map_err(|_| invalid("invalid current application version"))?;
        if self.draft
            || !version.build.is_empty()
            || (self.prerelease && version.pre.is_empty())
            || !config.channel.accepts(&version)
            || version.cmp_precedence(&current) != std::cmp::Ordering::Greater
            || config
                .required
                .as_ref()
                .is_some_and(|required| required != &version.to_string())
        {
            return Ok(None);
        }
        let downloads = format!("{}/{}", config.downloads, self.tag_name);
        let targets = if config.target.ends_with("apple-darwin") {
            ["aarch64-apple-darwin", "x86_64-apple-darwin"]
        } else {
            ["aarch64-pc-windows-msvc", "x86_64-pc-windows-msvc"]
        };
        let mut names = vec!["update-v1.json".to_owned()];
        for target in targets {
            let archive = format!("Sailry-{target}");
            names.extend([
                format!("{archive}.zip"),
                format!("{archive}.zip.update.json"),
                format!("SHA256SUMS-desktop-{target}"),
            ]);
            if target.ends_with("apple-darwin") {
                names.push(format!("{archive}.dmg"));
            }
        }
        for name in names {
            let mut matches = self.assets.iter().filter(|asset| asset.name == name);
            let Some(asset) = matches.next() else {
                return Ok(None);
            };
            if matches.next().is_some()
                || asset.browser_download_url != format!("{downloads}/{name}")
            {
                return Err(invalid("the published signed release asset set is invalid"));
            }
            if asset.state != "uploaded" || asset.size == 0 {
                return Ok(None);
            }
            if name == "update-v1.json" && asset.size > super::config::MAX_MANIFEST as u64 {
                return Err(invalid(
                    "the published signed catalog exceeds its size limit",
                ));
            }
        }
        Ok(Some(Published {
            version,
            catalog: format!("{downloads}/update-v1.json"),
            downloads,
        }))
    }
}

fn invalid(message: &str) -> Failure {
    Failure::new("updates_manifest_invalid", message)
}

#[cfg(test)]
#[path = "tests/discovery.rs"]
mod tests;
