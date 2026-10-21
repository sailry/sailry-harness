//! Typed image recognition shares the Node's model registry and CPU scheduler.
pub(crate) mod document;
use crate::models::{
    registry::{Registry, Resource, catalog},
    runtime::{Engine, Lane},
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    models::{Choice, Kind, Model, State},
};
use std::sync::Arc;

pub(crate) struct Service {
    registry: Arc<Registry>,
}

impl Service {
    pub(crate) fn new(registry: Arc<Registry>) -> Arc<Self> {
        Arc::new(Self { registry })
    }

    pub(crate) fn recognize(
        &self,
        image: image::RgbImage,
        lane: Lane,
        cancel: &CancellationToken,
    ) -> Result<Vec<sailry_ocr::Line>, Fault> {
        self.recognize_resource(self.resource()?, image, lane, cancel)
    }

    pub(crate) fn signature(&self) -> Result<[u8; 32], Fault> {
        match self.resource() {
            Ok(resource) => Ok(*blake3::hash(resource.id.as_bytes()).as_bytes()),
            Err(error) if error.code == ErrorCode::NotConfigured => Ok([0; 32]),
            Err(error) => Err(error),
        }
    }

    fn resource(&self) -> Result<Resource, Fault> {
        let choice = self.registry.settings().choice(Kind::Ocr);
        if !choice.enabled {
            return Err(Fault::new(ErrorCode::NotConfigured, "OCR is disabled"));
        }
        self.registry.storage()?;
        // Startup and installation verify files; each execution checks the selected stamp.
        let models = self.registry.cached(Some(Kind::Ocr));
        let selected = select(&models, &choice).ok_or_else(|| {
            Fault::new(ErrorCode::NotConfigured, "an OCR model must be installed")
        })?;
        self.registry
            .resource(Kind::Ocr, &selected.id, Some(&choice))
    }

    fn recognize_resource(
        &self,
        resource: Resource,
        image: image::RgbImage,
        lane: Lane,
        cancel: &CancellationToken,
    ) -> Result<Vec<sailry_ocr::Line>, Fault> {
        self.registry
            .blocking(resource, lane, cancel, move |engine, cancel| {
                let Engine::Ocr(engine) = engine else {
                    return Err(Fault::invalid("OCR engine required"));
                };
                engine
                    .recognize(&image, || cancel.is_cancelled())
                    .map_err(|error| match error {
                        sailry_ocr::Error::Cancelled => cancelled(),
                        error => failure(error),
                    })
            })
    }
}

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "OCR operation was cancelled")
}
fn failure(error: impl std::fmt::Display) -> Fault {
    Fault::new(ErrorCode::Unavailable, format!("OCR failed: {error}"))
}

fn select<'a>(models: &'a [Model], choice: &Choice) -> Option<&'a Model> {
    if !choice.enabled {
        return None;
    }
    let installed = |id: &str| {
        models
            .iter()
            .find(|model| model.id == id && model.state == State::Installed)
    };
    choice
        .model
        .as_deref()
        .and_then(installed)
        .or_else(|| {
            catalog::ocr()
                .iter()
                .find_map(|definition| installed(&catalog::id(definition)))
        })
        .or_else(|| models.iter().find(|model| model.state == State::Installed))
}

#[cfg(test)]
mod tests;
