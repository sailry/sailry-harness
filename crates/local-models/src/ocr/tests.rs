use super::*;

fn model(definition: &sailry_protocol::models::Definition) -> Model {
    Model {
        id: catalog::id(definition),
        definition: definition.clone(),
        state: State::Installed,
    }
}

#[test]
fn selects_installed_profiles_and_respects_disable() {
    let models = catalog::ocr().iter().map(model).collect::<Vec<_>>();
    let mut choice = Choice {
        enabled: true,
        model: None,
    };
    assert_eq!(select(&models, &choice).unwrap().id, models[0].id);
    choice.model = Some(models[1].id.clone());
    assert_eq!(select(&models, &choice).unwrap().id, models[1].id);
    choice.model = Some("missing-model".into());
    assert_eq!(select(&models, &choice).unwrap().id, models[0].id);
    choice.enabled = false;
    assert!(select(&models, &choice).is_none());
    choice.enabled = true;
    assert!(select(&[], &choice).is_none());
}

#[tokio::test]
async fn skips_only_unconfigured_pdf_recognition() {
    let profile = tempfile::tempdir().unwrap();
    let registry = Registry::new(Some(profile.path().into()), CancellationToken::new());
    let service = Service::new(registry.clone());
    assert_eq!(service.signature().unwrap(), [0; 32]);
    let parts = || {
        vec![crate::office::Part {
            name: "Empty page".into(),
            text: String::new(),
            position: sailry_protocol::knowledge::Position::Page { page: 1 },
        }]
    };
    let unchanged = document::pdf(
        &service,
        b"not a PDF",
        parts(),
        Lane::Interactive,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(unchanged.len(), 1);
    assert!(unchanged[0].text.is_empty());
    registry.shutdown().await;
    let registry = Registry::new(
        Some(profile.path().join("missing-profile")),
        CancellationToken::new(),
    );
    let service = Service::new(registry.clone());
    assert_eq!(
        service.signature().unwrap_err().code,
        ErrorCode::Unavailable
    );
    assert_eq!(
        document::pdf(
            &service,
            b"not a PDF",
            parts(),
            Lane::Interactive,
            &CancellationToken::new()
        )
        .err()
        .expect("invalid model storage must fail OCR")
        .code,
        ErrorCode::Unavailable
    );
    registry.shutdown().await;
}
