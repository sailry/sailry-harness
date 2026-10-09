use super::*;

#[test]
fn selects_cls_without_other_tokens() {
    assert_eq!(
        pool_output(&[1, 2, 2], &[3., 4., 8., 9.], &[1, 1], 2, Pooling::Cls).unwrap(),
        [3., 4.]
    );
}

#[test]
fn rejects_mismatched_outputs() {
    for (shape, values, tokens, dimension) in [
        (vec![1, 2, 2], vec![0.; 3], 2, 2),
        (vec![2, 1, 2], vec![0.; 4], 1, 2),
        (vec![1, 1, 3], vec![0.; 3], 1, 2),
        (vec![1, 0, 2], vec![], 0, 2),
        (vec![1, 1, 0], vec![], 1, 0),
    ] {
        assert!(matches!(
            pool_output(&shape, &values, &vec![1; tokens], dimension, Pooling::Cls),
            Err(Error::Invalid(_))
        ));
    }
}

#[test]
fn excludes_masked_rows_from_mean() {
    let vector = pool_output(
        &[1, 3, 2],
        &[2., 4., 100., 200., 4., 8.],
        &[1, 0, 1],
        2,
        Pooling::Mean,
    )
    .unwrap();
    assert_eq!(vector, [3., 6.]);
    assert_eq!(
        pool_output(&[1, 2, 2], &[1., 2., 3., 4.], &[0, 0], 2, Pooling::Mean).unwrap(),
        [0., 0.]
    );
}

#[test]
fn joins_owned_workers() {
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = Threads
        .create(move || sender.send(std::thread::current().id()).unwrap())
        .unwrap();
    Threads::join(worker).unwrap();
    assert_ne!(receiver.recv().unwrap(), std::thread::current().id());
}
