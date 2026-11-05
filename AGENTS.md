# Checks

Run the unit tests of README's Checks on every change.

A change to the panel (`mods/digest/main.js`, `mods/digest/mod.json`) or to the digest file it reads is
checked with the `herdr-mod-vision` skill, in herdr-common's app sandbox, as README's Checks
shows: drive each behaviour the change touched, read `digest.debug`, take a shot and Read it,
then hand the user the screenshot. Run `tests/panel` as well, as README's Checks shows: it
checks that the panel shows what the collector writes. The digest runs only in the sandbox
during checks: the user's herdr-app and `~/.config/herdr-app` stay as they are.

`../herdr-common` serves checks only: what `install.sh` installs and what the mod runs never
refer to it.

`tests/fixtures/latest.json` follows the digest file `herdr-digest collect` writes
(`src/collect.rs`): a change to that file's fields updates the fixture in the same change.
