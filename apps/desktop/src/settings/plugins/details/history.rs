//! Official history is response metadata, independent of the installed package capture.
use super::*;
use sailry_protocol::plugin::{Origin, catalog::Version};

#[derive(Default)]
pub(super) struct State {
    pub(super) versions: Vec<Version>,
    pub(super) open: bool,
    pub(super) pending: bool,
    loaded: bool,
    task: Option<Task<()>>,
}

fn id(info: &Info) -> Option<&str> {
    let Origin::Online { source, path } = info.origin.as_ref()? else {
        return None;
    };
    (info.skill.is_none()
        && path == &info.summary.name
        && view::official_repository(&source.repository))
    .then_some(path.as_str())
}

impl Details {
    pub(super) fn load_history(&mut self, cx: &mut Context<Self>) {
        if self.stop.is_cancelled()
            || !matches!(self.target, Target::Installed(_))
            || self.history.pending
            || self.history.loaded
        {
            return;
        }
        let Some(id) = self.info.as_ref().and_then(id).map(str::to_owned) else {
            return;
        };
        let binding = self.binding.clone();
        let stop = self.stop.clone();
        self.history.pending = true;
        let job = binding.runtime.spawn(async move {
            tokio::select! { biased;
                _ = stop.cancelled() => None,
                result = binding.client.execute(binding.client.prepare(Command::ReadCatalogPluginInfo {
                    source: Source::Official, id, bundled: false,
                })) => Some(result),
            }
        });
        self.history.task = Some(cx.spawn(async move |details, cx| {
            let result = job.await;
            _ = details.update(cx, |details, cx| {
                if details.stop.is_cancelled() {
                    return;
                }
                details.history.pending = false;
                match result {
                    Ok(Some(Ok(Output::CatalogDetails(
                        sailry_protocol::plugin::catalog::Details::Plugin(info),
                    )))) if info.summary.name == details.name() => {
                        details.history.versions = info.versions;
                        details.history.loaded = true;
                    }
                    Ok(Some(Err(error))) => {
                        details.required = compatibility::required(&error).map(str::to_owned);
                        details.error = Some(live::error_key(&error));
                    }
                    _ => details.error = Some("plugins_market_failed"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn info(origin: serde_json::Value) -> Info {
        serde_json::from_value(serde_json::json!({
            "summary": {"name":"example", "revision":1, "digest":"capture",
                "settings_revision":0, "version":"0.1.0"},
            "origin": origin,
        }))
        .unwrap()
    }

    #[test]
    fn reads_only_official_package_history() {
        for repository in [
            "https://github.com/sailry/sailry-plugins",
            "https://github.com/sailry/sailry-plugins.git",
        ] {
            let official = info(serde_json::json!({"kind":"online", "source": {
                "repository":repository, "git_ref":"main", "commit":"a".repeat(40),
            }, "path":"example"}));
            assert_eq!(id(&official), Some("example"));
        }
        for origin in [
            serde_json::json!({"kind":"bundled"}),
            serde_json::json!({"kind":"directory"}),
            serde_json::json!({"kind":"online", "source": {
                "repository":"https://github.com/example/plugins.git", "git_ref":"main", "commit":"a".repeat(40),
            }, "path":"example"}),
            serde_json::json!({"kind":"online", "source": {
                "repository":"https://github.com/Sailry/other.git", "git_ref":"main", "commit":"a".repeat(40),
            }, "path":"example"}),
        ] {
            assert_eq!(id(&info(origin)), None);
        }
    }
}
