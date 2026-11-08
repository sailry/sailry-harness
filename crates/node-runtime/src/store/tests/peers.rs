use crate::store::database::{Database, WRITE_CONCURRENCY, conflict};
use sailry_link::EndpointAddr;
use sailry_protocol::{ErrorCode, Fault, NodeId};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retries_concurrent_pairing_writes() -> Result<(), crate::Error> {
    let directory = tempfile::tempdir()?;
    let storage = Database::open(&directory.path().join("node.turso"), NodeId([5; 32]), None)
        .await?
        .into_storage();
    let address = EndpointAddr::new(iroh::SecretKey::from_bytes(&[7; 32]).public())
        .with_ip_addr("127.0.0.1:1234".parse().unwrap());
    let peer = NodeId(*address.id.as_bytes());
    let mut contexts = Vec::with_capacity(WRITE_CONCURRENCY);
    for _ in 0..WRITE_CONCURRENCY {
        contexts.push(storage.context(false).await?);
    }
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(contexts.len()));
    let mut writes = tokio::task::JoinSet::new();
    for mut database in contexts {
        let barrier = barrier.clone();
        let address = address.clone();
        writes.spawn(async move {
            barrier.wait().await;
            let trusted = database.set_trust(peer, true).await;
            barrier.wait().await;
            let remembered = database.remember(address).await;
            (trusted, remembered, database)
        });
    }
    let mut completed = 0;
    while let Some(result) = writes.join_next().await {
        let (trusted, remembered, database) = result.expect("pairing worker completed");
        trusted?;
        remembered?;
        assert!(database.connection.is_autocommit()?);
        completed += 1;
    }
    assert_eq!(completed, WRITE_CONCURRENCY);
    assert_eq!(storage.writes.available_permits(), WRITE_CONCURRENCY);
    let observer = storage.context(true).await?;
    assert_eq!(observer.peers().await?, vec![peer]);
    assert_eq!(observer.addresses().await?, vec![address]);
    assert!(observer.connection.is_autocommit()?);
    Ok(())
}

#[tokio::test]
async fn writes_bypass_unrelated_transaction() -> Result<(), crate::Error> {
    let directory = tempfile::tempdir()?;
    let storage = Database::open(&directory.path().join("node.turso"), NodeId([5; 32]), None)
        .await?
        .into_storage();
    let mut database = storage.context(false).await?;
    let observer = storage.context(true).await?;
    let writer = storage.handle.writer().await?;
    let transaction = conflict::begin(&writer).await?;
    transaction
        .execute(
            "INSERT INTO oauth_registrations(provider,client_id) VALUES('fixture','client')",
            (),
        )
        .await?;
    let address = EndpointAddr::new(iroh::SecretKey::from_bytes(&[7; 32]).public())
        .with_ip_addr("127.0.0.1:1234".parse().unwrap());
    let peer = NodeId(*address.id.as_bytes());

    database.set_trust(peer, true).await?;
    database.remember(address.clone()).await?;
    assert_eq!(observer.peers().await?, vec![peer]);
    assert_eq!(observer.addresses().await?, vec![address.clone()]);
    let updated = address.with_ip_addr("127.0.0.1:5678".parse().unwrap());
    database.remember(updated.clone()).await?;
    assert_eq!(observer.addresses().await?, vec![updated]);
    assert!(database.connection.is_autocommit()?);
    assert_eq!(storage.writes.available_permits(), WRITE_CONCURRENCY - 1);
    assert!(!writer.is_autocommit()?);

    database.set_trust(peer, false).await?;
    assert!(observer.peers().await?.is_empty());
    assert!(observer.addresses().await?.is_empty());
    assert!(database.connection.is_autocommit()?);
    assert_eq!(storage.writes.available_permits(), WRITE_CONCURRENCY - 1);
    conflict::commit(transaction).await?;
    drop(writer);
    assert_eq!(storage.writes.available_permits(), WRITE_CONCURRENCY);
    let stored: String = observer
        .connection
        .prepare("SELECT client_id FROM oauth_registrations WHERE provider='fixture'")
        .await?
        .query_row(())
        .await?
        .get(0)?;
    assert_eq!(stored, "client");
    Ok(())
}

