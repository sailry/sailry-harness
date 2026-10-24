use super::*;
use sailry_protocol::models::{Choice, State};
use tokio::{
    sync::mpsc,
    time::{Duration, timeout},
};

fn model(kind: Kind, state: State) -> Model {
    let (id, definition) = catalog::definitions(&[])
        .unwrap()
        .into_iter()
        .find(|(_, definition)| definition.format.kind() == kind)
        .unwrap();
    Model {
        id,
        definition: definition.as_ref().clone(),
        state,
    }
}

fn update(models: Vec<Model>, ocr: bool) -> Update {
    Update {
        models,
        embedding: Choice {
            enabled: true,
            model: None,
        },
        ocr: Choice {
            enabled: ocr,
            model: None,
        },
    }
}

#[test]
fn reacts_to_embedding_and_ocr_changes() {
    let idle = update(
        vec![
            model(Kind::Embedding, State::Missing),
            model(Kind::Ocr, State::Missing),
        ],
        true,
    );
    let mut downloading = idle.clone();
    downloading.models[1].state = State::Downloading;
    assert_eq!(reaction(&idle, &downloading), None);
    let mut ocr = idle.clone();
    ocr.models[1].state = State::Installed;
    assert_eq!(reaction(&idle, &ocr), Some(Reaction::Refresh));
    assert_eq!(
        reaction(&idle, &update(idle.models.clone(), false)),
        Some(Reaction::Refresh)
    );
    let mut embedding = ocr.clone();
    embedding.models[0].state = State::Installed;
    assert_eq!(reaction(&ocr, &embedding), Some(Reaction::Invalidate));
    let mut selected = idle.clone();
    selected.embedding.model = Some(idle.models[0].id.clone());
    assert_eq!(reaction(&idle, &selected), Some(Reaction::Invalidate));
}

#[tokio::test]
async fn retains_registration_limits_and_identity() {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../schema.sql"))
        .await
        .unwrap();
    initialize(&db).await.unwrap();
    let (record, _) = crate::models::test_support::record(2, &[100., 0., 1., 0., 0., 1.]);
    let mut definition = record.definition;
    let mut last = None;
    for index in 0..catalog::LIMIT {
        definition.name = format!("Registration {index}");
        let (output, event) = register(&db, &definition).await.unwrap();
        assert!(event.is_none());
        last = Some(output);
    }
    assert_eq!(register(&db, &definition).await.unwrap().0, last.unwrap());
    definition.name = "Beyond the limit".into();
    assert_eq!(
        register(&db, &definition).await.unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    let builtin = &catalog::ocr()[0];
    assert_eq!(
        register(&db, builtin).await.unwrap().0,
        Output::LocalModel {
            id: catalog::id(builtin),
        }
    );
    let count: i64 = (async {
        let mut statement = db.prepare("SELECT COUNT(*) FROM local_models").await?;
        let row = statement.query_row(()).await?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped
    })
    .await
    .unwrap();
    assert_eq!(count, catalog::LIMIT as i64);
    assert_eq!(
        read(&db).await.unwrap().definitions.len(),
        catalog::LIMIT + 6
    );
}

#[tokio::test]
async fn survives_failed_writes() {
    let initial = update(vec![model(Kind::Ocr, State::Missing)], true);
    let (sender, receiver) = watch::channel(initial.clone());
    let (written, mut writes) = mpsc::unbounded_channel();
    let attempts = std::sync::atomic::AtomicUsize::new(0);
    let (reacted, mut reactions) = mpsc::unbounded_channel();
    let follower = tokio::spawn(follow(
        receiver,
        initial.clone(),
        CancellationToken::new(),
        move |models: Vec<Model>| {
            let first = attempts.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0;
            let _ = written.send(models);
            async move {
                if first {
                    Err(Fault::new(ErrorCode::Unavailable, "event write failed"))
                } else {
                    Ok(())
                }
            }
        },
        move |reaction| {
            let _ = reacted.send(reaction);
        },
    ));
    let mut installed = initial.clone();
    installed.models[0].state = State::Installed;
    sender.send(installed.clone()).unwrap();
    let wait = Duration::from_secs(10);
    assert_eq!(
        timeout(wait, writes.recv()).await.unwrap().unwrap(),
        installed.models
    );
    assert_eq!(
        timeout(wait, reactions.recv()).await.unwrap(),
        Some(Reaction::Refresh)
    );
    let disabled = update(installed.models.clone(), false);
    sender.send(disabled.clone()).unwrap();
    // The failed catalog is written again with the next change.
    assert_eq!(
        timeout(wait, writes.recv()).await.unwrap().unwrap(),
        disabled.models
    );
    assert_eq!(
        timeout(wait, reactions.recv()).await.unwrap(),
        Some(Reaction::Refresh)
    );
    sender.send(update(disabled.models.clone(), true)).unwrap();
    assert_eq!(
        timeout(wait, reactions.recv()).await.unwrap(),
        Some(Reaction::Refresh)
    );
    assert!(writes.try_recv().is_err());
    drop(sender);
    timeout(wait, follower).await.unwrap().unwrap();
}
