use super::*;

#[test]
fn preserves_control_boundaries() {
    let (sender, messages) = mpsc::sync_channel(QUEUE_CAPACITY);
    sender.send(Message::Bytes(vec![0xb8])).unwrap();
    sender
        .send(Message::Bytes(vec![0xad, b'\r', b'\n']))
        .unwrap();
    let terminal = TerminalId::new();
    let (reply, response) = oneshot::channel();
    sender
        .send(Message::Command {
            caller: NodeId([1; 32]),
            command: Box::new(Command::InspectTerminal { terminal }),
            reply,
        })
        .unwrap();
    sender.send(Message::Bytes(b"after".to_vec())).unwrap();
    sender.send(Message::Eof(Ok(()))).unwrap();

    let mut pending = None;
    assert_eq!(
        coalesce(vec![0xe4], &messages, &mut pending),
        "中\r\n".as_bytes()
    );
    let Some(Message::Command { command, reply, .. }) = pending.take() else {
        panic!("queued control command expected");
    };
    assert!(matches!(*command, Command::InspectTerminal { terminal: id } if id == terminal));
    reply.send(Ok(Output::TerminalInput { terminal })).unwrap();
    assert_eq!(
        response.blocking_recv().unwrap().unwrap(),
        Output::TerminalInput { terminal }
    );
    let Message::Bytes(bytes) = messages.try_recv().unwrap() else {
        panic!("output after control command expected");
    };
    assert_eq!(coalesce(bytes, &messages, &mut pending), b"after");
    assert!(matches!(pending, Some(Message::Eof(Ok(())))));
    assert!(matches!(
        messages.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn bounds_ready_output() {
    let (sender, messages) = mpsc::sync_channel(QUEUE_CAPACITY);
    sender.send(Message::Bytes(vec![2; 16 * 1024])).unwrap();
    sender.send(Message::Bytes(vec![3; 16 * 1024])).unwrap();
    sender.send(Message::Bytes(vec![4; 16 * 1024])).unwrap();
    sender.send(Message::Bytes(vec![5; 16 * 1024])).unwrap();
    let mut pending = None;
    let bytes = coalesce(vec![1; 16 * 1024], &messages, &mut pending);
    assert_eq!(bytes.len(), MAX_BATCH_BYTES);
    assert!(pending.is_none());
    for (index, chunk) in bytes.chunks(16 * 1024).enumerate() {
        assert!(chunk.iter().all(|byte| *byte == index as u8 + 1));
    }
    let Message::Bytes(bytes) = messages.try_recv().unwrap() else {
        panic!("unconsumed output expected");
    };
    assert_eq!(bytes, vec![5; 16 * 1024]);

    sender.send(Message::Bytes(vec![6; 16 * 1024])).unwrap();
    let bytes = coalesce(vec![5; MAX_BATCH_BYTES - 1], &messages, &mut pending);
    assert_eq!(bytes.len(), MAX_BATCH_BYTES - 1);
    assert!(matches!(pending.take(), Some(Message::Bytes(bytes)) if bytes == vec![6; 16 * 1024]));
}

#[test]
fn publishes_without_waiting_for_more() {
    let (_sender, messages) = mpsc::sync_channel(QUEUE_CAPACITY);
    let mut pending = None;
    assert_eq!(
        coalesce(b"ready".to_vec(), &messages, &mut pending),
        b"ready"
    );
    assert!(pending.is_none());
}
