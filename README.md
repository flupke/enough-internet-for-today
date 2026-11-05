# Enough Internet for Today

A finite Herdr digest from followed X accounts, public Slack channels you belong to, and your Gmail inbox. An interactive Claude Sonnet 5.5 helper at medium effort gathers Slack and email through your connected MCP servers, then selects at most five useful updates across all sources. Read the selection, follow an original link if needed, and choose Done for now.

## Install

Requirements: Linux with user systemd, Rust's `cargo` (1.89 or later), `herdr-app`, `hc`, and a logged-in `claude` CLI with Gmail and Slack MCP connections. Check connections with `claude mcp list`.

Run `./install.sh`. It builds the `herdr-digest` collector and installs it in `~/.local/bin`, installs the mod and timer, configures the `enough-internet` hc template, and preserves existing local configuration. On first launch, the Claude workspace at `~/.config/herdr-digest/helper` may require a one-time trust decision; inspect it in Herdr or use `hc attach enough-internet`. After accepting, run collection again. A startup task that wasn't delivered is never blindly resent.

## Source setup

Local settings live in `~/.config/herdr-digest/config.json`; `config.example.json` shows defaults. Set `interests` to the topics you want prioritized. Without a profile, the helper selects generally substantive updates and does not invent personal relevance. The old `llm_command` setting is no longer used.

For X, create an OAuth 2.0 Native App with callback URL `http://127.0.0.1:8765/callback`. Use this repository as its website URL. Store its OAuth Client ID as `ENOUGH_INTERNET_FOR_TODAY_X_COM_CLIENT_ID` in `~/.secrets` (shell-style `NAME=value`, optional `export`; shell code is never evaluated).

```
herdr-digest authorize --public-client
```

The helper requests `tweet.read`, `users.read`, and `offline.access`. It saves tokens with mode 0600 in `~/.config/herdr-digest/x-oauth.json` and refreshes them automatically. Never commit that file, `~/.secrets`, collected messages, or job files. For a confidential Web/Automated app, also set `ENOUGH_INTERNET_FOR_TODAY_X_COM_SECRET` to its OAuth 2.0 Client Secret and omit `--public-client`. A raw user token in `~/.config/herdr-digest/x-token` is supported as a fallback without automatic refresh. X endpoint access and credits are required.

Slack uses only public channels you belong to by default. Email uses recent Gmail inbox threads, excluding spam and trash. Both are read-only: no sends, edits, archive, delete, or mark-read tools are authorized. Private channels and DMs require explicit consent: set `slack_include_private` to true and run `herdr-digest configure --include-private-slack`. Restart an existing idle helper with `hc kill enough-internet` so its next launch picks up that permission change. `install.sh` preserves the configured Slack scope; new installations default to public-only Slack.

## Collection and reading

```
herdr-digest collect
systemctl --user enable --now herdr-digest.timer
herdr-app do tab.new --name 'Enough Internet' --mod digest
```

The timer checks every five minutes; `times` sets the daily local schedule, default 09:00 and 18:00. Missed slots produce one catch-up digest while the user service manager runs. It does not wake a sleeping machine. The panel reads local updates every 15 seconds. Collection works independently of the window.

X excludes replies/reposts and samples up to 300 new posts (configurable with `max_pages`, maximum 20). Initial collection covers the past 24 hours. If capped, older posts are explicitly labeled omitted; the X cursor advances. Selection failures retain a pending X batch so retries avoid another paid fetch.

Slack and email each inspect at most 100 recent messages/threads per digest, newest first, since their last successful collection (first run: 24 hours). Coverage and failures are shown per source. A failed source retains its previous watermark; other successful sources may publish a digest. If all sources fail or the result is invalid, the previous digest stays in place.

## Watch the helper

