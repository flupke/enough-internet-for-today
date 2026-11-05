//! Collect the zones that are due, each into its own file. Run from a timer, or from the panel's
//! Refresh; secrets are read from a local file.

use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp, civil};
use serde_json::{Map, Value, json};

use crate::error::{collection_message, is_shown};
use crate::files::{
    civil_iso, expand_user, integer, micros, parse_utc, read_or_empty, save, utc_iso,
};
use crate::helper::{self, HcCommand, Progress};
use crate::schedule::{schedule, times};
use crate::shown;
use crate::sources::{checked_result, gather_job, social_job};
use crate::x::{self, Post};
use crate::zone::Zone;

const X_FAILED: &str = "X collection failed; check token access, credits and rate limits";
/// How long a failed zone waits before the timer tries it again: a retry costs a helper turn,
/// and for Social an X request.
const RETRY: SignedDuration = SignedDuration::from_mins(30);
/// How far back a source's first collection looks.
const FIRST_LOOKBACK: SignedDuration = SignedDuration::from_hours(24);
/// How long Slack keeps a selection, and how many items it keeps at most.
const SLACK_KEEPS: SignedDuration = SignedDuration::from_hours(24);
const SLACK_MOST: usize = 20;

/// What a collection reaches out to: the clock, X and the helper.
pub trait World {
    /// Now, in UTC.
    fn now(&self) -> Timestamp;
    /// The user's time zone, which slots and labels follow.
    fn tz(&self) -> TimeZone;
    /// The ID of the user `token` belongs to, as [`x::me`] gives it.
    fn x_user(&mut self, token: &str) -> Result<String>;
    /// The timeline's posts after `cursor`, as [`x::timeline`] gives them.
    fn timeline(
        &mut self,
        token: &str,
        me: &str,
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

    fn tz(&self) -> TimeZone {
        TimeZone::system()
    }

    fn x_user(&mut self, token: &str) -> Result<String> {
        x::me(&mut x::Http::new(token.to_owned()))
    }

    fn timeline(
        &mut self,
        token: &str,
        me: &str,
        cursor: Option<&str>,
        max_pages: i64,
    ) -> Result<(Vec<Post>, bool)> {
        x::timeline(
            &mut x::Http::new(token.to_owned()),
            me,
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

/// How a collection run ended, from best to worst: a run of several zones ends as its worst.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Run {
    /// Another run holds the lock, or no zone was due.
    Skipped,
    /// Every zone it ran was collected.
    Published,
    /// A zone failed: it keeps its previous selection, and its file says why.
    Failed,
}

/// Collect `zones` into `data`, under its lock; with `due`, only those whose current slot has
/// no collection yet.
pub fn run(
    config_path: &Path,
    data: &Path,
    zones: &[Zone],
    due: bool,
    world: &mut dyn World,
) -> Result<Run> {
    std::fs::create_dir_all(data)?;
    let lock = File::create(data.join("worker.lock"))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            println!("Another collection is running");
            return Ok(Run::Skipped);
        }
        Err(TryLockError::Error(e)) => return Err(e.into()),
    }
    crate::legacy::migrate(data, &world.tz())?;
    let config = if config_path.exists() {
        read_or_empty(config_path)
    } else {
        Err(shown!("Set up collection in {}", config_path.display()))
    };
    let mut ran = Run::Skipped;
    let progress_path = data.join("progress.json");
    let mut helper = read_or_empty(&progress_path)?
        .get("helper")
        .cloned()
        .unwrap_or(Value::Null);
    for zone in zones {
        let outcome = match &config {
            Ok(config) => run_zone(*zone, config, data, due, world, &mut helper)?,
            Err(error) => {
                let mut state = read_or_empty(&zone.file(data))?;
                fail(*zone, &mut state, error);
                save(&zone.file(data), &Value::Object(state))?;
                Run::Failed
            }
        };
        ran = ran.max(outcome);
    }
    if ran != Run::Skipped {
        save(
            &progress_path,
            &json!({"zone": null, "phase": "Ready", "helper": helper, "state": "idle"}),
        )?;
    }
    Ok(ran)
}

/// Record a failed collection in the zone's state, and print it.
fn fail(zone: Zone, state: &mut Map<String, Value>, error: &anyhow::Error) {
    let message = collection_message(error);
    println!("{}: {message}", zone.key());
    state.insert("error".into(), json!(message));
}

/// Whether the zone's current `slot` still needs its collection: none published for it, and no
/// failure in the last [`RETRY`].
fn is_due(state: &Map<String, Value>, slot: &str, now: Timestamp) -> bool {
    if state.get("slot").and_then(Value::as_str).unwrap_or("") >= slot {
        return false;
    }
    let failed = state
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|e| !e.is_empty());
    let checked = state
        .get("checked_at")
        .and_then(Value::as_str)
        .and_then(|at| parse_utc(at).ok());
    match checked {
        Some(at) if failed => now.duration_since(at) >= RETRY,
        _ => true,
    }
}

