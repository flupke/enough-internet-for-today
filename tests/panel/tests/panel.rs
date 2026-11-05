//! The panel shows the zones the collector writes: `herdr-digest collect`, with a stand-in
//! `hc` whose helper selects one Slack message and finds no new email, and X unreachable,
//! writes into the app sandbox's home, and the `digest` mod there shows each zone, opens the
//! Slack message's original link and finishes.
//!
//! With `DIGEST_SHOTS` naming a directory, the test saves the panel's screenshots there:
//! `digest.png`, then `finished.png`.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use app_sandbox::{APP_HOME, AppSandbox, Options, Programs};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};

const LINK: &str = "https://team.slack.com/archives/C1/p123";

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// The collector, built from the repository.
fn collector() -> PathBuf {
    let root = repository();
    let target = root.join("target");
    let status = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["build", "--quiet", "--locked", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target)
        .status()
        .unwrap();
    assert!(status.success(), "the collector does not build");
    target.join("debug/herdr-digest")
}

/// Run the collector into `data`, its helper selecting one Slack message and finding no new
/// email, and X unreachable.
fn collect(data: &Path) {
    let home = tempfile::tempdir().unwrap();
    let root = home.path();
    let bin = root.join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::copy(repository().join("tests/fake-hc"), bin.join("hc")).unwrap();
    std::fs::set_permissions(bin.join("hc"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::create_dir_all(root.join(".config/herdr-digest")).unwrap();
    std::fs::write(root.join(".config/herdr-digest/config.json"), "{}").unwrap();
    let at = Timestamp::now() - SignedDuration::from_hours(1);
    let results = root.join("results");
    std::fs::create_dir(&results).unwrap();
    let slack = json!({
        "sources": {"Slack": {"status": "ok", "count": 3, "sampled": false, "error": ""}},
        "candidates": [{"id": "slack:C1:123", "kind": "Slack", "author": "Alice", "timestamp": at.to_string(),
                        "text": "We ship on Friday", "url": LINK}],
        "items": [{"id": "slack:C1:123", "title": "Friday release", "summary": "The team ships on Friday.",
                   "why": "A date to plan around."}],
    });
    let email = json!({
        "sources": {"Email": {"status": "ok", "count": 0, "sampled": false, "error": ""}},
        "candidates": [], "items": [],
    });
    std::fs::write(results.join("slack.json"), slack.to_string()).unwrap();
    std::fs::write(results.join("email.json"), email.to_string()).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(collector())
        .arg("collect")
        .arg("--data")
        .arg(data)
        .env("HOME", root)
        .env("PATH", path)
        .env("FAKE_HC_LOG", root.join("hc.log"))
        .env("FAKE_RESULTS", &results)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "only Social fails: {output:?}"
    );
}

/// `digest.debug` once the panel recorded `last`.
fn debug(app_home: &Path, last: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let shared = std::fs::read_to_string(app_home.join("shared.json")).unwrap_or_default();
        let debug =
            serde_json::from_str::<Value>(&shared).unwrap_or_default()["digest.debug"].take();
        if debug["last"] == last {
            return debug;
        }
        assert!(
            Instant::now() < deadline,
            "the panel never recorded {last}: {debug}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// A shot of the window once it settled on a frame other than `previous`: the app shows a
/// command's effect a frame or more after the state records it.
fn shoot(sandbox: &AppSandbox, previous: Option<&[u8]>, name: &str) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut last = sandbox.shot().unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let png = sandbox.shot().unwrap();
        if png == last && previous != Some(png.as_slice()) {
            if let Some(directory) = std::env::var_os("DIGEST_SHOTS") {
                std::fs::create_dir_all(&directory).unwrap();
                std::fs::write(Path::new(&directory).join(name), &png).unwrap();
            }
            return png;
        }
        assert!(
            Instant::now() < deadline,
            "the window never settled for {name}"
        );
        last = png;
    }
}

#[test]
fn panel_shows_the_collected_digest() {
    let sandbox = AppSandbox::start(&Programs::pinned().unwrap(), &Options::default()).unwrap();
    let app_home = sandbox.directory().join(APP_HOME);
    collect(&app_home.join("data/digest"));

    let module = app_home.join("mods/digest");
    std::fs::create_dir_all(&module).unwrap();
    for file in std::fs::read_dir(repository().join("mods/digest")).unwrap() {
        let file = file.unwrap().path();
        std::fs::copy(&file, module.join(file.file_name().unwrap())).unwrap();
    }
    let inside = sandbox.inside().join(APP_HOME).join("mods/digest");
    sandbox
        .herdr_app(&["check", &inside.to_string_lossy()])
        .unwrap();
    sandbox
        .herdr_app(&["do", "space.new", r#"{"name": "vision"}"#])
        .unwrap();
    // The app loads a mod a moment after it lands in its mods.
    let deadline = Instant::now() + Duration::from_secs(30);
    while let Err(error) =
        sandbox.herdr_app(&["do", "tab.new", r#"{"name": "digest", "mod": "digest"}"#])
    {
        assert!(
            Instant::now() < deadline,
            "the app never loaded the mod: {error:#}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    // Fits "Done for now" in the window.
    sandbox.herdr_app(&["do", "app.zoom_out"]).unwrap();

    let shown = debug(&app_home, "load");
    let slack = &shown["zones"]["slack"];
    assert_eq!(
        slack["items"],
        json!([{"title": "Friday release", "source": "Slack · Alice"}])
    );
    assert_eq!(slack["coverage"], "3 inspected");
    assert_eq!(slack["error"], "");
    assert_eq!(shown["zones"]["email"]["coverage"], "Nothing new");
    assert_eq!(shown["zones"]["email"]["items"], json!([]));
    assert_eq!(
        shown["zones"]["social"]["error"],
        "X collection failed; check token access, credits and rate limits"
    );
    assert_eq!(shown["unread"], 1);
    assert_eq!(
        shown["progress"],
        json!({"zone": null, "phase": "Ready", "helper": "t7", "state": "idle"})
    );
    let digest = shoot(&sandbox, None, "digest.png");

    sandbox.herdr_app(&["do", "digest.open:slack:0"]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let opened = sandbox.directory().join("opened.log");
    while !std::fs::read_to_string(&opened)
        .unwrap_or_default()
        .contains(LINK)
    {
        assert!(Instant::now() < deadline, "the panel never opened {LINK}");
        std::thread::sleep(Duration::from_millis(200));
    }

    sandbox.herdr_app(&["do", "digest.finish"]).unwrap();
    let finished = debug(&app_home, "finish");
    assert_eq!(finished["finished"], true);
    assert_eq!(finished["zones"]["slack"]["items"], json!([]));
    shoot(&sandbox, Some(&digest), "finished.png");
    sandbox.stop().unwrap();
}
