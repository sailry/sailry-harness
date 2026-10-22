use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sailry_protocol::plugin::desktop::{Bundle, MAX_BYTES, MAX_IMAGE_BYTES};
use std::time::Duration;

impl Host {
    pub(crate) async fn read_view(
        &self,
        info: Info,
        surface: sailry_protocol::plugin::desktop::Surface,
        closed: CancellationToken,
    ) -> Result<Bundle, Fault> {
        let stop = closed.child_token();
        let _guard = stop.clone().drop_guard();
        let host = self.clone();
        let work = async move {
            let permit = host.read_permit(&stop).await?;
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                let package = info.summary.reference();
                let settings_entry = info
                    .extension
                    .as_ref()
                    .and_then(|extension| extension.settings_page.as_ref())
                    .and_then(|page| page.entry.clone());
                let card_entry = match surface {
                    sailry_protocol::plugin::desktop::Surface::Card(_) => {
                        // The store has already resolved the exact receipt declaration.
                        info.extension
                            .as_ref()
                            .and_then(|extension| extension.cards.first())
                            .map(|card| card.entry.clone())
                    }
                    _ => None,
                };
                let manifest = info
                    .extension
                    .and_then(|extension| extension.desktop)
                    .filter(|manifest| manifest.valid())
                    .ok_or_else(|| {
                        Fault::new(ErrorCode::NotConfigured, "plugin has no desktop view")
                    })?;
                let mut bundle = Bundle {
                    package,
                    entry: match surface {
                        sailry_protocol::plugin::desktop::Surface::Card(_) => {
                            card_entry.ok_or_else(|| Fault::invalid("plugin has no card entry"))?
                        }
                        sailry_protocol::plugin::desktop::Surface::Permissions => {
                            return Err(Fault::invalid(
                                "permission contributions have no desktop view",
                            ));
                        }
                        sailry_protocol::plugin::desktop::Surface::Workspace => {
                            manifest.entry.ok_or_else(|| {
                                sailry_protocol::Fault::invalid(
                                    "plugin view requires a native renderer",
                                )
                            })?
                        }
                        sailry_protocol::plugin::desktop::Surface::Settings => settings_entry
                            .ok_or_else(|| {
                                sailry_protocol::Fault::invalid("plugin has no settings entry")
                            })?,
                        sailry_protocol::plugin::desktop::Surface::Composer
                        | sailry_protocol::plugin::desktop::Surface::Provider
                        | sailry_protocol::plugin::desktop::Surface::Project => {
                            manifest.ui_entry.ok_or_else(|| {
                                sailry_protocol::Fault::invalid("plugin has no contribution entry")
                            })?
                        }
                    },
                    files: BTreeMap::new(),
                    images: BTreeMap::new(),
                };
                let directory = host.resolve(&bundle.package, stop.clone())?;
                let mut remaining = MAX_BYTES;
                let mut image_remaining = MAX_IMAGE_BYTES;
                for path in manifest.resources {
                    if stop.is_cancelled() {
                        return Err(cancelled());
                    }
                    if path.ends_with(".png") {
                        let bytes = read(&directory, &path, image_remaining / 4 * 3)?;
                        let encoded = STANDARD.encode(bytes);
                        image_remaining =
                            image_remaining.checked_sub(encoded.len()).ok_or_else(|| {
                                sailry_protocol::Fault::invalid(
                                    "plugin image resources exceed limit",
                                )
                            })?;
                        bundle.images.insert(path, encoded);
                    } else {
                        let bytes = read(&directory, &path, remaining)?;
                        remaining -= bytes.len();
                        let content = String::from_utf8(bytes).map_err(|_| {
                            sailry_protocol::Fault::invalid(
                                "plugin desktop resource is not UTF-8 text",
                            )
                        })?;
                        bundle.files.insert(path, content);
                    }
                }
                if stop.is_cancelled() {
                    return Err(cancelled());
                }
                Ok(bundle)
            })
            .await
            .map_err(|_| Fault::new(ErrorCode::Internal, "plugin resource worker failed"))?
        };
        tokio::select! {
            _ = closed.cancelled() => Err(cancelled()),
            result = tokio::time::timeout(Duration::from_secs(5), work) => result
                .map_err(|_| Fault::new(ErrorCode::Busy, "plugin resource read deadline exceeded"))?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture() -> (tempfile::TempDir, Host, Info) {
        let (directory, host, source) = crate::plugins::tests::fixture();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(source.join("plugin.json")).unwrap()).unwrap();
        manifest["extensions"] = serde_json::json!({"dev.sailry.platform": {
            "api_version":"v1", "actions":[],
            "desktop":{"entry":"dev.sailry.platform/main.js", "resources":["dev.sailry.platform/main.js"]}
        }});
        fs::write(source.join("plugin.json"), manifest.to_string()).unwrap();
        fs::write(
            source.join("dev.sailry.platform/main.js"),
            "export default class Example {}",
        )
        .unwrap();
        let info = host.install(&source, "", "example").unwrap();
        (directory, host, info)
    }

    #[tokio::test]
    async fn waits_for_read_capacity() {
        let (_directory, host, info) = fixture();
        let permit = host.reads.clone().acquire_many_owned(2).await.unwrap();
        let stop = CancellationToken::new();
        let mut read = Box::pin(host.read_view(info, Default::default(), stop));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut read)
                .await
                .is_err()
        );
        drop(permit);
        assert!(read.await.unwrap().valid());
    }

    #[tokio::test]
    async fn cancels_queued_reads() {
        let (_directory, host, info) = fixture();
        let permit = host.reads.clone().acquire_many_owned(2).await.unwrap();
        let stop = CancellationToken::new();
        let mut read = Box::pin(host.read_view(info, Default::default(), stop.clone()));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut read)
                .await
                .is_err()
        );
        stop.cancel();
        assert_eq!(read.await.unwrap_err().code, ErrorCode::Cancelled);
        assert_eq!(host.reads.available_permits(), 0);
        drop(permit);
        assert_eq!(host.reads.available_permits(), 2);
    }

    #[test]
    fn rechecks_resource_content() {
        let (_directory, host, info) = fixture();
        let directory = host
            .resolve(&info.summary.reference(), CancellationToken::new())
            .unwrap();
        fs::write(
            directory.path.join("dev.sailry.platform/main.js"),
            "Changed",
        )
        .unwrap();
        assert_eq!(
            read(&directory, "dev.sailry.platform/main.js", MAX_BYTES)
                .unwrap_err()
                .code,
            ErrorCode::Unavailable
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_resource_symlinks() {
        let (temporary, host, info) = fixture();
        let directory = host
            .resolve(&info.summary.reference(), CancellationToken::new())
            .unwrap();
        let outside = temporary.path().join("outside");
        fs::write(&outside, "Do not expose this content").unwrap();
        fs::remove_file(directory.path.join("dev.sailry.platform/main.js")).unwrap();
        std::os::unix::fs::symlink(outside, directory.path.join("dev.sailry.platform/main.js"))
            .unwrap();
        assert!(read(&directory, "dev.sailry.platform/main.js", MAX_BYTES).is_err());
    }
}