/// `at` to the second: the windows the helper searches are in whole Unix seconds.
fn seconds(at: Timestamp) -> Timestamp {
    Timestamp::from_second(at.as_second()).unwrap_or(at)
}

/// A local time as the panel shows it: `Thu 08:00`.
fn label(at: civil::DateTime) -> String {
    at.strftime("%a %H:%M").to_string()
}

/// Collect one zone when it is due, or always without `due`, and save its state.
fn run_zone(
    zone: Zone,
    config: &Map<String, Value>,
    data: &Path,
    due: bool,
    world: &mut dyn World,
    helper: &mut Value,
) -> Result<Run> {
    let path = zone.file(data);
    let mut state = read_or_empty(&path)?;
    let now = world.now();
    let local = now.to_zoned(world.tz()).datetime();
    let slots = times(zone, config).and_then(|times| schedule(&times, local));
    let collected = match slots {
        Ok((slot, next)) => {
            let slot = civil_iso(slot);
            if due && !is_due(&state, &slot, now) {
                return Ok(Run::Skipped);
            }
            state.insert("next".into(), json!(label(next)));
            let mut progress = |address: &str, phase: &str, helper_state: &Value| {
                if !address.is_empty() {
                    *helper = json!(address);
                }
                save(
                    &data.join("progress.json"),
                    &json!({"zone": zone.key(), "phase": phase, "helper": helper, "state": helper_state}),
                )
            };
            progress("", "Starting", &json!("starting"))?;
            let collected = match zone.gathered() {
                None => collect_social(config, data, &mut state, now, world, &mut progress),
                Some(kind) => {
                    collect_gathered(zone, kind, config, &mut state, now, world, &mut progress)
                }
            };
            collected.map(|()| slot)
        }
        Err(error) => Err(error),
    };
    state.insert("checked_at".into(), json!(utc_iso(now)));
    state.insert("checked".into(), json!(label(local)));
    let outcome = match collected {
        Ok(slot) => {
            state.insert("slot".into(), json!(slot));
            state.insert("refreshed".into(), json!(label(local)));
            state.insert("error".into(), json!(""));
            Run::Published
        }
        Err(error) => {
            fail(zone, &mut state, &error);
            Run::Failed
        }
    };
    save(&path, &Value::Object(state))?;
    // The batch goes once the cursor past it is saved: a crash before leaves it for the retry.
    if zone == Zone::Social && outcome == Run::Published {
        crate::files::remove(&data.join("pending.json"))?;
    }
    Ok(outcome)
}

/// Where a zone's collection says what it is doing: the helper's address, the phase and the
/// helper's state.
type Phase<'a> = dyn FnMut(&str, &str, &Value) -> Result<()> + 'a;

/// The items of a selection made at `now`, each stamped with it, which the panel's "Done for
/// now" compares.
fn stamped(items: Vec<Value>, now: Timestamp) -> Vec<Value> {
    items
        .into_iter()
        .map(|mut item| {
            item["at"] = json!(utc_iso(now));
            item
        })
        .collect()
}

/// The configured path at `key`, or `default`.
fn config_path(config: &Map<String, Value>, key: &str, default: &str) -> Result<PathBuf> {
    match config.get(key) {
        None => Ok(expand_user(default)),
        Some(Value::String(path)) => Ok(expand_user(path)),
        Some(_) => Err(shown!("{key} must be a path")),
    }
}

