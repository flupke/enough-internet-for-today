//! The daily digest times.

use anyhow::Result;
use jiff::{ToSpan, civil};
use serde_json::Value;

use crate::shown;

/// The configured times, `["09:00", "18:00"]` when the config names none.
pub fn times(config: Option<&Value>) -> Result<Vec<String>> {
    match config {
        None => Ok(vec!["09:00".into(), "18:00".into()]),
        Some(Value::Array(values)) => values
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| shown!("times must be a list of HH:MM times"))
            })
            .collect(),
        Some(_) => Err(shown!("times must be a list of HH:MM times")),
    }
}

/// The latest digest slot at or before `now`, and the next one after it, in local time.
pub fn schedule(
    times: &[String],
    now: civil::DateTime,
) -> Result<(civil::DateTime, civil::DateTime)> {
    if times.is_empty() {
        return Err(shown!("Configure at least one digest time"));
    }
    let clocks = times
        .iter()
        .map(|t| {
            t.parse::<civil::Time>()
                .map_err(|_| shown!("Invalid isoformat string: '{t}'"))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut slots = Vec::new();
    for days in [-1, 0, 1] {
        let date = now.date().checked_add(days.days())?;
        slots.extend(clocks.iter().map(|clock| date.to_datetime(*clock)));
    }
    let slot = slots.iter().filter(|s| **s <= now).max();
    let next = slots.iter().filter(|s| **s > now).min();
    match (slot, next) {
        (Some(slot), Some(next)) => Ok((*slot, *next)),
        _ => Err(shown!("Configure at least one digest time")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn schedule_rolls_over_midnight() {
        let times = ["09:00".to_owned(), "18:00".to_owned()];
        let (slot, following) = schedule(&times, date(2026, 10, 30).at(7, 0, 0, 0)).unwrap();
        assert_eq!(slot, date(2026, 10, 29).at(18, 0, 0, 0));
        assert_eq!(following, date(2026, 10, 30).at(9, 0, 0, 0));
    }

    #[test]
    fn rejects_no_times_and_bad_times() {
        let now = date(2026, 10, 30).at(7, 0, 0, 0);
        assert!(schedule(&[], now).is_err());
        assert!(schedule(&["nine".to_owned()], now).is_err());
        assert!(times(Some(&Value::String("09:00".into()))).is_err());
    }
}
