use super::*;

fn at(value: &str) -> i64 {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .timestamp_millis()
}

#[test]
fn preserves_storage_contracts() {
    for (scenario, check) in [
        (
            "skips_weekends_and_consumed_times",
            (|scenario: &'static str| {
                let calendar = Calendar::new("Asia/Shanghai", &[1, 2, 3, 4, 5], 9, 0).unwrap();
                let friday = at("2026-10-16T01:00:00Z");
                let monday = at("2026-10-19T01:00:00Z");
                assert_eq!(
                    calendar.next(friday - 1).unwrap(),
                    Some(friday),
                    "{scenario}"
                );
                assert_eq!(calendar.next(friday).unwrap(), Some(monday), "{scenario}");
                assert_eq!(
                    calendar.next(friday + 1).unwrap(),
                    Some(monday),
                    "{scenario}"
                );
                assert_eq!(
                    calendar.due(friday, at("2026-10-18T15:00:00Z")).unwrap(),
                    (friday, Some(monday)),
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "uses_local_weekdays_and_fractional_offsets",
            (|scenario: &'static str| {
                let calendar = Calendar::new("Asia/Kolkata", &[1], 0, 15).unwrap();
                assert_eq!(
                    calendar.next(at("2026-10-18T18:30:00Z")).unwrap(),
                    Some(at("2026-10-18T18:45:00Z")),
                    "{scenario}"
                );
            }),
        ),
        (
            "preserves_wall_time_across_offset_changes",
            (|scenario: &'static str| {
                let calendar =
                    Calendar::new("Europe/London", &[1, 2, 3, 4, 5, 6, 7], 9, 0).unwrap();
                assert_eq!(
                    calendar.next(at("2024-03-30T09:00:00Z")).unwrap(),
                    Some(at("2024-03-31T08:00:00Z")),
                    "{scenario}"
                );
                assert_eq!(
                    calendar.next(at("2024-10-26T08:00:00Z")).unwrap(),
                    Some(at("2024-10-27T09:00:00Z")),
                    "{scenario}"
                );
            }),
        ),
        (
            "skips_nonexistent_occurrences",
            (|scenario: &'static str| {
                let calendar = Calendar::new("Europe/London", &[7], 1, 30).unwrap();
                assert_eq!(
                    calendar.next(at("2024-03-31T00:00:00Z")).unwrap(),
                    Some(at("2024-04-07T00:30:00Z")),
                    "{scenario}"
                );
            }),
        ),
        (
            "consumes_ambiguous_occurrences_once",
            (|scenario: &'static str| {
                let calendar = Calendar::new("Europe/London", &[7], 1, 30).unwrap();
                let earlier = at("2024-10-27T00:30:00Z");
                let following = at("2024-11-03T01:30:00Z");
                assert_eq!(
                    calendar.next(at("2024-10-27T00:00:00Z")).unwrap(),
                    Some(earlier),
                    "{scenario}"
                );
                for observed in [at("2024-10-27T00:45:00Z"), at("2024-10-27T01:45:00Z")] {
                    assert_eq!(
                        calendar.due(earlier, observed).unwrap(),
                        (earlier, Some(following)),
                        "{scenario}"
                    );
                    assert_eq!(
                        calendar.next(observed).unwrap(),
                        Some(following),
                        "{scenario}"
                    );
                }
            }),
        ),
        (
            "coalesces_long_downtime",
            (|scenario: &'static str| {
                let calendar = Calendar::new("Asia/Shanghai", &[1, 2, 3, 4, 5], 9, 0).unwrap();
                assert_eq!(
                    calendar
                        .due(at("2020-01-06T01:00:00Z"), at("2026-10-21T03:00:00Z"))
                        .unwrap(),
                    (at("2026-10-21T01:00:00Z"), Some(at("2026-10-22T01:00:00Z"))),
                    "{scenario}"
                );
            }),
        ),
        (
            "rejects_invalid_definitions",
            (|scenario: &'static str| {
                assert!(
                    Calendar::new("Mars/Olympus", &[1], 9, 0).is_err(),
                    "{scenario}"
                );
                for days in [vec![], vec![0], vec![8], vec![1, 1]] {
                    assert!(Calendar::new("UTC", &days, 9, 0).is_err(), "{scenario}");
                }
                assert!(Calendar::new("UTC", &[1], 24, 0).is_err(), "{scenario}");
                assert!(Calendar::new("UTC", &[1], 9, 60).is_err(), "{scenario}");
            }),
        ),
        (
            "handles_calendar_bounds",
            (|scenario: &'static str| {
                let calendar = Calendar::new("UTC", &[1, 2, 3, 4, 5, 6, 7], 23, 59).unwrap();
                assert!(calendar.next(i64::MAX).is_err(), "{scenario}");
                assert_eq!(
                    calendar
                        .next(DateTime::<Utc>::MAX_UTC.timestamp_millis())
                        .unwrap(),
                    None,
                    "{scenario}"
                );
                let positive = Calendar::new("Asia/Shanghai", &[1], 9, 0).unwrap();
                assert!(
                    positive
                        .next(DateTime::<Utc>::MAX_UTC.timestamp_millis())
                        .is_err(),
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