The `enough-internet` helper is persistent and watchable: use the panel's Inspect Sonnet helper button, `hc attach enough-internet`, or `hc read enough-internet`. Each job has unique input/result files under `~/.config/herdr-digest/helper/jobs`; results are read from JSON files, never scraped from terminal text. Inputs and results contain private source data and remain local/untracked. `hc wait` follows completion; blocked/error/timeout states surface in the panel. Only the helper bearing the app's ownership label is reused. No additional agents are started by the helper.

The collector validates selection IDs, field sizes, time windows, source counts, and HTTPS links to the source domains. X links are constructed from fetched IDs. Slack and email links must come from their MCP results. These structural checks cannot independently verify the accuracy of an LLM's summary or MCP evidence extraction; original links are retained for review.

## Checks

```
cargo test
```

`tests/collect.rs` runs `herdr-digest collect` with a stand-in `hc` (`tests/fake-hc`), whose helper writes a set result.

The panel is checked with the `herdr-mod-vision` skill in [herdr-common](https://github.com/flupke/herdr-common)'s app sandbox: a private herdr-app, at the release herdr-common's flake pins, on a headless display, so the check never touches your window or `~/.config/herdr-app`. Check herdr-common out beside this repository; it serves checks only, and nothing the digest installs or runs refers to it. Run it from the repository's root; `N` names the sandbox after the checkout, since other agents run sandboxes on the same machine:

```
S=../herdr-common/skills/herdr-mod-vision/app-sandbox N=digest-$(basename "$PWD" | tr -c "A-Za-z0-9_\n-" -) out=$(mktemp -d)
$S start $N
copy=$($S mod $N mods/digest) && $S herdr-app $N check "$copy"
home=$($S info $N | $S tool jq -r .app_home)
mkdir -p "$home/data/digest" && cp tests/fixtures/latest.json "$home/data/digest/latest.json"
$S herdr-app $N do space.new --name vision
$S herdr-app $N do tab.new --name digest --mod digest
$S herdr-app $N do app.zoom_out    # fits "Done for now" in the 1400×900 window
for _ in $(seq 30); do $S tool jq -e '."digest.debug".last == "load"' "$home/shared.json" >/dev/null 2>&1 && break; sleep 1; done
$S tool jq '."digest.debug" | {error, status, titles: [.items[].title]}' "$home/shared.json"
$S shot $N "$out/digest.png"       # both fixture items, then "Done for now"
$S herdr-app $N do digest.finish
for _ in $(seq 30); do $S tool jq -e '."digest.debug".last == "finish"' "$home/shared.json" >/dev/null 2>&1 && break; sleep 1; done
$S shot $N "$out/finished.png"     # "You’re caught up."
$S stop $N
```

`tests/panel` checks the same in a test of its own: it runs `herdr-digest collect` with the stand-in `hc` into a sandbox's app home, then the panel shows the collected digest, opens its original link and finishes. It is a workspace of its own, which takes herdr-common's `app-sandbox` crate by path, so the collector's build never reads `../herdr-common`. Run it in herdr-common's dev shell; `DIGEST_SHOTS=<directory>` keeps its screenshots:

```
../herdr-common/scripts/dev-shell cargo test --manifest-path tests/panel/Cargo.toml
```

`tests/fixtures/latest.json` is a digest file in the shape `herdr-digest collect` writes (`src/collect.rs`, whose tests check the fixture's keys), which the panel reads as it reads a collected one. For other states, open the tab with `--props '{"demo":true}'`: `digest.sample` and `digest.empty` replace its content, `digest.failure` adds a sample collection error, and all three are ignored outside demo mode. `digest.finish`, `digest.peek` and `digest.open:0` use the same action methods as clicks. `digest.debug` in the app's `shared.json` records what the panel shows, the text to check against the screenshot. A link the panel opens lands in the sandbox's `opened.log`, never in a browser.

References: [X timeline](https://docs.x.com/x-api/users/get-timeline), [X OAuth](https://docs.x.com/fundamentals/authentication/oauth-2-0/user-access-token), [Claude MCP](https://code.claude.com/docs/en/mcp), [Claude permissions](https://code.claude.com/docs/en/permissions).
