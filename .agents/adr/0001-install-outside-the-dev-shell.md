# Install outside the dev shell

The checks run in a Nix dev shell (`scripts/dev-shell`), which pins the Rust toolchain. A binary
cargo builds there takes its interpreter and `RUNPATH` from the Nix store's glibc, and Nix's
garbage collector may delete those paths under the installed `~/.local/bin/herdr-digest`,
which the systemd timer runs.

`make install` refuses to run in the dev shell (`IN_NIX_SHELL`), and builds with the `cargo`
on the user's `PATH`, which README's Install already requires: the installed binary links the
system's libraries, as it does for anyone who installs the digest without Nix.

Given up: installing through the dev shell, as herdr-bowtie and herdr-ruminate do, and rooting
the store paths the binary names so the collector keeps them (herdr-bowtie's
`scripts/root-store-references`). It would pin the installed toolchain too, but adds a step
that has to stay in sync with each build, for a binary whose only consumer is a timer.
