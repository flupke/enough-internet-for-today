# Install

What `make install` changes, and how to test a change to it.

## What it does

`make install` runs `scripts/install`, the one install entry point: README's Install sends
people to it, and the lead runs it in the default checkout after each merge on main. It must
stay non-interactive and safe to run again:

- It builds the release collector with the `cargo` on `PATH` and copies it to
  `~/.local/bin/herdr-digest`; the systemd service runs that copy, never a file of the
  checkout.
- It replaces the mod's files in `~/.config/herdr-app/mods/digest`, the systemd units in
  `~/.config/systemd/user`, and the `enough-internet` hc template
  (`~/.config/herdr-core/templates/`, through `herdr-digest configure`, which refuses to
  overwrite a template it did not write).
- It keeps the user's `~/.config/herdr-digest/config.json`: only a missing one is created, from
  `mods/digest/config.example.json`.
- It reloads the systemd user manager, and leaves the timer as the user set it: enabling it is
  theirs (README, Collection and reading).
- `herdr-app check` writes `gpui-kit.d.ts` and `jsconfig.json` into the installed mod's folder.

`scripts/install` itself refuses to run, so running it without `make` is guarded the same:

- in a jj workspace other than the default one (`.jj/repo` is a file there): a task's workspace
  holds a change under way. `INSTALL_FROM_WORKSPACE=1` overrides it, for a test into a
  temporary `HOME` only;
- in the dev shell (`IN_NIX_SHELL`), whose build links the Nix store's glibc (ADR 0001).

## Testing a change to it

Run it twice into a temporary `HOME`, from your workspace, outside the dev shell, with a
stand-in `systemctl` first on `PATH`: a temporary `HOME` does not move the user's systemd
manager, which `systemctl --user` still reaches. Keep `rustup`'s and cargo's own homes, which
it reads from `HOME` otherwise:

```sh
dir=$(mktemp -d) && mkdir "$dir/bin" "$dir/home"
printf '#!/bin/sh\necho "systemctl $*" >> "$HOME/systemctl.log"\n' > "$dir/bin/systemctl"
chmod +x "$dir/bin/systemctl"
for run in 1 2; do
  env RUSTUP_HOME="$HOME/.rustup" CARGO_HOME="$HOME/.cargo" HOME="$dir/home" \
    PATH="$dir/bin:$PATH" INSTALL_FROM_WORKSPACE=1 make install || break
done
find "$dir/home" -type f | sort
```

Both runs pass. The listing holds the binary, the mod, `config.json` (mode 600), the two
units and the template; `systemctl.log` holds one `daemon-reload` per run. Edit `config.json`
between the runs: the second keeps it. Never run it against the user's own `HOME` from a
workspace.
