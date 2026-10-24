use super::*;

const HOUR: i64 = 3_600_000;

fn local(offset_minutes: i32) -> Query {
    let offset = i64::from(offset_minutes) * 60_000;
    Query {
        start_ms: START - offset,
        end_ms: START - offset + 3 * DAY,
        utc_offset_minutes: offset_minutes,
        ..query()
    }
}

#[tokio::test]
async fn starts_days_at_local_midnight() {
    let mut fixture = Fixture::new().await;
    // Local UTC+8 midnight is 16:00 UTC on the previous day.
    fixture
        .append([
            event(START - 8 * HOUR + 1, 1),
            event(START + 16 * HOUR - 1, 2),
            event(START + 16 * HOUR, 4),
        ])
        .await;
    let east = report(&fixture, &local(480)).await;
    assert_eq!(east.days.len(), 3);
    assert_eq!(east.days[0].start_ms, START - 8 * HOUR);
    assert_eq!(east.days[0].metrics.tokens.as_ref().unwrap().input, 3);
    assert_eq!(east.days[1].metrics.tokens.as_ref().unwrap().input, 4);
    assert_eq!(east.days[2].metrics.responses, 0);
    let utc = report(
        &fixture,
        &Query {
            start_ms: START - 8 * HOUR,
            end_ms: START - 8 * HOUR + 3 * DAY,
            ..query()
        },
    )
    .await;
    assert_eq!(utc.days[0].start_ms, START - DAY);
    assert_eq!(utc.days[0].metrics.tokens.as_ref().unwrap().input, 1);
    assert_eq!(utc.days[1].metrics.tokens.as_ref().unwrap().input, 6);
}

#[tokio::test]
async fn keeps_western_evenings_on_the_prior_day() {
    let mut fixture = Fixture::new().await;
    // 04:00 UTC is 23:00 on the previous local day at UTC-5.
    fixture
        .append([event(START + 4 * HOUR, 7), event(START + 5 * HOUR, 9)])
        .await;
    let west = report(&fixture, &local(-300)).await;
    assert_eq!(west.days[0].start_ms, START + 5 * HOUR);
    assert_eq!(west.days[0].metrics.tokens.as_ref().unwrap().input, 9);
    assert_eq!(west.totals.responses, 1);
    let mut wider = local(-300);
    wider.start_ms -= DAY;
    let report = report(&fixture, &wider).await;
    assert_eq!(report.days[0].metrics.tokens.as_ref().unwrap().input, 7);
    assert_eq!(report.days[1].metrics.tokens.as_ref().unwrap().input, 9);
}

#[tokio::test]
async fn aligns_recent_buckets_to_quarter_hour_offsets() {
    let fixture = Fixture::new().await;
    let report = report(&fixture, &local(345)).await;
    assert!(
        report
            .recent
            .iter()
            .all(|point| (point.start_ms + 345 * 60_000).rem_euclid(HALF_HOUR) == 0)
    );
}

#[tokio::test]
async fn rejects_offsets_beyond_time_zones() {
    let fixture = Fixture::new().await;
    for offset in [-841, 841] {
        assert!(
            read(
                &fixture.database.connection,
                fixture.database.node,
                &local(offset),
            )
            .await
            .is_err()
        );
    }
}
