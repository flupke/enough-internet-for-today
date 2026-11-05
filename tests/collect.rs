//! `herdr-digest collect` end to end, with a stand-in `hc` whose helper writes a set result for
//! each zone: the files the panel reads, under `HERDR_APP_HOME`.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};

/// An `hc` that logs its calls, has no helper to reuse, starts one, and on `wait` writes
/// `$FAKE_RESULTS/<zone>.json` as each job's result; with `FAKE_HC_FAIL` set, `run` fails.
const FAKE_HC: &str = include_str!("fake-hc");

struct Home {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Home {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_owned();
        let bin = root.join("bin");
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(bin.join("hc"), FAKE_HC).unwrap();
        std::fs::set_permissions(bin.join("hc"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let config = root.join(".config/herdr-digest");
        std::fs::create_dir_all(&config).unwrap();
        // No X token: Social fails, and Slack and Email still collect. Every zone runs at
        // midnight, so a second run with --due finds its slot collected.
        std::fs::write(
            config.join("config.json"),
            r#"{"morning": "00:00", "slack_every_minutes": 1440}"#,
        )
        .unwrap();
        let at = (Timestamp::now() - SignedDuration::from_hours(1)).to_string();
        let results = root.join("results");
        std::fs::create_dir(&results).unwrap();
        let slack = json!({
            "sources": {"Slack": {"status": "ok", "count": 3, "sampled": false, "error": ""}},
            "candidates": [{"id": "slack:C1:123", "kind": "Slack", "author": "Alice", "timestamp": at,
                            "text": "We ship on Friday", "url": "https://team.slack.com/archives/C1/p123"}],
            "items": [{"id": "slack:C1:123", "title": "Friday release", "summary": "The team ships on Friday.",
                       "why": "A date to plan around."}],
        });
        let email = json!({
            "sources": {"Email": {"status": "ok", "count": 0, "sampled": false, "error": ""}},
            "candidates": [], "items": [],
        });
        std::fs::write(results.join("slack.json"), slack.to_string()).unwrap();
        std::fs::write(results.join("email.json"), email.to_string()).unwrap();
        Self { _dir: dir, root }
    }

    fn collect(&self, args: &[&str], fail: bool) -> Output {
        let path = format!(
            "{}:{}",
            self.root.join("bin").display(),
            std::env::var("PATH").unwrap()
        );
        let mut command = Command::new(env!("CARGO_BIN_EXE_herdr-digest"));
        command
            .arg("collect")
            .args(args)
            .env("HOME", &self.root)
            .env("PATH", path)
            .env("HERDR_APP_HOME", self.root.join("app"))
            .env("FAKE_HC_LOG", self.root.join("hc.log"))
            .env("FAKE_RESULTS", self.root.join("results"));
        if fail {
            command.env("FAKE_HC_FAIL", "1");
        }
        command.output().unwrap()
    }

    fn data(&self, name: &str) -> Value {
        read(&self.root.join("app/data/digest").join(name))
    }

    fn hc_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.join("hc.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn collects_each_zone_through_the_helper_then_waits_for_the_next_slot() {
    let home = Home::new();
    let output = home.collect(&[], false);
    assert_eq!(output.status.code(), Some(1), "Social failed: {output:?}");
    let slack = home.data("slack.json");
    let item = &slack["items"][0];
    assert_eq!(item["title"], "Friday release");
    assert_eq!(item["source"], "Slack · Alice");
    assert_eq!(item["url"], "https://team.slack.com/archives/C1/p123");
    assert_eq!(slack["error"], "");
    assert_eq!(home.data("email.json")["error"], "");
    let social = home.data("social.json");
    assert_eq!(
        social["error"],
        "X collection failed; check token access, credits and rate limits"
    );
    assert_eq!(
        home.data("progress.json"),
        json!({"zone": null, "phase": "Ready", "helper": "t7", "state": "idle"})
    );
    assert_eq!(
        home.hc_calls(),
        ["get", "run", "wait", "get", "run", "wait"]
    );
    let jobs = home.root.join(".config/herdr-digest/helper/jobs");
    for job in std::fs::read_dir(jobs).unwrap() {
        let mode = std::fs::metadata(job.unwrap().path().join("input.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    let again = home.collect(&["--due"], false);
    assert!(again.status.success(), "{again:?}");
    assert_eq!(home.hc_calls().len(), 6, "every slot has its collection");
    assert_eq!(home.data("slack.json"), slack);
}

#[test]
fn a_helper_that_cannot_start_leaves_a_visible_error() {
    let home = Home::new();
    let output = home.collect(&["--zone", "slack"], true);
    assert_eq!(output.status.code(), Some(1));
    let message = "hc run failed; inspect the Enough Internet helper in Herdr";
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("slack: {message}")
    );
    assert_eq!(home.data("slack.json")["error"], message);
    assert!(!home.root.join("app/data/digest/email.json").exists());
}
