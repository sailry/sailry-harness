//! Captured package templates are immutable trusted presentation resources.
use super::*;
use sailry_protocol::artifact::{MAX_VIEW_BYTES, ViewContent};

impl Host {
    pub(crate) async fn read_artifact_view(
        &self,
        info: Info,
        closed: CancellationToken,
    ) -> Result<Option<ViewContent>, Fault> {
        let Some(view) = info
            .extension
            .as_ref()
            .and_then(|extension| extension.artifact_view.clone())
        else {
            return Ok(None);
        };
        if !view.valid() {
            return Err(Fault::invalid("invalid artifact view declaration"));
        }
        let stop = closed.child_token();
        let _guard = stop.clone().drop_guard();
        let host = self.clone();
        let work = async move {
            tokio::task::spawn_blocking(move || {
                let directory = host.resolve(&info.summary.reference(), stop.clone())?;
                let mut remaining = MAX_VIEW_BYTES;
                let mut text = |path: &str| -> Result<String, Fault> {
                    if stop.is_cancelled() {
                        return Err(cancelled());
                    }
                    let bytes = read(&directory, path, remaining)?;
                    remaining -= bytes.len();
                    String::from_utf8(bytes)
                        .map_err(|_| Fault::invalid("artifact view is not UTF-8"))
                };
                let content = ViewContent {
                    template: text(&view.template)?,
                    styles: view
                        .styles
                        .iter()
                        .map(|path| text(path))
                        .collect::<Result<_, _>>()?,
                    scripts: view
                        .scripts
                        .iter()
                        .map(|path| text(path))
                        .collect::<Result<_, _>>()?,
                    libraries: view.libraries,
                };
                if !content.valid() {
                    return Err(Fault::invalid("artifact view exceeds limits"));
                }
                Ok(Some(content))
            })
            .await
            .map_err(|_| Fault::new(ErrorCode::Internal, "artifact resource worker failed"))?
        };
        tokio::select! {
            biased;
            _ = closed.cancelled() => Err(cancelled()),
            result = tokio::time::timeout(std::time::Duration::from_secs(5), work) => result
                .map_err(|_| Fault::new(ErrorCode::Busy, "artifact view read timed out"))?,
        }
    }
}

#[cfg(test)]
#[path = "artifact/tests.rs"]
mod tests;
