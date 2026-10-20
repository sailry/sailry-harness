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
    replace: bool,
) -> Result<(), Fault> {
    if models.is_empty() {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "account model catalog is empty",
        ));
    }
    // Reconnecting the same account preserves its explicit model selection.
    // A replacement account uses its complete catalog and retains shared choices.
    if !replace && !provider.models.is_empty() {
        for configured in &mut provider.models {
            if let Some(discovered) = models.iter().find(|model| model.id == configured.id) {
                features(configured, discovered);
            }
        }
        return Ok(());
    }
    let mut references = BTreeMap::new();
    if crate::store::providers::catalog::status(db)?.revision != 0 {
        for models in models.chunks(100) {
            let mut query = catalog::Query {
                provider: matches!(
                    provider.authentication,
                    Authentication::ChatGpt | Authentication::ChatGptOAuth
                )
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
    let configured = std::mem::take(&mut provider.models)
        .into_iter()
        .map(|model| (model.id.clone(), model))
        .collect::<BTreeMap<_, _>>();
    provider.models = models
        .iter()
        .map(|model| {
            configured
                .get(&model.id)
                .cloned()
                .map(|mut configured| {
                    features(&mut configured, model);
                    configured
                })
                .unwrap_or_else(|| {
                    model.configuration(
                        references.get(&model.id),
                        provider.api,
                        provider.authentication,
                    )
                })
        })
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

fn features(configured: &mut sailry_protocol::conversation::Model, discovered: &discovery::Model) {
    let features = discovered
        .capabilities
        .as_ref()
        .map(|value| value.features.clone())
        .unwrap_or_default();
    if let Some(capabilities) = &mut configured.capabilities {
        capabilities.features = features;
    } else if !features.is_empty() {
        configured.capabilities = Some(discovery::Capabilities {
            features,
            ..Default::default()
        });
    }
}
