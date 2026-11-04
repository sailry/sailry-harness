use super::*;
use sailry_protocol::RequestId;

#[test]
fn immediate_guidance_preserves_owners() {
    let controls = Controls::default();
    let turn = TurnId::new();
    let session = SessionId::new();
    let owner = CancellationToken::new();
    let child = CancellationToken::new();
    controls.register(turn, session, owner.clone());
    controls.register_child(session, SessionId::new(), TurnId::new(), child.clone());
    let runner = controls.runner(turn).unwrap();
    let operation = controls.begin_operation(turn).unwrap();
    let request = RequestId::new();
    assert!(controls.guidance(turn, request, true));
    assert!(operation.is_cancelled());
    assert!(!owner.is_cancelled());
    assert!(!runner.is_cancelled());
    assert!(!child.is_cancelled());
    assert!(controls.running(turn));
    let next = controls.begin_operation(turn).unwrap();
    assert!(!next.is_cancelled());
    assert!(!controls.guidance(turn, request, true));
    assert!(!next.is_cancelled());
    controls.finish_operation(turn);
    controls.finish_operation(turn);
    assert_eq!(controls.operations(turn), 0);
}

#[tokio::test]
async fn boundary_guidance_only_wakes_recipient() {
    let controls = Controls::default();
    let turn = TurnId::new();
    controls.register(turn, SessionId::new(), CancellationToken::new());
    let operation = controls.begin_operation(turn).unwrap();
    let notice = controls.guidance_notice(turn).unwrap();
    let notified = notice.notified();
    tokio::pin!(notified);
    notified.as_mut().enable();
    assert!(controls.guidance(turn, RequestId::new(), false));
    tokio::time::timeout(std::time::Duration::ZERO, notified)
        .await
        .unwrap();
    assert!(!operation.is_cancelled());
    assert!(controls.running(turn));
    assert!(!controls.guidance(TurnId::new(), RequestId::new(), true));
    controls.finish_operation(turn);
}

#[test]
fn immediate_guidance_interrupts_next_batch() {
    let controls = Controls::default();
    let turn = TurnId::new();
    controls.register(turn, SessionId::new(), CancellationToken::new());
    assert!(controls.guidance(turn, RequestId::new(), true));
    let batch = controls.tool_token(turn).unwrap();
    assert!(batch.is_cancelled());
    let operation = controls.begin_operation(turn).unwrap();
    assert!(operation.is_cancelled());
    controls.finish_operation(turn);
    controls.settle_tools(turn);
    let next = controls.tool_token(turn).unwrap();
    assert!(!next.is_cancelled());
    controls.settle_tools(turn);
    assert!(controls.running(turn));
}

#[test]
fn running_batch_absorbs_immediate_guidance() {
    let controls = Controls::default();
    let turn = TurnId::new();
    controls.register(turn, SessionId::new(), CancellationToken::new());
    let batch = controls.tool_token(turn).unwrap();
    assert!(controls.guidance(turn, RequestId::new(), true));
    assert!(batch.is_cancelled());
    controls.settle_tools(turn);
    assert!(!controls.tool_token(turn).unwrap().is_cancelled());
}

#[test]
fn delivered_guidance_clears_interruption() {
    let controls = Controls::default();
    let turn = TurnId::new();
    controls.register(turn, SessionId::new(), CancellationToken::new());
    assert!(controls.guidance(turn, RequestId::new(), true));
    controls.steering(turn);
    assert!(!controls.tool_token(turn).unwrap().is_cancelled());
}
