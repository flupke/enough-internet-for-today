# Eyes and hands

Give yourself eyes and hands on what you build: run it, look at it, act on it as its user
would, and fix what you see. Tests prove what you thought of; eyes show what you did not.

- The panel: the `herdr-mod-vision` skill, in herdr-common's app sandbox, a private herdr-app on
  a headless display, as README's Checks shows. Drive each behaviour the change touched, read
  `digest.debug`, take a shot and Read it, then hand the user the screenshot.
- The collector: `herdr-digest collect` with the stand-in `hc` (`tests/fake-hc`) first on
  `PATH`, a temporary `HOME` and `HERDR_APP_HOME`, as `tests/collect.rs` runs it.

The digest runs only in a sandbox or a temporary `HOME` during checks: the user's herdr-app,
`hc`, `~/.config/herdr-app` and `~/.config/herdr-digest` stay as they are.

# Standards

Read `CODING_STANDARDS.md`, the coding standards, before you write a change. While you work:

- When the user corrects a concept, fix its entry in `CONTEXT.md` in the same change.
- herdr-bowtie and herdr-ruminate (`../herdr-bowtie`, `../herdr-progressive-reviewer`) came
  first, on the same stack: this repository's checks and change workflow come from them. Read
  how they do a thing before you design it here.

# Tests

Before you write a test, read `.agents/wiki/unit-tests.md`: it says which tests earn their
place. Every change deletes the tests it makes obsolete or redundant. Acceptance criteria name
outcomes the user can see, not lists of test cases.

How each kind of change is tested:

- The collector: `make test`.
- The panel (`mods/digest/`) or the digest file it reads: the panel's eyes and hands above, and
  `make test-panel`, which checks that the panel shows what the collector writes.
- The digest file's fields (`src/collect.rs`): `tests/fixtures/latest.json` follows them, in
  the same change.
- The install (`scripts/install`, `make install`, `systemd/`): run it twice into a temporary
  `HOME`, as [Install](.agents/wiki/install.md) says.
- The checks (`Makefile`, `flake.nix`, `flake.lock`, `scripts/check`, `scripts/dev-shell`,
  `_typos.toml`, `.config/`): `make check`, and one run that goes red on what the changed check
  catches.
- Docs alone: search README.md, AGENTS.md, CONTEXT.md, CODING_STANDARDS.md, the ADRs and the
  wiki for each statement of what the change changes, until all agree with it.

# Wiki

The wiki is a playbook for working in this repository, one topic per page, listed in
`.agents/wiki/index.md`. Read the page for a tool (the checks, the install) before you run or
debug it. The workflow says when to read the pages of an area (step 1) and when to update them
(step 2).

A line earns its place when an agent working here needs it and cannot get it cheaply from the
code, the tests or a `--help`:

- how to run a tool, and its gotchas;
- how an outside tool behaves, such as herdr-app, `hc`, Claude Code, X's API and systemd;
- the reason for a choice, or a guarantee that spans several files, which no single file can
  state.

What a module does, its files and the behaviour a test checks stay in the code and the tests.
Give a new topic its own page, and add its line to the index. A page says how things work now:
the account of a past run goes in the change description.

# Decisions

`.agents/adr/` holds the decision records (ADRs), one decision per file, numbered from `0001`.
A record keeps a decision from being undone by accident.

- When a change would contradict a record, tell the user which record, and why its reasons may
  no longer hold. Change the design once the user agrees.
- Record a decision when all three hold: it is hard to reverse, it would surprise a reader who
  lacks the context, and a real alternative was weighed. Say the situation, the choice, and the
  alternative given up with the reason.
- A record is frozen. When a decision changes, write a new record that names the one it
  supersedes, and leave the old text as it was.

# Change workflow

Follow these steps for every change: a feature, a fix, or a change to docs.

1. Create a fresh jj change before implementation. Use the previous change as the fixed point
   for this change. Read the wiki pages and the decision records of the area you will change.
2. Implement the change, and check it as you go with the levels that fit it, cheapest first,
   each in the dev shell (`scripts/dev-shell scripts/check lint`, and so on):
   1. `make lint`: clippy on every target, the complexity gate, and spelling.
   2. `make test`: every test of the collector.
   3. `make test-panel`: the panel test, for a change that reaches the panel or the digest
      file.
   Once the change is complete, run `make fmt` and bring the wiki up to date: edit each line
   the change made false, and add a line only when it earns its place (see Wiki). Record a
   decision that the change made, when the decision passes the test in Decisions.
3. For a change the user can see, explore the affected paths with the tools of Eyes and hands,
   and fix what you find.
4. Invoke the `code-review` skill against the fixed point; the subagents it starts are
   authorized. Check each finding against the code before acting on it: a finding you cannot
   confirm does not count. Fix the confirmed ones, by your judgement for a judgement call, and
   review again until a round finds nothing that you fix, two rounds at most. After the second
   round, fix what it found, and report any finding you leave unfixed, with the reason.
5. Once step 4 is over, run the gate, with the change as the working copy: `make check`. Fix
   whatever fails, even outside the change, run `make fmt`, review those fixes with one round
   of step 4, and run the gate again until it passes. This repository runs no CI: that run is
   the only check main gets.
6. Check the wiki against the change once more, since the fixes of steps 4 and 5 count too.
   Then describe the change with `jj describe`: a plain imperative subject, then a first
   paragraph on why, then what changed for the user.
7. Run `make install` once the gate passes, in the default workspace only, unless the user
   deferred installing; say so when you skip it. It builds and installs only; it does not run
   the checks.
8. Keep later user-feedback fixes in the same change, and run the gate again once each set of
   them is done. Create another change only when the user asks for something new.

The dev shell (`flake.nix`) brings the tools the checks run, in the default workspace and in
any other jj workspace alike; `scripts/check` keeps the whole output under `target/checks/`
and ends on the check's status. Run `make install` outside it (ADR 0001). Keep the warning
gate of the checks (`-Dwarnings`) enabled; fix warnings in the current change instead of
overriding it.

## When herdr-bowtie runs you

herdr-bowtie runs its workers on this repository too, each in its task's jj workspace. There,
its procedures replace steps 1, 4, 6, 7 and 8: start, review, describe and ship as they say.
Steps 2, 3 and 5 still apply. Never install from a task's workspace: the lead installs from the
default one once the change is on main.

# Skill settings

## Agent skills

### Issue tracker

Issues live in GitHub Issues for flupke/enough-internet-for-today (via `gh`). See
`.agents/settings/issue-tracker.md`.

### Triage labels

Default five-role vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`,
`ready-for-human`, `wontfix`). See `.agents/settings/triage-labels.md`.

### Domain docs

Single-context: root `CONTEXT.md` plus `.agents/adr/`. See `.agents/settings/domain.md`.
