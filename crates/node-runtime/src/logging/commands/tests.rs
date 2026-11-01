use super::*;
use sailry_protocol::{Receipt, RequestId};
use std::time::Duration;

#[tokio::test]
async fn releases_cancelled_read_receivers() {
    let (mut owner, completion) = tokio::sync::oneshot::channel();
    let admission = admitted(
        Span::query(Entry::new(Area::Command, Phase::Begin)),
        Admission {
            receipt: Receipt {
                id: RequestId::new(),
                durable: false,
            },
            completion,
        },
        true,
    );
    drop(admission);
    tokio::time::timeout(Duration::from_secs(1), owner.closed())
        .await
        .expect("logging must release the cancelled read receiver");
}

#[tokio::test]
async fn retains_durable_result_observers() {
    let (mut owner, completion) = tokio::sync::oneshot::channel();
    let admission = admitted(
        Span::new(Entry::new(Area::Command, Phase::Begin)),
        Admission {
            receipt: Receipt {
                id: RequestId::new(),
                durable: true,
            },
            completion,
        },
        true,
    );
    drop(admission);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), owner.closed())
            .await
            .is_err()
    );
    owner
        .send(Ok(Output::PluginResult(serde_json::Value::Null)))
        .unwrap();
}
