//! The JSON files the digest keeps, written whole or not at all, and the shared formats of
//! their values: Python-compatible ISO timestamps, so files written before the Rust port
//! compare and parse the same.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use jiff::{Timestamp, civil};
use serde_json::{Map, Value};

use crate::shown;

/// The user's home directory, from `HOME`.
pub fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

/// `~` and `~/…` expanded to the user's home, as Python's `expanduser` does for them.
pub fn expand_user(path: &str) -> PathBuf {
    if path == "~" {
        home()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home().join(rest)
    } else {
        PathBuf::from(path)
    }
}

/// The JSON object in `path`, or `None` when the file does not exist.
pub fn read(path: &Path) -> Result<Option<Map<String, Value>>> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text)? {
            Value::Object(map) => Ok(Some(map)),
            _ => Err(shown!("{} must hold a JSON object", path.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// The JSON object in `path`, or an empty one when the file does not exist.
pub fn read_or_empty(path: &Path) -> Result<Map<String, Value>> {
    Ok(read(path)?.unwrap_or_default())
}

/// Write `value` to `path` through a private temporary file beside it, then rename it over
/// `path`: a reader sees the old file or the new one, never a part.
pub fn save(path: &Path, value: &Value) -> Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(serde_json::to_string(value)?.as_bytes())?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// Remove `path`, unless it is already gone.
pub fn remove(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Python's truthiness of a JSON value, as `bool(value)` reads it.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Whether `value` is a JSON integer, as Python's `type(value) is int` reads it.
pub fn integer(value: Option<&Value>) -> Option<i64> {
    value.and_then(Value::as_i64)
}

/// A timestamp to microseconds, the precision of the digest's files.
pub fn micros(at: Timestamp) -> Timestamp {
    Timestamp::from_microsecond(at.as_microsecond()).unwrap_or(at)
}

/// `at` in UTC as Python's `isoformat` writes it: `2026-11-05T17:00:12.402311+00:00`, without
/// the fraction when it is zero.
pub fn utc_iso(at: Timestamp) -> String {
    format!("{}+00:00", civil_iso(jiff::tz::Offset::UTC.to_datetime(at)))
}

/// A civil date and time as Python's `isoformat` writes a naive one.
pub fn civil_iso(at: civil::DateTime) -> String {
    let micro = at.subsec_nanosecond() / 1000;
    let base = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        at.year(),
        at.month(),
        at.day(),
        at.hour(),
        at.minute(),
        at.second()
    );
    if micro == 0 {
        base
    } else {
        format!("{base}.{micro:06}")
    }
}

/// An ISO timestamp with its offset (`Z` or `±HH:MM`), as Python's `fromisoformat` takes it.
pub fn parse_utc(value: &str) -> Result<Timestamp> {
    if let Ok(at) = value.parse::<Timestamp>() {
        return Ok(at);
    }
    if value.parse::<civil::DateTime>().is_ok() {
        return Err(shown!("Source timestamp must include its timezone"));
    }
    Err(shown!("Invalid isoformat string: '{value}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_timestamps_as_python_does() {
        let at: Timestamp = "2026-11-05T17:00:12.402311Z".parse().unwrap();
        assert_eq!(utc_iso(at), "2026-11-05T17:00:12.402311+00:00");
        let whole: Timestamp = "2026-10-29T18:00:00Z".parse().unwrap();
        assert_eq!(utc_iso(whole), "2026-10-29T18:00:00+00:00");
        assert_eq!(
            parse_utc("2026-10-30T17:00:00+02:00").unwrap(),
            "2026-10-30T15:00:00Z".parse::<Timestamp>().unwrap()
        );
        assert!(parse_utc("2026-10-30T17:00:00").is_err());
    }
}
