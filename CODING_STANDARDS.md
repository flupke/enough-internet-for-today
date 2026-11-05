# Coding standards

The rules every change to enough-internet-for-today follows, in its code, tests, docs and the
text its agents read. Code review checks the change against each rule, and a finding names the
rule it breaks. Leave out what the checks already enforce: clippy with `-Dwarnings`, the
complexity gate and `typos` of `make lint`.

Keep this file under 100 lines. When a topic outgrows it, move the topic to a page of
`.agents/wiki/` and leave a line here that says when to read the page.

## Names

- Name things with the terms of `CONTEXT.md`, in code, docs and UI text, and use none of the
  synonyms it lists to avoid.

## Design

- The digest is open source: what it installs or runs assumes none of the maintainer's own
  tools or paths (a dotfiles manager, Nix, a login shell, a checkout location). It needs only
  what README's Install lists.
- `../herdr-common` and the dev shell serve checks only: what `make install` installs and what
  the mod runs never refer to them.
- One way to do a thing: `make install` installs, `herdr-digest` collects and configures, the
  panel reads. A second entry point to the same act goes.
- The source data the digest reads is private: collected messages, job files, tokens and
  secrets stay local, untracked, with owner-only modes, and out of logs and error messages the
  panel shows (`src/error.rs`).
- The helper stays read-only towards Slack and Gmail: it gets no tool that sends, edits,
  archives, deletes or marks read, and private Slack channels and DMs only on the user's
  explicit consent (README, Source setup).

## Rust

- Use the smallest visibility that permits the required use.
- Use `#[must_use]` only when ignoring a return value is likely to cause a bug.
- Design types first, then implement their methods.

## Tests

- `.agents/wiki/unit-tests.md` holds the testing policy and the kinds of tautological test: read
  it before you review a test, and hold each test the change adds or keeps to it.

## Docs

- `README.md` is for people deciding whether to use the digest: what it does, how to install,
  set up and read it, and README's Checks. History goes in change descriptions, decisions in
  `.agents/adr/`, knowledge for agents in the wiki, and how to develop here in `AGENTS.md`.

## Text for agents

- Text that agents read follows the `writing-for-agents` skill: `AGENTS.md`, this file, the
  wiki, and the helper's instructions (`src/helper.rs`, `src/sources.rs`). Load the skill
  before you write or review such text.
