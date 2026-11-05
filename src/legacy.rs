//! The digest's files from before the zones, moved into the zones' files: `latest.json`, which
//! held one digest across X, Slack and email with their cursors, `status.json` and
//! `source-starts.json`. Each zone keeps its cursor and its items, so the first run after an
//! update fetches nothing twice.

use std::path::Path;

use anyhow::Result;
use jiff::tz::TimeZone;
use serde_json::{Map, Value, json};

use crate::files::{parse_utc, read, remove, save};
use crate::zone::Zone;

/// Move the old files under `data` into the zones' files, unless a zone already has its own;
/// then remove them.
pub fn migrate(data: &Path, tz: &TimeZone) -> Result<()> {
    let latest = read(&data.join("latest.json"))?;
    let starts = read(&data.join("source-starts.json"))?;
    if latest.is_none() && starts.is_none() {
        return Ok(());
    }
    let latest = latest.unwrap_or_default();
    let starts = starts.unwrap_or_default();
    for zone in Zone::ALL {
        let path = zone.file(data);
        if !path.exists() {
            save(
                &path,
                &Value::Object(zone_state(zone, &latest, &starts, tz)),
            )?;
        }
    }
    migrate_pending(data, &latest)?;
    for old in ["latest.json", "status.json", "source-starts.json"] {
        remove(&data.join(old))?;
    }
    Ok(())
}

/// An old pending batch was kept for the digest it followed: it follows the X cursor now. An
/// empty one, or one for another digest, goes.
fn migrate_pending(data: &Path, latest: &Map<String, Value>) -> Result<()> {
    let path = data.join("pending.json");
    let Some(pending) = read(&path)? else {
        return Ok(());
    };
    if !pending.contains_key("previous_id") {
        return Ok(());
    }
    let posts = pending.get("posts").and_then(Value::as_array);
    if posts.is_some_and(|p| !p.is_empty())
        && pending.get("previous_id") == Some(latest.get("id").unwrap_or(&Value::Null))
    {
        let cursor = latest.get("cursor").cloned().unwrap_or(Value::Null);
        save(
            &path,
            &json!({"since": cursor, "posts": posts, "sampled": pending.get("sampled")}),
        )
    } else {
        remove(&path)
    }
}

/// The zone's share of the old digest: its items, its cursor, and the slot it was collected for.
fn zone_state(
    zone: Zone,
    latest: &Map<String, Value>,
    starts: &Map<String, Value>,
    tz: &TimeZone,
) -> Map<String, Value> {
    let collected = latest.get("id").cloned().unwrap_or(Value::Null);
    let prefix = match zone {
        Zone::Slack => "Slack · ",
        Zone::Email => "Email · ",
        Zone::Social => "X · ",
    };
    let items: Vec<Value> = latest
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| {
            item["source"]
                .as_str()
                .is_some_and(|s| s.starts_with(prefix))
        })
        .map(|item| {
            let mut item = item.clone();
            item["at"] = collected.clone();
            item
        })
        .collect();
    let mut state = Map::new();
    state.insert("items".into(), json!(items));
    let cursor = match zone.gathered() {
        None => latest.get("cursor"),
        Some(kind) => latest
            .get("watermarks")
            .and_then(|w| w.get(kind))
            .or_else(|| starts.get(kind)),
    };
    if let Some(cursor) = cursor.filter(|c| !c.is_null()) {
        state.insert("cursor".into(), cursor.clone());
    }
    if zone == Zone::Slack
        && let Some(scope) = latest
            .get("slack_scope")
            .or_else(|| starts.get("slack_scope"))
    {
        state.insert("slack_scope".into(), scope.clone());
    }
    if let Some(slot) = latest.get("slot") {
        state.insert("slot".into(), slot.clone());
    }
    if let Some(at) = collected.as_str().and_then(|at| parse_utc(at).ok()) {
        let refreshed = at.to_zoned(tz.clone()).strftime("%a %H:%M").to_string();
        state.insert("refreshed".into(), json!(refreshed));
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::read_or_empty;

    #[test]
    fn an_old_digest_moves_into_the_zones_with_its_cursors() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path();
        let latest: Value =
            serde_json::from_str(include_str!("../tests/fixtures/legacy/latest.json")).unwrap();
        save(&data.join("latest.json"), &latest).unwrap();
        save(&data.join("status.json"), &json!({"error": ""})).unwrap();
        save(
            &data.join("pending.json"),
            &json!({"previous_id": latest["id"], "posts": [{"id": "9", "author": "a", "text": "t"}], "sampled": false}),
        )
        .unwrap();
        migrate(data, &TimeZone::UTC).unwrap();

        let zone = |zone: Zone| read_or_empty(&zone.file(data)).unwrap();
        let social = zone(Zone::Social);
        assert_eq!(social["cursor"], "1986123456789012345");
        assert_eq!(
            social["items"][0]["title"],
            "Fixture: a release with a useful new capability"
        );
        assert_eq!(social["items"][0]["at"], "2026-11-05T17:00:12.402311+00:00");
        assert_eq!(social["slot"], "2026-11-05T18:00:00");
        assert_eq!(social["refreshed"], "Thu 17:00");
        let email = zone(Zone::Email);
        assert_eq!(email["cursor"], "2026-11-05T17:00:12.402311+00:00");
        assert_eq!(email["items"].as_array().unwrap().len(), 1);
        let slack = zone(Zone::Slack);
        assert_eq!(slack["cursor"], "2026-11-05T17:00:12.402311+00:00");
        assert_eq!(slack["slack_scope"], "public");
        assert_eq!(slack["items"], json!([]));
        assert_eq!(
            read_or_empty(&data.join("pending.json")).unwrap()["since"],
            "1986123456789012345"
        );
        for old in ["latest.json", "status.json", "source-starts.json"] {
            assert!(!data.join(old).exists(), "{old}");
        }
    }

    #[test]
    fn an_empty_old_batch_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path();
        let latest: Value =
            serde_json::from_str(include_str!("../tests/fixtures/legacy/latest.json")).unwrap();
        save(&data.join("latest.json"), &latest).unwrap();
        save(
            &data.join("pending.json"),
            &json!({"previous_id": latest["id"], "posts": [], "sampled": false}),
        )
        .unwrap();
        migrate(data, &TimeZone::UTC).unwrap();
        assert!(!data.join("pending.json").exists());
    }

    #[test]
    fn a_first_window_survives_the_move() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path();
        let start = "2026-10-29T18:00:00+00:00";
        save(
            &data.join("source-starts.json"),
            &json!({"Slack": start, "Email": start, "slack_scope": "all"}),
        )
        .unwrap();
        // The batch of a first selection that failed, before any digest.
        save(
            &data.join("pending.json"),
            &json!({"previous_id": null, "posts": [{"id": "9", "author": "a", "text": "t"}], "sampled": false}),
        )
        .unwrap();
        migrate(data, &TimeZone::UTC).unwrap();
        let pending = read_or_empty(&data.join("pending.json")).unwrap();
        assert_eq!(pending["since"], Value::Null);
        assert_eq!(pending["posts"][0]["id"], "9");
        let slack = read_or_empty(&Zone::Slack.file(data)).unwrap();
        assert_eq!(slack["cursor"], start);
        assert_eq!(slack["slack_scope"], "all");
        assert!(
            !read_or_empty(&Zone::Social.file(data))
                .unwrap()
                .contains_key("cursor")
        );
    }
}
