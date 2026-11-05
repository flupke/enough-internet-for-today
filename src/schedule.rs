//! When each zone collects: its slots, in local time.

use anyhow::Result;
use jiff::{ToSpan, civil};
use serde_json::{Map, Value};

use crate::files::integer;
use crate::shown;
use crate::zone::Zone;

/// The morning slot of Email and Social when the config sets no `morning`.
const MORNING: &str = "08:00";
/// Slack's interval when the config sets no `slack_every_minutes`.
const SLACK_EVERY: i64 = 30;

/// The zone's daily slots: `morning` for Email and Social, every `slack_every_minutes` from
/// midnight for Slack.
pub fn times(zone: Zone, config: &Map<String, Value>) -> Result<Vec<civil::Time>> {
    match zone {
        Zone::Email | Zone::Social => {
            let morning = match config.get("morning") {
                None => MORNING,
                Some(Value::String(morning)) => morning,
                Some(_) => return Err(shown!("morning must be a HH:MM time")),
            };
            let time = morning
                .parse::<civil::Time>()
                .map_err(|_| shown!("morning must be a HH:MM time"))?;
            Ok(vec![time])
        }
        Zone::Slack => {
            let every = match config.get("slack_every_minutes") {
                None => SLACK_EVERY,
                Some(value) => integer(Some(value))
                    .filter(|m| (5..=1440).contains(m) && 1440 % m == 0)
                    .ok_or_else(|| {
                        shown!("slack_every_minutes must divide a day, from 5 to 1440")
                    })?,
            };
            Ok((0..1440 / every)
                .map(|n| civil::Time::midnight() + (n * every).minutes())
                .collect())
        }
    }
}

/// The latest slot at or before `now`, and the next one after it.
pub fn schedule(
    times: &[civil::Time],
    now: civil::DateTime,
) -> Result<(civil::DateTime, civil::DateTime)> {
    let mut slots = Vec::new();
    for days in [-1, 0, 1] {
        let date = now.date().checked_add(days.days())?;
        slots.extend(times.iter().map(|time| date.to_datetime(*time)));
    }
    let slot = slots.iter().filter(|s| **s <= now).max();
    let next = slots.iter().filter(|s| **s > now).min();
    match (slot, next) {
        (Some(slot), Some(next)) => Ok((*slot, *next)),
        _ => Err(shown!("A zone has no collection time")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{date, time};
    use serde_json::json;

    fn config(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn mornings_roll_over_midnight() {
        let times = times(Zone::Social, &Map::new()).unwrap();
        let (slot, next) = schedule(&times, date(2026, 10, 30).at(7, 0, 0, 0)).unwrap();
        assert_eq!(slot, date(2026, 10, 29).at(8, 0, 0, 0));
        assert_eq!(next, date(2026, 10, 30).at(8, 0, 0, 0));
    }

    #[test]
    fn slack_runs_every_half_hour_by_default() {
        let times = times(Zone::Slack, &Map::new()).unwrap();
        let (slot, next) = schedule(&times, date(2026, 10, 30).at(23, 45, 0, 0)).unwrap();
        assert_eq!(slot, date(2026, 10, 30).at(23, 30, 0, 0));
        assert_eq!(next, date(2026, 10, 31).at(0, 0, 0, 0));
        let hourly = super::times(Zone::Slack, &config(json!({"slack_every_minutes": 60})));
        assert_eq!(hourly.unwrap()[1], time(1, 0, 0, 0));
    }

    #[test]
    fn rejects_bad_schedules() {
        for bad in [
            json!({"morning": "nine"}),
            json!({"morning": ["08:00"]}),
            json!({"slack_every_minutes": 7}),
            json!({"slack_every_minutes": 0}),
        ] {
            let zone = if bad.get("morning").is_some() {
                Zone::Email
            } else {
                Zone::Slack
            };
            assert!(times(zone, &config(bad.clone())).is_err(), "{bad}");
        }
    }
}
