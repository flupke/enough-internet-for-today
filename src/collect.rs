//! Collect a finite X, Slack and email digest. Run from a timer; secrets are read from a local
//! file.

use std::fs::{File, TryLockError};
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use jiff::{SignedDuration, Timestamp, civil};
use serde_json::{Map, Value, json};

use crate::error::collection_message;
use crate::files::{civil_iso, expand_user, integer, micros, read_or_empty, save, truthy, utc_iso};
use crate::helper::{self, HcCommand, Progress};
use crate::schedule::{schedule, times};
use crate::shown;
use crate::sources::{MCP_KINDS, checked_result, job_input};
use crate::x::{self, Post};

const X_FAILED: &str = "X collection failed; check token access, credits and rate limits";

/// What a collection reaches out to: the clock, X and the helper.
pub trait World {
    /// Now, in UTC.
    fn now(&self) -> Timestamp;
    /// Now, in local time.
    fn local_now(&self) -> civil::DateTime;
    /// The timeline's posts since `cursor`, as [`x::timeline`] gives them.
    fn timeline(
        &mut self,
        token: &str,
        cursor: Option<&str>,
        max_pages: i64,
    ) -> Result<(Vec<Post>, bool)>;
    /// The helper's result for `payload`, as [`helper::run_job`] gives it.
    fn gather(&mut self, payload: &Value, progress: &mut Progress<'_>) -> Result<Value>;
}

/// The real clock, X and helper.
pub struct Live;

impl World for Live {
    fn now(&self) -> Timestamp {
        micros(Timestamp::now())
    }

    fn local_now(&self) -> civil::DateTime {
        let now = jiff::Zoned::now().datetime();
        now.with()
            .subsec_nanosecond(now.subsec_nanosecond() / 1000 * 1000)
            .build()
            .unwrap_or(now)
    }

    fn timeline(
        &mut self,
        token: &str,
        cursor: Option<&str>,
        max_pages: i64,
    ) -> Result<(Vec<Post>, bool)> {
        x::timeline(
            &mut x::Http::new(token.to_owned()),
            cursor,
            self.now(),
            max_pages,
        )
    }

    fn gather(&mut self, payload: &Value, progress: &mut Progress<'_>) -> Result<Value> {
        let workspace = helper::workspace(&crate::files::home());
        helper::run_job(
            payload,
            &workspace,
            &mut HcCommand::default(),
            progress,
            Duration::from_secs(540),
        )
    }
}

/// How a collection run ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Run {
    /// Another run holds the lock, or the current slot already has its digest.
    Skipped,
    Published,
    /// The previous digest stays; `status.json` says why.
    Failed,
}

/// Collect a digest into `data`, under its lock; with `due`, only when the current slot has none
/// yet.
pub fn run(config_path: &Path, data: &Path, due: bool, world: &mut dyn World) -> Result<Run> {
    std::fs::create_dir_all(data)?;
    let lock = File::create(data.join("worker.lock"))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(Run::Skipped),
        Err(TryLockError::Error(e)) => return Err(e.into()),
    }
    match collect(config_path, data, due, world) {
        Ok(true) => Ok(Run::Published),
        Ok(false) => Ok(Run::Skipped),
        Err(error) => {
            let message = collection_message(&error);
            save(&data.join("status.json"), &json!({"error": message}))?;
            let mut progress = read_or_empty(&data.join("progress.json"))?;
            progress.insert("phase".into(), json!("Needs attention"));
            save(&data.join("progress.json"), &Value::Object(progress))?;
            println!("{message}");
            Ok(Run::Failed)
        }
    }
}

