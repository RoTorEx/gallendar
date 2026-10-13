#!/usr/bin/env sh
set -eu

binary="${1:?binary path is required}"
install_dir="${2:?install directory is required}"
config_dir="${GALLENDAR_CONFIG_DIR:-$install_dir}"

[ -x "$binary" ] || { echo "ERROR: missing binary $binary" >&2; exit 1; }
mkdir -p "$install_dir/bin" "$config_dir"
chmod 0700 "$install_dir" "$config_dir"
temporary="$install_dir/bin/.gallendar-install-$$"
trap 'rm -f "$temporary"' EXIT HUP INT TERM
cp "$binary" "$temporary"
chmod 0755 "$temporary"
mv "$temporary" "$install_dir/bin/gallendar"
trap - EXIT HUP INT TERM

if [ ! -f "$config_dir/config.toml" ]; then
    cp gallendar.example.toml "$config_dir/config.toml"
    chmod 0600 "$config_dir/config.toml"
    echo "Created $config_dir/config.toml"
else
    echo "Preserved $config_dir/config.toml"
fi

case "${SHELL:-}" in */bash) profile="$HOME/.bashrc" ;; *) profile="$HOME/.zshrc" ;; esac
if [ "$install_dir" = "$HOME/.x-cli-gallendar" ]; then
    line='export PATH="$HOME/.x-cli-gallendar/bin:$PATH"'
else
    line="export PATH=\"$install_dir/bin:\$PATH\""
fi
touch "$profile"
grep -Fqx "$line" "$profile" || printf '\n# x-cli-gallendar\n%s\n' "$line" >> "$profile"

# A matching loaded LaunchAgent must use the newly installed executable.
# Older releases do not expose this hook; keep explicit historical installs usable.
if [ "$(uname -s)" = Darwin ] &&
   "$install_dir/bin/gallendar" daemon restart --help >/dev/null 2>&1; then
    "$install_dir/bin/gallendar" daemon restart >/dev/null
fi

echo "Installed $install_dir/bin/gallendar"
echo "For this shell: export PATH=\"$install_dir/bin:\$PATH\""
