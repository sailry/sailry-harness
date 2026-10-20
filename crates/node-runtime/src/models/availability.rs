//! Execution availability belongs to the selected Node, independently of installed models.
use sailry_protocol::{
    ErrorCode, Fault,
    models::{Availability, Catalog, Kind, Model},
};

pub(crate) fn ensure(kind: Kind) -> Result<(), Fault> {
    match kind {
        Kind::Decision => sailry_decision::available()
            .map_err(|error| Fault::new(ErrorCode::Unavailable, error.to_string())),
        Kind::Embedding | Kind::Ocr => Ok(()),
        Kind::Speech => Err(Fault::invalid("speech belongs to the controller")),
    }
}

pub(crate) fn catalog(models: Vec<Model>) -> Catalog {
    Catalog {
        models,
        availability: [Kind::Embedding, Kind::Ocr, Kind::Decision]
            .into_iter()
            .map(|kind| {
                let status = match ensure(kind) {
                    Ok(()) => Availability::Available,
                    Err(error) => Availability::Unavailable {
                        reason: error.message,
                    },
                };
                (kind, status)
            })
            .collect(),
    }
}
