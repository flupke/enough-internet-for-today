#!/usr/bin/env bash
set -euo pipefail
project_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
app_dir=$HOME/.config/herdr-app
config_dir=$HOME/.config/herdr-digest
mkdir -p "$app_dir/mods/digest" "$config_dir" "$HOME/.config/systemd/user"
chmod 700 "$config_dir"
for file in main.js mod.json collect.py x_auth.py helper.py sources.py config.example.json test_collect.py test_auth.py test_helper.py test_sources.py; do
  install -m 644 "$project_dir/mods/digest/$file" "$app_dir/mods/digest/$file"
done
chmod 755 "$app_dir/mods/digest/collect.py"
install -m 644 "$project_dir/README.md" "$app_dir/mods/digest/README.md"
if [[ ! -e "$config_dir/config.json" ]]; then
  install -m 600 "$project_dir/mods/digest/config.example.json" "$config_dir/config.json"
fi
install -m 644 "$project_dir/systemd/herdr-digest.service" "$HOME/.config/systemd/user/herdr-digest.service"
install -m 644 "$project_dir/systemd/herdr-digest.timer" "$HOME/.config/systemd/user/herdr-digest.timer"
python3 "$app_dir/mods/digest/helper.py" --configure --config "$config_dir/config.json"
herdr-app check "$app_dir/mods/digest"
systemctl --user daemon-reload
printf '%s\n' 'Installed. Authorize X, then enable herdr-digest.timer when ready.'
