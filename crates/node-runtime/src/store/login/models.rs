use rusqlite::Connection;
use sailry_protocol::{
    Authentication, ErrorCode, Fault,
    conversation::{Provider, catalog, discovery},
};
use std::collections::BTreeMap;

pub(super) fn seed(
    db: &Connection,
    provider: &mut Provider,
    models: &[discovery::Model],
) -> Result<(), Fault> {
    if models.is_empty() {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "account model catalog is empty",
        ));
    }
    // A saved nonempty list is the user's model selection. Reconnecting must not
    // re-enable removed models or replace explicit limits and reasoning choices.
    if !provider.models.is_empty() {
        return Ok(());
    }
    let mut references = BTreeMap::new();
    if crate::store::providers::catalog::status(db)?.revision != 0 {
        for models in models.chunks(100) {
            let mut query = catalog::Query {
                provider: (provider.authentication == Authentication::ChatGpt)
                    .then(|| "openai".into()),
                ids: models.iter().map(|model| model.id.clone()).collect(),
                revision: None,
                after: None,
                limit: 100,
            };
            loop {
                let page = crate::store::providers::catalog::page(db, &query)?;
                references.extend(
                    page.models
                        .into_iter()
                        .map(|model| (model.id.clone(), model)),
                );
                query.revision = Some(page.revision);
                query.after = page.next;
                if query.after.is_none() {
                    break;
                }
            }
        }
    }
    provider.models = models
        .iter()
        .map(|model| model.configuration(references.get(&model.id), provider.api))
        .collect();
    if !provider
        .models
        .iter()
        .any(|model| model.id == provider.default_model)
    {
        provider.default_model = provider.models[0].id.clone();
    }
    Ok(())
}
