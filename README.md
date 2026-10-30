# Enough Internet for Today

A finite Herdr digest from the authenticated user's followed accounts. The collector excludes replies and reposts, reads at most 300 new posts by default, and asks Claude to select at most five items. The initial collection covers the past 24 hours. Increasing `max_pages` permits larger batches (maximum 20). If more posts are available, the digest explicitly says it covers only the newest batch and older posts were omitted. Its cursor then advances past that batch. This is deliberate sampling, not exhaustive coverage. Failed selections retain a local pending batch so retries do not fetch those posts again.

## Install

Run `./install.sh` from the repository checkout to install the mod and inactive timer. It preserves existing local configuration.

## Setup

Local configuration: `~/.config/herdr-digest/config.json` (copy `config.example.json`). Set `interests` to the topics you want prioritized. Without a profile, Claude selects generally substantive updates and does not claim personal relevance.

Create an X developer app with OAuth 2.0 enabled, Native App type, and callback URL `http://127.0.0.1:8765/callback`. Use this repository as the website URL. Store your OAuth Client ID as `ENOUGH_INTERNET_FOR_TODAY_X_COM_CLIENT_ID` in `~/.secrets` (shell-style `NAME=value`, optional `export`; no shell code is evaluated).

Authorize locally:

```
python3 ~/.config/herdr-app/mods/digest/x_auth.py --authorize --public-client
```

The helper opens X in your browser, verifies the callback state, and requests `tweet.read`, `users.read` and `offline.access`. Tokens are saved atomically with mode 0600 in `~/.config/herdr-digest/x-oauth.json` and refreshed automatically before expiry. Keep this file and `~/.secrets` outside version control. The X developer app must have endpoint access and sufficient credits.

For a confidential Web/Automated app, also set `ENOUGH_INTERNET_FOR_TODAY_X_COM_SECRET` to the OAuth 2.0 Client Secret and omit `--public-client`. The API key secret from OAuth 1.0a is a different credential. A raw user token in `~/.config/herdr-digest/x-token` is supported as a fallback but cannot be refreshed automatically.

The default selector uses your Claude CLI login, disables tools and MCP servers, and uses a JSON schema for structured output on stdout. `llm_command` is an argument array; no shell expansion occurs. Post text is sent to Claude. The selected IDs are checked against collected posts, and source links are constructed locally.

Run one collection:

```
~/.config/herdr-app/mods/digest/collect.py
```

Activate scheduled collection after configuring access:

```
systemctl --user daemon-reload
systemctl --user enable --now herdr-digest.timer
```

The timer checks every five minutes. `times` controls the local daily schedule (default 09:00 and 18:00). Missed slots produce one catch-up digest while the user service manager is running. This does not wake a sleeping machine. No unread counter or refresh button is shown.

Open the panel:

```
herdr-app do tab.new --name Digest --mod digest
```

`Done for now` persists per mod copy through Herdr localStorage. A new digest shows the next selection. The panel reads updates once a minute, including while hidden. Collection runs independently of the window. Failures appear above the previous digest; its cursor stays unchanged. Local files are replaced atomically and have mode 0600.

## Checks

```
herdr-app check ~/.config/herdr-app/mods/digest
python3 -m unittest discover -s ~/.config/herdr-app/mods/digest -p 'test_*.py'
```

For herdr-mod-vision, open with `--props '{"demo":true}'`. Commands `digest.sample`, `digest.finish`, `digest.empty`, `digest.failure`, and `digest.open:0` use the same methods as clicks. `digest.debug` in shared.json records the view. Sample commands are ignored outside demo mode.

API reference: https://docs.x.com/x-api/users/get-timeline
Official OAuth example: https://github.com/xdevplatform/samples/blob/main/python/users/timeline/get_home_timeline.py

OAuth reference: https://docs.x.com/fundamentals/authentication/oauth-2-0/user-access-token
