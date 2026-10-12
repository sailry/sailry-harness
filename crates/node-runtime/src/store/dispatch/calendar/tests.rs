use super::*;

fn at(value: &str) -> i64 {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .timestamp_millis()
}

#[test]
fn skips_weekends_and_consumed_times() {
    let calendar = Calendar::new("Asia/Shanghai", &[1, 2, 3, 4, 5], 9, 0).unwrap();
    let friday = at("2026-10-16T01:00:00Z");
    let monday = at("2026-10-19T01:00:00Z");
    assert_eq!(calendar.next(friday - 1).unwrap(), Some(friday));
    assert_eq!(calendar.next(friday).unwrap(), Some(monday));
    assert_eq!(calendar.next(friday + 1).unwrap(), Some(monday));
    assert_eq!(
        calendar.due(friday, at("2026-10-18T15:00:00Z")).unwrap(),
        (friday, Some(monday))
    );
}

#[test]
fn uses_local_weekdays_and_fractional_offsets() {
    let calendar = Calendar::new("Asia/Kolkata", &[1], 0, 15).unwrap();
    assert_eq!(
        calendar.next(at("2026-10-18T18:30:00Z")).unwrap(),
        Some(at("2026-10-18T18:45:00Z"))
    );
}

#[test]
fn preserves_wall_time_across_offset_changes() {
    let calendar = Calendar::new("Europe/London", &[1, 2, 3, 4, 5, 6, 7], 9, 0).unwrap();
    assert_eq!(
        calendar.next(at("2024-03-30T09:00:00Z")).unwrap(),
        Some(at("2024-03-31T08:00:00Z"))
    );
    assert_eq!(
        calendar.next(at("2024-10-26T08:00:00Z")).unwrap(),
        Some(at("2024-10-27T09:00:00Z"))
    );
}

#[test]
fn skips_nonexistent_occurrences() {
    let calendar = Calendar::new("Europe/London", &[7], 1, 30).unwrap();
    assert_eq!(
        calendar.next(at("2024-03-31T00:00:00Z")).unwrap(),
        Some(at("2024-04-07T00:30:00Z"))
    );
}

#[test]
fn consumes_ambiguous_occurrences_once() {
    let calendar = Calendar::new("Europe/London", &[7], 1, 30).unwrap();
    let earlier = at("2024-10-27T00:30:00Z");
    let following = at("2024-11-03T01:30:00Z");
    assert_eq!(
        calendar.next(at("2024-10-27T00:00:00Z")).unwrap(),
        Some(earlier)
    );
    for observed in [at("2024-10-27T00:45:00Z"), at("2024-10-27T01:45:00Z")] {
        assert_eq!(
            calendar.due(earlier, observed).unwrap(),
            (earlier, Some(following))
        );
        assert_eq!(calendar.next(observed).unwrap(), Some(following));
    }
}

#[test]
fn coalesces_long_downtime() {
    let calendar = Calendar::new("Asia/Shanghai", &[1, 2, 3, 4, 5], 9, 0).unwrap();
    assert_eq!(
        calendar
            .due(at("2020-01-06T01:00:00Z"), at("2026-10-21T03:00:00Z"))
            .unwrap(),
        (at("2026-10-21T01:00:00Z"), Some(at("2026-10-22T01:00:00Z")))
    );
}

#[test]
fn rejects_invalid_definitions() {
    assert!(Calendar::new("Mars/Olympus", &[1], 9, 0).is_err());
    for days in [vec![], vec![0], vec![8], vec![1, 1]] {
        assert!(Calendar::new("UTC", &days, 9, 0).is_err());
    }
    assert!(Calendar::new("UTC", &[1], 24, 0).is_err());
    assert!(Calendar::new("UTC", &[1], 9, 60).is_err());
}

#[test]
fn handles_calendar_bounds() {
    let calendar = Calendar::new("UTC", &[1, 2, 3, 4, 5, 6, 7], 23, 59).unwrap();
    assert!(calendar.next(i64::MAX).is_err());
    assert_eq!(
        calendar
            .next(DateTime::<Utc>::MAX_UTC.timestamp_millis())
            .unwrap(),
        None
    );
    let positive = Calendar::new("Asia/Shanghai", &[1], 9, 0).unwrap();
    assert!(
        positive
            .next(DateTime::<Utc>::MAX_UTC.timestamp_millis())
            .is_err()
    );
}
