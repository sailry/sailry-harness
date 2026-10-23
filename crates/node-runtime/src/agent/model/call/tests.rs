use super::*;
use adk_core::LlmResponse;
use futures::StreamExt;

#[tokio::test]
async fn isolates_concurrent_stream_identities() {
    async fn consume() -> sailry_protocol::RequestId {
        let identity = identity(crate::logging::Context::default());
        let call = identity.call.unwrap();
        let responses = scope(identity.clone(), async {
            assert_eq!(current().unwrap().call, Some(call));
            let mut polls = 0;
            Box::pin(futures::stream::poll_fn(move |cx| {
                assert_eq!(current().unwrap().call, Some(call));
                polls += 1;
                if polls == 1 {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    Poll::Ready((polls == 2).then_some(Ok(LlmResponse::default())))
                }
            })) as LlmResponseStream
        })
        .await;
        assert!(current().is_none());
        let responses = stream(identity, responses).collect::<Vec<_>>().await;
        assert_eq!(responses.len(), 1);
        call
    }
    let (first, second) = futures::join!(consume(), consume());
    assert_ne!(first, second);
    assert!(current().is_none());
}

#[tokio::test]
async fn restores_parent_after_nested_calls() {
    let parent = identity(crate::logging::Context::default());
    scope(parent.clone(), async {
        let child = identity(crate::logging::Context::default());
        assert_ne!(child.call, parent.call);
        scope(child.clone(), async {
            assert_eq!(current().unwrap().call, child.call);
        })
        .await;
        assert_eq!(current().unwrap().call, parent.call);
    })
    .await;
}
