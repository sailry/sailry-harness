use super::*;
use crate::plugins::script::Bundle;

impl Host {
    /// Executed on a bounded callback worker before evaluating any package code.
    pub(crate) fn read_host(&self, info: &Info, stop: CancellationToken) -> Result<Bundle, Fault> {
        let manifest = info
            .extension
            .as_ref()
            .and_then(|extension| extension.host.as_ref())
            .filter(|manifest| manifest.valid())
            .ok_or_else(|| invalid("plugin has no host entry"))?;
        let directory = self.resolve(&info.summary.reference(), stop.clone())?;
        let mut remaining = sailry_protocol::plugin::host::MAX_BYTES;
        let mut files = BTreeMap::new();
        for path in &manifest.resources {
            if stop.is_cancelled() {
                return Err(cancelled());
            }
            let bytes = read(&directory, path, remaining)?;
            remaining -= bytes.len();
            files.insert(
                path.clone(),
                String::from_utf8(bytes)
                    .map_err(|_| invalid("plugin host resource is not UTF-8 text"))?,
            );
        }
        Ok(Bundle {
            entry: manifest.entry.clone(),
            files,
        })
    }
}