/// The X token: the OAuth store's, refreshed when due, or a raw token's.
fn x_token(config: &Map<String, Value>) -> Result<String> {
    let oauth = config_path(
        config,
        "x_oauth_file",
        "~/.config/herdr-digest/x-oauth.json",
    )?;
    if oauth.exists() {
        return crate::x_auth::access_token(&oauth);
    }
    let raw = config_path(config, "x_token_file", "~/.config/herdr-digest/x-token")?;
    Ok(std::fs::read_to_string(raw)?.trim().to_owned())
}

/// The new X posts after `cursor`: the batch a failed selection kept, so a retry does not pay
/// X again, or a fresh fetch, kept until its selection is published.
fn x_posts(
    config: &Map<String, Value>,
    cursor: Option<&str>,
    pending_path: &Path,
    state: &mut Map<String, Value>,
    world: &mut dyn World,
) -> Result<(Vec<Post>, bool)> {
    let pending = read_or_empty(pending_path)?;
    let kept = pending
        .get("posts")
        .and_then(Value::as_array)
        .is_some_and(|posts| !posts.is_empty());
    if kept && pending.get("since").and_then(Value::as_str) == cursor {
        let posts = pending
            .get("posts")
            .and_then(Value::as_array)
            .ok_or_else(|| shown!("Invalid pending X batch"))?
            .iter()
            .map(Post::from_value)
            .collect::<Result<_>>()?;
        return Ok((posts, pending.get("sampled") == Some(&json!(true))));
    }
    let max_pages = match config.get("max_pages") {
        None => 3,
        Some(value) => integer(Some(value))
            .filter(|l| (1..=20).contains(l))
            .ok_or_else(|| shown!("max_pages must be between 1 and 20"))?,
    };
    let fetched = (|| {
        let token = x_token(config)?;
        let me = match state.get("x_user").and_then(Value::as_str) {
            Some(me) => me.to_owned(),
            None => {
                let me = world.x_user(&token)?;
                state.insert("x_user".into(), json!(me));
                me
            }
        };
        world.timeline(&token, &me, cursor, max_pages)
    })();
    let (posts, sampled) = fetched.map_err(|e| {
        if is_shown(&e) {
            e
        } else {
            shown!("{X_FAILED}")
        }
    })?;
    if !posts.is_empty() {
        let batch: Vec<Value> = posts.iter().map(Post::to_value).collect();
        save(
            pending_path,
            &json!({"since": cursor, "posts": batch, "sampled": sampled}),
        )?;
    }
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

/// Social: the X posts after its cursor, and the helper's selection from them, which replaces
/// the previous one. No new post keeps the previous selection, and calls neither X again nor
/// the helper.
fn collect_social(
    config: &Map<String, Value>,
    data: &Path,
    state: &mut Map<String, Value>,
    now: Timestamp,
    world: &mut dyn World,
    progress: &mut Phase<'_>,
) -> Result<()> {
    let pending_path = data.join("pending.json");
    let cursor = state
        .get("cursor")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
        .map(str::to_owned);
    progress("", "Collecting X", &json!("starting"))?;
    let (posts, sampled) = x_posts(config, cursor.as_deref(), &pending_path, state, world)?;
    if posts.is_empty() {
        state.insert("coverage".into(), json!("No new posts"));
        return Ok(());
    }
    let payload = social_job(&posts, config);
    let result = world.gather(&payload, &mut |address, helper_state| {
        progress(address, "Selecting from X", helper_state)
    })?;
    let (items, _) = checked_result(&result, &payload)?;
    let mut ids: Vec<&str> = posts.iter().map(|p| p.id.as_str()).collect();
    ids.extend(cursor.as_deref());
    state.insert("cursor".into(), json!(newest(&ids)?));
    state.insert("items".into(), json!(stamped(items, now)));
    let omitted = if sampled {
        " · newest batch only; older posts omitted"
    } else {
        ""
    };
    state.insert(
        "coverage".into(),
        json!(format!("{} new posts{omitted}", posts.len())),
    );
    Ok(())
}

/// Record the Slack scope the config asks for; a different one from the cursor's starts a
/// fresh window, for the channels never collected in it.
fn slack_scope(config: &Map<String, Value>, state: &mut Map<String, Value>) {
    let scope = if crate::files::truthy(config.get("slack_include_private")) {
        "all"
    } else {
        "public"
    };
    if state
        .get("slack_scope")
        .and_then(Value::as_str)
        .unwrap_or("public")
        != scope
    {
        state.remove("cursor");
    }
    state.insert("slack_scope".into(), json!(scope));
}

/// Slack's new `items`, then those of `previous` selected in the last [`SLACK_KEEPS`], at most
/// [`SLACK_MOST`] in all.
fn with_the_day_s(mut items: Vec<Value>, previous: Option<&Value>, now: Timestamp) -> Vec<Value> {
    let kept = now - SLACK_KEEPS;
    for item in previous.and_then(Value::as_array).into_iter().flatten() {
        let recent = item["at"]
            .as_str()
            .and_then(|at| parse_utc(at).ok())
            .is_some_and(|at| at > kept);
        if recent && !items.iter().any(|i| i["id"] == item["id"]) {
            items.push(item.clone());
        }
    }
    items.truncate(SLACK_MOST);
    items
}

/// Slack or Email: the helper gathers the messages after the zone's cursor and selects from
/// them. Email's selection replaces the previous one; Slack's adds to those of the last day. A
/// run that finds nothing new keeps the selection as it was.
fn collect_gathered(
    zone: Zone,
    kind: &str,
    config: &Map<String, Value>,
    state: &mut Map<String, Value>,
    now: Timestamp,
    world: &mut dyn World,
    progress: &mut Phase<'_>,
) -> Result<()> {
    if zone == Zone::Slack {
        slack_scope(config, state);
    }
    let since = match state.get("cursor").and_then(Value::as_str) {
        Some(cursor) => cursor.to_owned(),
        None => utc_iso(seconds(now - FIRST_LOOKBACK)),
    };
    // Kept on failure too: a source's first window keeps its start until it succeeds.
    state.insert("cursor".into(), json!(since));
    let until = utc_iso(seconds(now));
    let payload = gather_job(kind, &since, &until, config);
    let phase = format!("Gathering {kind}");
    let result = world.gather(&payload, &mut |address, helper_state| {
        progress(address, &phase, helper_state)
    })?;
    let (items, status) = checked_result(&result, &payload)?;
    let status = status.ok_or_else(|| shown!("Helper did not report {kind} status"))?;
    if status["status"] != "ok" {
        return Err(shown!("{}", status["error"].as_str().unwrap_or_default()));
    }
    state.insert("cursor".into(), json!(until));
    let count = integer(status.get("count")).unwrap_or_default();
    if count == 0 {
        if zone == Zone::Slack {
            let items = with_the_day_s(Vec::new(), state.get("items"), now);
            state.insert("items".into(), json!(items));
        }
        state.insert("coverage".into(), json!("Nothing new"));
        return Ok(());
    }
    let mut items = stamped(items, now);
    if zone == Zone::Slack {
        items = with_the_day_s(items, state.get("items"), now);
    }
    state.insert("items".into(), json!(items));
    let sampled = if status["sampled"] == true {
        " · bounded sample"
    } else {
        ""
    };
    state.insert(
        "coverage".into(),
        json!(format!("{count} inspected{sampled}")),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A world with a set clock, posts and helper results by zone, which records X's requests
    /// and the helper's jobs.
    struct Fake {
        now: Timestamp,
        posts: Result<Vec<Post>, &'static str>,
        results: Map<String, Value>,
        payloads: Vec<Value>,
        /// The cursor of each timeline request.
        timelines: Vec<Option<String>>,
        users: usize,
    }

    impl World for Fake {
        fn now(&self) -> Timestamp {
            self.now
        }

        fn tz(&self) -> TimeZone {
            TimeZone::UTC
        }

        fn x_user(&mut self, token: &str) -> Result<String> {
            assert_eq!(token, "test-token");
            self.users += 1;
            Ok("42".into())
        }

        fn timeline(
            &mut self,
            _: &str,
            me: &str,
            cursor: Option<&str>,
            _: i64,
        ) -> Result<(Vec<Post>, bool)> {
            assert_eq!(me, "42");
            self.timelines.push(cursor.map(str::to_owned));
            self.posts
                .clone()
                .map(|p| (p, false))
                .map_err(|e| shown!("{e}"))
        }

        fn gather(&mut self, payload: &Value, progress: &mut Progress<'_>) -> Result<Value> {
            self.payloads.push(payload.clone());
            progress("t99", &json!("done"))?;
            Ok(self.results[payload["zone"].as_str().unwrap()].clone())
        }
    }

    fn post(id: &str) -> Post {
        Post {
            id: id.into(),
            author: "alice".into(),
            text: format!("Post {id}"),
        }
    }

    fn gathered(kind: &str, id: &str, at: &str) -> Value {
        let (url, ident) = if kind == "Slack" {
            (
                format!("https://team.slack.com/archives/C1/p{id}"),
                format!("slack:{id}"),
            )
        } else {
            (
                format!("https://mail.google.com/mail/u/0/#inbox/{id}"),
                format!("email:{id}"),
            )
        };
        json!({
            "sources": {kind: {"status": "ok", "count": 1, "sampled": false, "error": ""}},
            "candidates": [{"id": ident, "kind": kind, "author": "Bob", "timestamp": at,
                            "text": "A message", "url": url}],
            "items": [{"id": ident, "title": format!("Item {id}"), "summary": "Said", "why": "Matters"}],
        })
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
            let at = "2026-10-30T17:00:00Z";
            let results = json!({
                "social": {"sources": {}, "candidates": [],
                           "items": [{"id": "x:123", "title": "Release", "summary": "A useful release", "why": "New capability"}]},
                "slack": gathered("Slack", "1", at),
                "email": gathered("Email", "e1", at),
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
                    posts: Ok(vec![post("123")]),
                    results: results.as_object().unwrap().clone(),
                    payloads: Vec::new(),
                    timelines: Vec::new(),
                    users: 0,
                },
            }
        }

        fn run(&mut self, zone: Zone) -> Run {
            run(&self.config, &self.data, &[zone], false, &mut self.world).unwrap()
        }

        fn due(&mut self) -> Run {
            run(&self.config, &self.data, &Zone::ALL, true, &mut self.world).unwrap()
        }

        fn zone(&self, zone: Zone) -> Map<String, Value> {
            read_or_empty(&zone.file(&self.data)).unwrap()
        }

        fn titles(&self, zone: Zone) -> Vec<String> {
            self.zone(zone)["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["title"].as_str().unwrap().to_owned())
                .collect()
        }

        fn later(&mut self, minutes: i64) {
            self.world.now += SignedDuration::from_mins(minutes);
        }
    }

    #[test]
    fn social_summarizes_the_new_posts_and_moves_its_cursor() {
        let mut s = Setup::new();
        assert_eq!(s.run(Zone::Social), Run::Published);
        let social = s.zone(Zone::Social);
        assert_eq!(social["cursor"], "123");
        assert_eq!(social["items"][0]["url"], "https://x.com/i/status/123");
        assert_eq!(social["items"][0]["source"], "X · alice");
        assert_eq!(social["items"][0]["at"], "2026-10-30T18:00:00.402311+00:00");
        assert_eq!(social["coverage"], "1 new posts");
        assert_eq!(social["refreshed"], "Fri 18:00");
        assert_eq!(social["next"], "Sat 08:00");
        assert_eq!(social["error"], "");
        assert_eq!(s.world.timelines, [None]);
        assert!(!s.data.join("pending.json").exists());
        let progress = read_or_empty(&s.data.join("progress.json")).unwrap();
        assert_eq!(
            Value::Object(progress),
            json!({"zone": null, "phase": "Ready", "helper": "t99", "state": "idle"})
        );
    }

    #[test]
    fn a_refresh_with_nothing_new_asks_x_once_after_the_cursor_and_keeps_the_selection() {
        let mut s = Setup::new();
        s.run(Zone::Social);
        s.later(10);
        s.world.posts = Ok(Vec::new());
        assert_eq!(s.run(Zone::Social), Run::Published);
        assert_eq!(s.world.timelines, [None, Some("123".into())]);
        assert_eq!(s.world.users, 1, "the user's ID is kept");
        assert_eq!(s.world.payloads.len(), 1, "no helper turn");
        let social = s.zone(Zone::Social);
        assert_eq!(social["cursor"], "123");
        assert_eq!(social["coverage"], "No new posts");
        assert_eq!(s.titles(Zone::Social), ["Release"]);

        s.later(10);
        s.world.posts = Ok(vec![post("130")]);
        s.world.results["social"]["items"][0]["id"] = json!("x:130");
        s.world.results["social"]["items"][0]["title"] = json!("Newer");
        s.run(Zone::Social);
        assert_eq!(
            s.titles(Zone::Social),
            ["Newer"],
            "a new selection replaces it"
        );
        assert_eq!(s.zone(Zone::Social)["cursor"], "130");
    }

    #[test]
    fn a_failed_selection_keeps_the_previous_one_the_cursor_and_the_paid_batch() {
        let mut s = Setup::new();
        s.run(Zone::Social);
        s.later(10);
        s.world.posts = Ok(vec![post("130")]);
        s.world.results["social"]["items"][0]["id"] = json!("unknown");
        assert_eq!(s.run(Zone::Social), Run::Failed);
        let social = s.zone(Zone::Social);
        assert_eq!(
            social["error"],
            "Helper selected an unknown or duplicate source ID"
        );
        assert_eq!(social["cursor"], "123");
        assert_eq!(social["refreshed"], "Fri 18:00");
        assert_eq!(s.titles(Zone::Social), ["Release"]);

        s.world.results["social"]["items"][0]["id"] = json!("x:130");
        s.world.posts = Err("paid refetch");
        assert_eq!(s.run(Zone::Social), Run::Published);
        assert_eq!(
            s.world.timelines.len(),
            2,
            "the kept batch is not fetched again"
        );
        assert_eq!(s.zone(Zone::Social)["cursor"], "130");
        assert_eq!(s.zone(Zone::Social)["error"], "");
    }

    #[test]
    fn an_x_failure_is_the_social_zone_s_alone() {
        let mut s = Setup::new();
        s.world.posts = Err("X unavailable");
        assert_eq!(
            run(&s.config, &s.data, &Zone::ALL, false, &mut s.world).unwrap(),
            Run::Failed
        );
        assert_eq!(s.zone(Zone::Social)["error"], "X unavailable");
        assert!(!s.zone(Zone::Social).contains_key("cursor"));
        assert_eq!(s.titles(Zone::Slack), ["Item 1"]);
        assert_eq!(s.titles(Zone::Email), ["Item e1"]);
    }

    #[test]
    fn slack_adds_up_a_day_of_selections_and_email_replaces_its_own() {
        let mut s = Setup::new();
        s.run(Zone::Slack);
        s.run(Zone::Email);
        s.later(30);
        s.world.results["slack"] = gathered("Slack", "2", "2026-10-30T18:10:00Z");
        s.world.results["email"] = gathered("Email", "e2", "2026-10-30T18:10:00Z");
        s.run(Zone::Slack);
        s.run(Zone::Email);
        assert_eq!(s.titles(Zone::Slack), ["Item 2", "Item 1"]);
        assert_eq!(s.titles(Zone::Email), ["Item e2"]);

        s.later(24 * 60);
        s.world.results["slack"] = gathered("Slack", "3", "2026-10-31T18:30:00Z");
        s.run(Zone::Slack);
        assert_eq!(
            s.titles(Zone::Slack),
            ["Item 3"],
            "a day later, older ones go"
        );
        s.later(24 * 60);
        s.world.results["slack"] = json!({
            "sources": {"Slack": {"status": "ok", "count": 0, "sampled": false, "error": ""}},
            "candidates": [], "items": [],
        });
        s.run(Zone::Slack);
        assert!(
            s.titles(Zone::Slack).is_empty(),
            "a quiet day lets them go too"
        );
    }

    #[test]
    fn a_run_with_nothing_new_moves_the_cursor_and_keeps_the_selection() {
        let mut s = Setup::new();
        s.run(Zone::Slack);
        s.later(30);
        s.world.results["slack"] = json!({
            "sources": {"Slack": {"status": "ok", "count": 0, "sampled": false, "error": ""}},
            "candidates": [], "items": [],
        });
        assert_eq!(s.run(Zone::Slack), Run::Published);
        let slack = s.zone(Zone::Slack);
        assert_eq!(slack["cursor"], "2026-10-30T18:30:00+00:00");
        assert_eq!(slack["coverage"], "Nothing new");
        assert_eq!(s.titles(Zone::Slack), ["Item 1"]);
        assert_eq!(
            s.world.payloads[1]["windows"]["Slack"]["since"],
            "2026-10-30T18:00:00+00:00"
        );
    }

    #[test]
    fn a_failed_source_keeps_its_cursor_and_first_window() {
        let mut s = Setup::new();
        s.world.results["slack"] = json!({
            "sources": {"Slack": {"status": "error", "count": 0, "sampled": false, "error": "Slack unavailable"}},
            "candidates": [], "items": [],
        });
        assert_eq!(s.run(Zone::Slack), Run::Failed);
        assert_eq!(s.zone(Zone::Slack)["error"], "Slack unavailable");
        s.later(48 * 60);
        s.run(Zone::Slack);
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
    fn expanding_slack_scope_starts_a_new_window() {
        let mut s = Setup::new();
        s.run(Zone::Slack);
        let mut settings = read_or_empty(&s.config).unwrap();
        settings.insert("slack_include_private".into(), json!(true));
        save(&s.config, &Value::Object(settings)).unwrap();
        s.later(60);
        s.world.results["slack"] = gathered("Slack", "2", "2026-10-30T12:00:00Z");
        assert_eq!(s.run(Zone::Slack), Run::Published);
        assert_eq!(s.zone(Zone::Slack)["slack_scope"], "all");
        let payload = &s.world.payloads[1];
        assert!(
            payload["instructions"]
                .as_str()
                .unwrap()
                .contains("explicitly authorized private channels and DMs")
        );
        assert_eq!(
            payload["windows"]["Slack"]["since"],
            "2026-10-29T19:00:00+00:00"
        );
    }

    #[test]
    fn the_timer_runs_each_zone_on_its_own_schedule() {
        let mut s = Setup::new();
        assert_eq!(s.due(), Run::Published);
        assert_eq!(s.world.payloads.len(), 3);
        s.later(20);
        assert_eq!(
            s.due(),
            Run::Skipped,
            "18:20: every slot has its collection"
        );
        s.later(10);
        s.world.results["slack"] = gathered("Slack", "2", "2026-10-30T18:20:00Z");
        assert_eq!(s.due(), Run::Published);
        let zones: Vec<_> = s.world.payloads.iter().map(|p| p["zone"].clone()).collect();
        assert_eq!(zones[3..], [json!("slack")], "18:30: Slack alone");
        assert_eq!(s.zone(Zone::Slack)["next"], "Fri 19:00");
    }

    #[test]
    fn a_failed_zone_waits_before_the_timer_tries_again() {
        let mut s = Setup::new();
        s.world.posts = Err("X unavailable");
        s.due();
        s.world.posts = Ok(vec![post("123")]);
        s.later(10);
        s.due();
        assert_eq!(s.world.timelines.len(), 1, "no retry before 30 minutes");
        s.later(20);
        s.due();
        assert_eq!(s.world.timelines.len(), 2);
        assert_eq!(s.zone(Zone::Social)["error"], "");
    }

    #[test]
    fn missing_config_asks_for_setup_in_every_zone() {
        let mut s = Setup::new();
        let missing = s.root.join("missing.json");
        assert_eq!(
            run(&missing, &s.data, &Zone::ALL, true, &mut s.world).unwrap(),
            Run::Failed
        );
        for zone in Zone::ALL {
            let error = s.zone(zone)["error"].as_str().unwrap().to_owned();
            assert!(error.starts_with("Set up collection in"), "{error}");
        }
    }

    #[test]
    fn zones_have_the_fixtures_shape() {
        let mut s = Setup::new();
        s.due();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/digest");
        for zone in Zone::ALL {
            let fixture = read_or_empty(&zone.file(&fixtures)).unwrap();
            let state = s.zone(zone);
            let keys = |v: &Map<String, Value>| {
                let mut keys: Vec<_> = v.keys().cloned().collect();
                keys.sort();
                keys
            };
            assert_eq!(keys(&state), keys(&fixture), "{zone:?}");
            let item = |v: &Map<String, Value>| v["items"][0].as_object().unwrap().clone();
            assert_eq!(keys(&item(&state)), keys(&item(&fixture)), "{zone:?}");
        }
    }

    #[test]
    fn a_held_lock_skips_the_run() {
        let mut s = Setup::new();
        std::fs::create_dir_all(&s.data).unwrap();
        let held = File::create(s.data.join("worker.lock")).unwrap();
        held.lock().unwrap();
        assert_eq!(s.due(), Run::Skipped);
        assert!(s.world.payloads.is_empty());
    }
}