/// The X posts to select from, and whether older ones were left out: the pending batch a failed
/// selection kept, so a retry does not pay X again, or a fresh fetch, kept as the new pending
/// batch.
fn x_posts(
    config: &Map<String, Value>,
    previous: &Map<String, Value>,
    pending_path: &Path,
    world: &mut dyn World,
) -> Result<(Vec<Post>, bool)> {
    let pending = read_or_empty(pending_path)?;
    let previous_id = previous.get("id").unwrap_or(&Value::Null);
    if !pending.is_empty() && pending.get("previous_id").unwrap_or(&Value::Null) == previous_id {
        let posts = pending
            .get("posts")
            .and_then(Value::as_array)
            .ok_or_else(|| shown!("Invalid pending X batch"))?
            .iter()
            .map(Post::from_value)
            .collect::<Result<_>>()?;
        return Ok((posts, truthy(pending.get("sampled"))));
    }
    let limit = match config.get("max_pages") {
        None => 3,
        Some(value) => integer(Some(value))
            .filter(|l| (1..=20).contains(l))
            .ok_or_else(|| shown!("max_pages must be between 1 and 20"))?,
    };
    let path = |key: &str, default: &str| -> Result<std::path::PathBuf> {
        match config.get(key) {
            None => Ok(expand_user(default)),
            Some(Value::String(path)) => Ok(expand_user(path)),
            Some(_) => Err(shown!("{key} must be a path")),
        }
    };
    let oauth = path("x_oauth_file", "~/.config/herdr-digest/x-oauth.json")?;
    let token = if oauth.exists() {
        crate::x_auth::access_token(&oauth)?
    } else {
        std::fs::read_to_string(path("x_token_file", "~/.config/herdr-digest/x-token")?)?
            .trim()
            .to_owned()
    };
    let cursor = previous
        .get("cursor")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty());
    let (posts, sampled) = world.timeline(&token, cursor, limit)?;
    let batch: Vec<Value> = posts.iter().map(Post::to_value).collect();
    save(
        pending_path,
        &json!({"previous_id": previous_id, "posts": batch, "sampled": sampled}),
    )?;
    Ok((posts, sampled))
}

/// The newest of the X post IDs, by number.
fn newest(ids: &[&str]) -> Result<Option<String>> {
    let mut best: Option<(u128, &str)> = None;
    for id in ids {
        let n: u128 = id
            .parse()
            .map_err(|_| shown!("invalid literal for int() with base 10: '{id}'"))?;
        if best.is_none_or(|(b, _)| n > b) {
            best = Some((n, id));
        }
    }
    Ok(best.map(|(_, id)| id.to_owned()))
}

