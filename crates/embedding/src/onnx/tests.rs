use super::*;

#[test]
fn preserves_encoder_contracts() {
    for (scenario, check) in [
        (
            "selects_cls_without_other_tokens",
            (|scenario: &'static str| {
                assert_eq!(
                    pool_output(&[1, 2, 2], &[3., 4., 8., 9.], &[1, 1], 2, Pooling::Cls).unwrap(),
                    [3., 4.],
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "rejects_mismatched_outputs",
            (|scenario: &'static str| {
                for (shape, values, tokens, dimension) in [
                    (vec![1, 2, 2], vec![0.; 3], 2, 2),
                    (vec![2, 1, 2], vec![0.; 4], 1, 2),
                    (vec![1, 1, 3], vec![0.; 3], 1, 2),
                    (vec![1, 0, 2], vec![], 0, 2),
                    (vec![1, 1, 0], vec![], 1, 0),
                ] {
                    assert!(
                        matches!(
                            pool_output(&shape, &values, &vec![1; tokens], dimension, Pooling::Cls),
                            Err(Error::Invalid(_))
                        ),
                        "{scenario}"
                    );
                }
            }),
        ),
        (
            "excludes_masked_rows_from_mean",
            (|scenario: &'static str| {
                let vector = pool_output(
                    &[1, 3, 2],
                    &[2., 4., 100., 200., 4., 8.],
                    &[1, 0, 1],
                    2,
                    Pooling::Mean,
                )
                .unwrap();
                assert_eq!(vector, [3., 6.], "{scenario}");
                assert_eq!(
                    pool_output(&[1, 2, 2], &[1., 2., 3., 4.], &[0, 0], 2, Pooling::Mean).unwrap(),
                    [0., 0.],
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
