use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Offset, TimeZone, Utc};
use chrono_tz::Tz;
use sailry_protocol::Fault;
use std::collections::BTreeSet;

pub(super) struct Calendar {
    timezone: Tz,
    weekdays: BTreeSet<u8>,
    time: NaiveTime,
}

impl Calendar {
    pub(super) fn new(
        timezone: &str,
        weekdays: &[u8],
        hour: u8,
        minute: u8,
    ) -> Result<Self, Fault> {
        let timezone = timezone
            .parse::<Tz>()
            .map_err(|_| Fault::invalid("schedule timezone is invalid"))?;
        let days = weekdays.iter().copied().collect::<BTreeSet<_>>();
        if days.is_empty()
            || days.len() != weekdays.len()
            || days.iter().any(|day| !(1..=7).contains(day))
        {
            return Err(Fault::invalid("schedule weekdays are invalid"));
        }
        let time = NaiveTime::from_hms_opt(hour.into(), minute.into(), 0)
            .ok_or_else(|| Fault::invalid("schedule local time is invalid"))?;
        Ok(Self {
            timezone,
            weekdays: days,
            time,
        })
    }

    fn date(&self, timestamp: i64) -> Result<NaiveDate, Fault> {
        DateTime::<Utc>::from_timestamp_millis(timestamp)
            .and_then(|time| {
                let local = time.with_timezone(&self.timezone);
                local.naive_utc().checked_add_offset(local.offset().fix())
            })
            .map(|time| time.date())
            .ok_or_else(|| Fault::invalid("schedule time is invalid"))
    }

    fn occurrence(&self, date: NaiveDate) -> Option<i64> {
        if !self
            .weekdays
            .contains(&(date.weekday().number_from_monday() as u8))
        {
            return None;
        }
        // A repeated wall time represents one occurrence; a missing time is skipped.
        self.timezone
            .from_local_datetime(&date.and_time(self.time))
            .earliest()
            .map(|time| time.timestamp_millis())
    }

    pub(super) fn next(&self, timestamp: i64) -> Result<Option<i64>, Fault> {
        let mut date = self.date(timestamp)?;
        loop {
            if let Some(occurrence) = self.occurrence(date)
                && occurrence > timestamp
            {
                return Ok(Some(occurrence));
            }
            let Some(next) = date.succ_opt() else {
                return Ok(None);
            };
            date = next;
        }
    }

    pub(super) fn due(&self, first: i64, timestamp: i64) -> Result<(i64, Option<i64>), Fault> {
        if first > timestamp {
            return Err(Fault::invalid("schedule occurrence is not due"));
        }
        let lower = self.date(first)?;
        let mut date = self.date(timestamp)?;
        let mut latest = first;
        // Search near the observation date instead of replaying every missed week.
        while date >= lower {
            if let Some(occurrence) = self.occurrence(date)
                && occurrence <= timestamp
            {
                latest = latest.max(occurrence);
                break;
            }
            let Some(previous) = date.pred_opt() else {
                break;
            };
            date = previous;
        }
        Ok((latest, self.next(timestamp)?))
    }
}

#[cfg(test)]
mod tests;
