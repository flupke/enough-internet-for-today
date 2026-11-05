.PHONY: check complexity fmt install lint test test-panel

# The checks come in levels, cheapest first; AGENTS.md says when to run which. Run them in the
# dev shell: `scripts/dev-shell scripts/check <target>`. They all build with the same flags, so
# they share one build cache.
lint test test-panel check: export RUSTFLAGS = -Dwarnings

# The panel test's own workspace, which takes herdr-common's app-sandbox crate by path.
PANEL = --manifest-path tests/panel/Cargo.toml

# Level 1: lints of every target, the panel test's included, the complexity gate, and spelling
# (typos, which skips hidden folders unless named: a right word it flags goes in _typos.toml).
lint: complexity
	cargo clippy --locked --all-targets
	cargo clippy --locked $(PANEL) --all-targets
	typos . .agents .config

# Level 2: every unit test of the collector, `tests/collect.rs` included.
# A pane of the user's Herdr sets the HERDR_* variables, one of the user's herdr-app the HC_*
# ones: the tests run without them, so none reaches the user's Herdr or herdr-app.
NO_LIVE_HERDR = env -u HERDR_ENV -u HERDR_SOCKET_PATH -u HERDR_PANE_ID -u HERDR_WORKSPACE_ID \
	-u HERDR_TAB_ID -u HERDR_APP_HOME -u HC_SCOPE -u HC_SESSION -u HC_SOCKET -u HC_TERMINAL_ID \
	-u HC_TERMINAL_SOCKET
test:
	$(NO_LIVE_HERDR) cargo nextest run --locked

# Level 3: the panel shows what the collector writes, in herdr-common's app sandbox: a private
# herdr-app on a headless display (README, Checks). Needs herdr-common checked out beside this
# repository. DIGEST_SHOTS=<directory> keeps its screenshots.
# nextest reads .config/nextest.toml from the root of the workspace it runs, tests/panel here:
# --config-file gives this test the same hang guard.
test-panel:
	$(NO_LIVE_HERDR) cargo nextest run --locked --config-file .config/nextest.toml $(PANEL)

# Every level in one run: the gate of a change (AGENTS.md, Change workflow).
check: lint
	$(MAKE) test
	$(MAKE) test-panel

# Formats the code, once a change is complete. No check fails on formatting.
fmt:
	cargo fmt --all
	cargo fmt --all $(PANEL)

# A function past 10 cyclomatic or 15 cognitive fails, unless .config/complexity-baseline.json
# names it ("<path> <name>"), with scores it may not exceed: the functions over the limit when
# the gate came. A change that touches one brings it under the limit and drops its line.
# cccc runs apart from jq: make's shell has no pipefail, so a failing cccc would pass.
complexity:
	@scores="$$(cccc --lang rust src tests)" || exit; \
	report="$$(printf '%s' "$$scores" | jq -r --slurpfile baseline .config/complexity-baseline.json '[.files[] | .path as $$path | .functions[] | recurse(.children[]?) | select(.cyclomatic > 10 or .cognitive > 15) | { path: $$path, line, name, cognitive, cyclomatic } | ($$baseline[0]["\(.path) \(.name)"]) as $$cap | select($$cap == null or .cognitive > $$cap.cognitive or .cyclomatic > $$cap.cyclomatic)] | sort_by([-.cyclomatic, -.cognitive, .path, .line]) | if length == 0 then empty else ("Cognitive\tCyclomatic\tFunction", (.[] | "\(.cognitive)\t\(.cyclomatic)\t\(.path):\(.line) \(.name)")) end')" || exit; \
	if [ -n "$$report" ]; then printf '%s\n' "$$report"; exit 1; fi

# The digest for this user: builds the collector and installs it, the mod, the config and the
# systemd units under $HOME. scripts/install says what it refuses, .agents/wiki/install.md
# what it changes.
install:
	scripts/install
