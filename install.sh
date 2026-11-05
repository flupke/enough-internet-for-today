#!/usr/bin/env bash
set -euo pipefail
project_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
app_dir=$HOME/.config/herdr-app
config_dir=$HOME/.config/herdr-digest
bin_dir=$HOME/.local/bin
cargo build --release --locked --manifest-path "$project_dir/Cargo.toml" --target-dir "$project_dir/target"
mkdir -p "$app_dir/mods/digest" "$config_dir" "$HOME/.config/systemd/user" "$bin_dir"
chmod 700 "$config_dir"
for file in main.js mod.json config.example.json; do
  install -m 644 "$project_dir/mods/digest/$file" "$app_dir/mods/digest/$file"
done
# What the Python collector, which herdr-digest replaces, installed beside the mod.
for file in collect.py x_auth.py helper.py sources.py test_collect.py test_auth.py test_helper.py test_sources.py; do
  rm -f "$app_dir/mods/digest/$file"
done
rm -rf "$app_dir/mods/digest/__pycache__"
install -m 755 "$project_dir/target/release/herdr-digest" "$bin_dir/herdr-digest"
install -m 644 "$project_dir/README.md" "$app_dir/mods/digest/README.md"
if [[ ! -e "$config_dir/config.json" ]]; then
  install -m 600 "$project_dir/mods/digest/config.example.json" "$config_dir/config.json"
fi
install -m 644 "$project_dir/systemd/herdr-digest.service" "$HOME/.config/systemd/user/herdr-digest.service"
install -m 644 "$project_dir/systemd/herdr-digest.timer" "$HOME/.config/systemd/user/herdr-digest.timer"
"$bin_dir/herdr-digest" configure --config "$config_dir/config.json"
herdr-app check "$app_dir/mods/digest"
systemctl --user daemon-reload
printf '%s\n' 'Installed. Authorize X, then enable herdr-digest.timer when ready.'
