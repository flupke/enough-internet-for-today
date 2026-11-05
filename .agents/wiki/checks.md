# Checks

The dev shell and the gotchas of the check commands. [AGENTS.md](../../AGENTS.md) lists the
check levels (Change workflow, step 2) and the gate (step 5); the `Makefile` says what each
target runs. What neither says:

## The dev shell

- Enter it through `scripts/dev-shell`, from the default checkout or a jj workspace alike: a jj
  workspace has no `.git`, so a plain `nix develop` would copy the whole folder, `target/`
  included, into the Nix store. The script hands Nix a copy of `flake.nix` and `flake.lock`
  alone, and never writes `flake.lock` back: after a change to the inputs, run `nix flake lock`
  in the checkout.
- `flake.nix` takes herdr-common as an input, and its nixpkgs, rust-overlay and cccc follow
  herdr-common's lock, so the shell reuses the store paths of herdr-common's own. `nix flake
  update herdr-common` moves all of them, and the herdr-app and `hc` the panel test runs, at
  once; run `scripts/dev-shell scripts/check check` after it.
- A binary built in the dev shell links the Nix store's glibc: never install one
  (ADR 0001).

## The commands

- `typos` skips hidden folders unless named, so `make lint` names `.agents` and `.config`; a new
  hidden folder of text needs adding there. A word `typos` flags that is right goes in
  `_typos.toml`, under `[default.extend-words]`.
- The collector is a binary crate with no library, so it has no doc test, and `cargo test
  --doc` fails on it: `make test` runs nextest alone.
- `.config/nextest.toml` ends a test still running after two minutes: the hang guard of tests
  that wait on events without a deadline ([Tests](unit-tests.md), Waiting).
- The complexity gate (`make complexity`, part of `make lint`) fails a function past 10
  cyclomatic or 15 cognitive, as herdr-bowtie's and herdr-ruminate's do. The functions over that
  limit when the gate came are named in `.config/complexity-baseline.json`, each with the
  scores it had then as its cap: a change may not raise them, and a change that rewrites one
  brings it under the limit and drops its line.

## The panel test

`tests/panel` is a Cargo workspace of its own, with its own `Cargo.lock` and `target/`, which
takes herdr-common's `app-sandbox` crate by path from `../herdr-common`, so the collector's own
build never reads it. A search or a `cargo` command at the root misses it: `make lint` and
`make fmt` name its manifest.

It runs the herdr-app and `hc` releases the dev shell names (`TEST_HERDR_APP_BIN_PATH`,
`TEST_HC_BIN_PATH`), which herdr-common's flake pins, and builds `app-sandbox` from the
checkout beside this one as it stands: the two can differ. A change herdr-common needs goes
through `lead@herdr-common`, never an edit of its checkout from here.

`DIGEST_SHOTS=<directory> make test-panel` keeps the panel's screenshots; Read them to see what
the test saw.

The sandbox's app renders `font_semibold()`, `font_bold()` and `italic()` in the regular face
of its UI font: a sandbox shot cannot show weights or italics, so hierarchy that must hold
there comes from size and colour. In the sandbox, `home()` is `/tmp/home`, so the panel's Refresh
runs `/tmp/home/.local/bin/herdr-digest`: a stand-in script there drives it without a
collector.
