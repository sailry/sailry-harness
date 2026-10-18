use super::*;
use crate::plugins::script::Bundle;

impl Host {
    /// The caller authorizes the exact captured reference before entering this
    /// immutable resource loader. Pure translations receive no SDK capability.
    pub(crate) fn read_translation(
        &self,
        reference: &Reference,
        script: &sailry_protocol::plugin::model_adapter::Script,
        stop: CancellationToken,
    ) -> Result<Bundle, Fault> {
        if !script.valid() {
            return Err(sailry_protocol::Fault::invalid(
                "plugin translation resources are invalid",
            ));
        }
        let directory = self.resolve(reference, stop.clone())?;
        let mut remaining = sailry_protocol::plugin::host::MAX_BYTES;
        let mut files = BTreeMap::new();
        for path in std::iter::once(&script.entry).chain(&script.resources) {
            if files.contains_key(path) {
                continue;
            }
            if stop.is_cancelled() {
                return Err(cancelled());
            }
            let bytes = read(&directory, path, remaining)?;
            remaining -= bytes.len();
            files.insert(
                path.clone(),
                String::from_utf8(bytes).map_err(|_| {
                    sailry_protocol::Fault::invalid("plugin translation resource is not UTF-8 text")
                })?,
            );
        }
        Ok(Bundle {
            entry: script.entry.clone(),
            files,
        })
    }

    /// Executed on a bounded callback worker before evaluating any package code.
    pub(crate) fn read_host(&self, info: &Info, stop: CancellationToken) -> Result<Bundle, Fault> {
        let manifest = info
            .extension
            .as_ref()
            .and_then(|extension| extension.host.as_ref())
            .filter(|manifest| manifest.valid())
            .ok_or_else(|| sailry_protocol::Fault::invalid("plugin has no host entry"))?;
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
                String::from_utf8(bytes).map_err(|_| {
                    sailry_protocol::Fault::invalid("plugin host resource is not UTF-8 text")
                })?,
            );
        }
        Ok(Bundle {
            entry: manifest.entry.clone(),
            files,
        })
    }
}