#[tokio::test]
async fn capacity_rejection_releases_transaction() -> Result<(), crate::Error> {
    let directory = tempfile::tempdir()?;
    let mut database =
        Database::open(&directory.path().join("node.turso"), NodeId([5; 32]), None).await?;
    let expected = (0..128)
        .map(|value| NodeId([value; 32]))
        .collect::<Vec<_>>();
    let transaction = conflict::begin(&database.connection).await?;
    for peer in &expected {
        transaction
            .execute("INSERT INTO link_peers(identity) VALUES(?1)", [&peer.0[..]])
            .await?;
    }
    conflict::commit(transaction).await?;

    database.set_trust(expected[0], true).await?;
    let additional = NodeId([128; 32]);
    assert_eq!(
        database.set_trust(additional, true).await,
        Err(Fault::new(
            ErrorCode::Busy,
            "paired device capacity exhausted"
        ))
    );
    assert_eq!(database.peers().await?, expected);
    assert!(database.connection.is_autocommit()?);
    assert_eq!(database.writes.available_permits(), WRITE_CONCURRENCY);

    database.set_trust(expected[0], false).await?;
    database.set_trust(additional, true).await?;
    let mut replaced = expected[1..].to_vec();
    replaced.push(additional);
    assert_eq!(database.peers().await?, replaced);
    assert!(database.connection.is_autocommit()?);
    assert_eq!(database.writes.available_permits(), WRITE_CONCURRENCY);
    Ok(())
}

#[tokio::test]
async fn closed_budget_preserves_records() -> Result<(), crate::Error> {
    let directory = tempfile::tempdir()?;
    let mut database =
        Database::open(&directory.path().join("node.turso"), NodeId([5; 32]), None).await?;
    let address = EndpointAddr::new(iroh::SecretKey::from_bytes(&[7; 32]).public())
        .with_ip_addr("127.0.0.1:1234".parse().unwrap());
    let peer = NodeId(*address.id.as_bytes());
    database.set_trust(peer, true).await?;
    database.remember(address.clone()).await?;
    database.writes.close();
    let expected = Fault::new(ErrorCode::Unavailable, "Node storage is closing");

    assert_eq!(database.set_trust(peer, false).await, Err(expected.clone()));
    assert_eq!(
        database
            .remember(
                address
                    .clone()
                    .with_ip_addr("127.0.0.1:5678".parse().unwrap())
            )
            .await,
        Err(expected)
    );
    assert_eq!(database.peers().await?, vec![peer]);
    assert_eq!(database.addresses().await?, vec![address]);
    assert!(database.connection.is_autocommit()?);
    assert_eq!(database.writes.available_permits(), WRITE_CONCURRENCY);
    Ok(())
}

#[tokio::test]
async fn rejected_address_releases_transaction() -> Result<(), crate::Error> {
    let directory = tempfile::tempdir()?;
    let mut database =
        Database::open(&directory.path().join("node.turso"), NodeId([5; 32]), None).await?;
    let address = EndpointAddr::new(iroh::SecretKey::from_bytes(&[7; 32]).public())
        .with_ip_addr("127.0.0.1:1234".parse().unwrap());
    let error = database.remember(address.clone()).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::Internal);
    assert!(database.addresses().await?.is_empty());
    assert!(database.peers().await?.is_empty());
    assert!(database.connection.is_autocommit()?);
    assert_eq!(database.writes.available_permits(), WRITE_CONCURRENCY);

    let peer = NodeId(*address.id.as_bytes());
    database.set_trust(peer, true).await?;
    database.remember(address.clone()).await?;
    assert_eq!(database.peers().await?, vec![peer]);
    assert_eq!(database.addresses().await?, vec![address]);
    assert!(database.connection.is_autocommit()?);
    assert_eq!(database.writes.available_permits(), WRITE_CONCURRENCY);
    Ok(())
}