/// One collection; `false` when `due` and the slot already has its digest.
fn collect(config_path: &Path, data: &Path, due: bool, world: &mut dyn World) -> Result<bool> {
    if !config_path.exists() {
        return Err(shown!("Set up collection in {}", config_path.display()));
    }
    let config = read_or_empty(config_path)?;
    let (slot, next_slot) = schedule(&times(config.get("times"))?, world.local_now())?;
    let slot = civil_iso(slot);
    let previous = read_or_empty(&data.join("latest.json"))?;
    if due && previous.get("slot").and_then(Value::as_str).unwrap_or("") >= slot.as_str() {
        return Ok(false);
    }
    let end_at = world.now();
    let end = utc_iso(end_at);
    let progress_path = data.join("progress.json");
    save(
        &progress_path,
        &json!({"phase": "Collecting X", "helper": null}),
    )?;
    let pending_path = data.join("pending.json");
    let (x_error, posts, sampled) = match x_posts(&config, &previous, &pending_path, world) {
        Ok((posts, sampled)) => (None, posts, sampled),
        Err(_) => (Some(X_FAILED), Vec::new(), false),
    };

    let starts_path = data.join("source-starts.json");
    let mut starts = read_or_empty(&starts_path)?;
    let first = json!(utc_iso(end_at - SignedDuration::from_hours(24)));
    let scope = if truthy(config.get("slack_include_private")) {
        "all"
    } else {
        "public"
    };
    let public = json!("public");
    if *starts.get("slack_scope").unwrap_or(&public) != scope {
        starts.insert("Slack".into(), first.clone());
    }
    starts.insert("slack_scope".into(), json!(scope));
    for kind in MCP_KINDS {
        starts.entry(kind).or_insert_with(|| first.clone());
    }
    save(&starts_path, &Value::Object(starts.clone()))?;
    let mut watermarks_before = previous
        .get("watermarks")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if *previous.get("slack_scope").unwrap_or(&public) != scope {
        watermarks_before.remove("Slack");
    }
    let mut effective: Map<String, Value> = MCP_KINDS
        .iter()
        .map(|k| ((*k).to_owned(), starts[*k].clone()))
        .collect();
    effective.extend(watermarks_before.clone());
    let payload = job_input(&posts, &effective, &config, &end)?;
    let mut progress = |address: &str, state: &Value| {
        save(
            &progress_path,
            &json!({"phase": "Gathering Slack and email / selecting digest", "helper": address, "state": state}),
        )
    };
    let result = world.gather(&payload, &mut progress)?;
    let (items, statuses) = checked_result(&result, &payload)?;
    if x_error.is_some() && statuses.values().all(|s| s["status"] == "error") {
        return Err(shown!("All sources failed; previous digest preserved"));
    }

    let mut watermarks = watermarks_before;
    let mut errors: Vec<String> = x_error.iter().map(|e| (*e).to_owned()).collect();
    let mut summaries = vec![format!(
        "X · {}",
        x_error.map_or_else(
            || format!(
                "{} new posts{}",
                posts.len(),
                if sampled {
                    " · newest batch only; older posts omitted"
                } else {
                    ""
                }
            ),
            str::to_owned,
        )
    )];
    for (kind, status) in &statuses {
        if status["status"] == "ok" {
            watermarks.insert(kind.clone(), json!(end));
            summaries.push(format!(
                "{kind} · {} inspected{}",
                status["count"],
                if status["sampled"] == true {
                    " · bounded sample"
                } else {
                    ""
                }
            ));
        } else {
            summaries.push(format!("{kind} · failed"));
            errors.push(format!(
                "{kind}: {}",
                status["error"].as_str().unwrap_or_default()
            ));
        }
    }
    let mut ids: Vec<&str> = posts.iter().map(|p| p.id.as_str()).collect();
    if let Some(cursor) = previous
        .get("cursor")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
    {
        ids.push(cursor);
    }
    let interests = config
        .get("interests")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let digest = json!({
        "id": end,
        "slot": slot,
        "cursor": newest(&ids)?,
        "watermarks": watermarks,
        "slack_scope": scope,
        "status": format!("{} selected across X, Slack and email", items.len()),
        "next": next_slot.strftime("%a %H:%M").to_string(),
        "items": items,
        "sources": summaries,
        "profile": !interests.trim().is_empty(),
    });
    save(&data.join("latest.json"), &digest)?;
    save(
        &data.join("status.json"),
        &json!({"error": errors.join("; ")}),
    )?;
    let helper = read_or_empty(&progress_path)?
        .get("helper")
        .cloned()
        .unwrap_or(Value::Null);
    save(
        &progress_path,
        &json!({"phase": "Ready", "helper": helper, "state": "idle"}),
    )?;
    match std::fs::remove_file(&pending_path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A world with a set clock, posts and helper result, which records the helper's jobs.
    struct Fake {
        now: Timestamp,
        posts: Result<Vec<Post>, &'static str>,
        result: Value,
        payloads: Vec<Value>,
    }

    impl World for Fake {
        fn now(&self) -> Timestamp {
            self.now
        }

        fn local_now(&self) -> civil::DateTime {
            jiff::tz::Offset::UTC.to_datetime(self.now)
        }

        fn timeline(&mut self, token: &str, _: Option<&str>, _: i64) -> Result<(Vec<Post>, bool)> {
            assert_eq!(token, "test-token");
            self.posts
                .clone()
                .map(|p| (p, false))
                .map_err(|e| shown!("{e}"))
        }

        fn gather(&mut self, payload: &Value, progress: &mut Progress<'_>) -> Result<Value> {
            self.payloads.push(payload.clone());
            progress("t99", &json!("done"))?;
            Ok(self.result.clone())
        }
    }

    struct Setup {
        _spawning: std::sync::RwLockReadGuard<'static, ()>,
        _dir: tempfile::TempDir,
        root: PathBuf,
        config: PathBuf,
        data: PathBuf,
        world: Fake,
    }

    impl Setup {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().to_owned();
            let token = root.join("token");
            std::fs::write(&token, "test-token\n").unwrap();
            let config = root.join("config.json");
            save(
                &config,
                &json!({"x_token_file": token, "x_oauth_file": root.join("oauth.json")}),
            )
            .unwrap();
            let result = json!({
                "sources": {"Slack": {"status": "ok", "count": 0, "sampled": false, "error": ""},
                            "Email": {"status": "ok", "count": 0, "sampled": false, "error": ""}},
                "candidates": [],
                "items": [{"id": "x:123", "title": "Release", "summary": "A useful release", "why": "New capability"}],
            });
            Self {
                _spawning: crate::SPAWNING
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                data: root.join("data"),
                config,
                root,
                _dir: dir,
                world: Fake {
                    now: "2026-10-30T18:00:00.402311Z".parse().unwrap(),
                    posts: Ok(vec![Post {
                        id: "123".into(),
                        author: "alice".into(),
                        text: "A useful release".into(),
                    }]),
                    result,
                    payloads: Vec::new(),
                },
            }
        }

        fn run(&mut self) -> Run {
            run(&self.config, &self.data, false, &mut self.world).unwrap()
        }

        fn read(&self, name: &str) -> Map<String, Value> {
            read_or_empty(&self.data.join(name)).unwrap()
        }
    }

    #[test]
    fn publishes_verified_links_and_independent_watermarks() {
        let mut s = Setup::new();
        assert_eq!(s.run(), Run::Published);
        let digest = s.read("latest.json");
        assert_eq!(digest["cursor"], "123");
        assert_eq!(digest["items"][0]["url"], "https://x.com/i/status/123");
        let keys: Vec<_> = digest["watermarks"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(keys, ["Slack", "Email"]);
        assert_eq!(digest["id"], "2026-10-30T18:00:00.402311+00:00");
        assert_eq!(digest["slot"], "2026-10-30T18:00:00");
        assert_eq!(digest["next"], "Sat 09:00");
        assert_eq!(
            digest["sources"],
            json!([
                "X · 1 new posts",
                "Slack · 0 inspected",
                "Email · 0 inspected"
            ])
        );
        assert_eq!(
            s.read("progress.json"),
            json!({"phase": "Ready", "helper": "t99", "state": "idle"})
                .as_object()
                .unwrap()
                .clone()
        );
        assert!(!s.data.join("pending.json").exists());
    }

    #[test]
    fn digest_has_the_fixture_shape() {
        let mut s = Setup::new();
        s.run();
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/latest.json")).unwrap();
        let digest = Value::Object(s.read("latest.json"));
        let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
        assert_eq!(keys(&digest), keys(&fixture));
        assert_eq!(keys(&digest["items"][0]), keys(&fixture["items"][0]));
    }

    #[test]
    fn failed_selection_preserves_previous_digest_and_cached_x() {
        let mut s = Setup::new();
        s.run();
        let previous = std::fs::read(s.data.join("latest.json")).unwrap();
        s.world.result["items"][0]["id"] = json!("unknown");
        assert_eq!(s.run(), Run::Failed);
        assert_eq!(std::fs::read(s.data.join("latest.json")).unwrap(), previous);
        assert_eq!(
            s.read("status.json")["error"],
            "Helper selected an unknown or duplicate source ID"
        );
        assert_eq!(s.read("progress.json")["phase"], "Needs attention");
        s.world.result["items"][0]["id"] = json!("x:123");
        s.world.posts = Err("paid refetch");
        assert_eq!(s.run(), Run::Published);
        assert_eq!(s.read("status.json")["error"], "");
    }

    #[test]
    fn source_failure_is_visible_without_advancing_its_watermark() {
        let mut s = Setup::new();
        s.run();
        let previous = s.read("latest.json")["watermarks"]["Slack"].clone();
        s.world.now += SignedDuration::from_hours(1);
        s.world.result["sources"]["Slack"] =
            json!({"status": "error", "count": 0, "sampled": false, "error": "Slack unavailable"});
        assert_eq!(s.run(), Run::Published);
        let digest = s.read("latest.json");
        assert_eq!(digest["watermarks"]["Slack"], previous);
        assert_ne!(digest["watermarks"]["Email"], previous);
        assert!(
            s.read("status.json")["error"]
                .as_str()
                .unwrap()
                .contains("Slack unavailable")
        );
        assert!(
            digest["sources"]
                .as_array()
                .unwrap()
                .contains(&json!("Slack · failed"))
        );
    }

    #[test]
    fn initial_source_failure_keeps_original_boundary_days_later() {
        let mut s = Setup::new();
        s.world.now = "2026-10-30T18:00:00Z".parse().unwrap();
        s.world.result["sources"]["Slack"] =
            json!({"status": "error", "count": 0, "sampled": false, "error": "Unavailable"});
        assert_eq!(s.run(), Run::Published);
        s.world.now += SignedDuration::from_hours(48);
        assert_eq!(s.run(), Run::Published);
        let p = &s.world.payloads;
        assert_eq!(
            p[0]["windows"]["Slack"]["since"],
            "2026-10-29T18:00:00+00:00"
        );
        assert_eq!(
            p[1]["windows"]["Slack"]["since"],
            p[0]["windows"]["Slack"]["since"]
        );
        assert_ne!(
            p[1]["windows"]["Slack"]["until"],
            p[0]["windows"]["Slack"]["until"]
        );
    }

    #[test]
    fn expanding_slack_scope_starts_a_new_collection_window() {
        let mut s = Setup::new();
        s.run();
        let mut settings = read_or_empty(&s.config).unwrap();
        settings.insert("slack_include_private".into(), json!(true));
        save(&s.config, &Value::Object(settings)).unwrap();
        s.world.now += SignedDuration::from_hours(1);
        assert_eq!(s.run(), Run::Published);
        let latest = s.read("latest.json");
        assert_eq!(latest["slack_scope"], "all");
        let payload = &s.world.payloads[1];
        assert!(
            payload["instructions"]
                .as_str()
                .unwrap()
                .contains("explicitly authorized private channels and DMs")
        );
        assert!(
            payload["windows"]["Slack"]["since"].as_str().unwrap()
                < latest["watermarks"]["Slack"].as_str().unwrap()
        );
    }

    #[test]
    fn processed_slot_does_not_launch_a_helper() {
        let mut s = Setup::new();
        s.run();
        assert_eq!(
            run(&s.config, &s.data, true, &mut s.world).unwrap(),
            Run::Skipped
        );
        assert_eq!(s.world.payloads.len(), 1);
    }

    #[test]
    fn x_failure_still_allows_mcp_digest() {
        let mut s = Setup::new();
        s.world.result["items"] = json!([]);
        s.world.posts = Err("X unavailable");
        assert_eq!(s.run(), Run::Published);
        assert!(
            s.read("status.json")["error"]
                .as_str()
                .unwrap()
                .contains("X collection failed")
        );
        assert_eq!(s.read("latest.json")["cursor"], Value::Null);
    }

    #[test]
    fn all_sources_failing_keeps_the_previous_digest() {
        let mut s = Setup::new();
        s.world.result["items"] = json!([]);
        s.world.posts = Err("X unavailable");
        for kind in MCP_KINDS {
            s.world.result["sources"][kind] =
                json!({"status": "error", "count": 0, "sampled": false, "error": "Down"});
        }
        assert_eq!(s.run(), Run::Failed);
        assert!(!s.data.join("latest.json").exists());
        assert_eq!(
            s.read("status.json")["error"],
            "All sources failed; previous digest preserved"
        );
    }

    #[test]
    fn missing_config_asks_for_setup() {
        let mut s = Setup::new();
        let missing = s.root.join("missing.json");
        assert_eq!(
            run(&missing, &s.data, false, &mut s.world).unwrap(),
            Run::Failed
        );
        assert!(
            s.read("status.json")["error"]
                .as_str()
                .unwrap()
                .starts_with("Set up collection in")
        );
    }

    #[test]
    fn a_held_lock_skips_the_run() {
        let mut s = Setup::new();
        std::fs::create_dir_all(&s.data).unwrap();
        let held = File::create(s.data.join("worker.lock")).unwrap();
        held.lock().unwrap();
        assert_eq!(s.run(), Run::Skipped);
        assert!(s.world.payloads.is_empty());
    }
}
