//! Typed image recognition shares the Node's model registry and CPU scheduler.
pub(crate) mod document;
use crate::models::{
    registry::Registry,
    runtime::{Engine, Lane},
};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, models::Kind};
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
        let resource = self.registry.resource(Kind::Ocr, None, None)?;
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

    pub(crate) fn signature(&self) -> [u8; 32] {
        self.registry.signature(Kind::Ocr)
    }
}

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "OCR operation was cancelled")
}
fn failure(error: impl std::fmt::Display) -> Fault {
    Fault::new(ErrorCode::Unavailable, format!("OCR failed: {error}"))
}
