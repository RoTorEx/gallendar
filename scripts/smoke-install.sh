#!/usr/bin/env sh
set -eu

target="${1:?target is required}"
version="${2:?version is required}"
output_dir="${3:?output directory is required}"

case "$target" in
    aarch64-apple-darwin) platform=macos; arch=aarch64 ;;
    x86_64-apple-darwin) platform=macos; arch=x86_64 ;;
    x86_64-unknown-linux-gnu) platform=linux; arch=x86_64 ;;
    *) echo "ERROR: unsupported release target: $target" >&2; exit 1 ;;
esac

temp_dir="$(mktemp -d "${TMPDIR:-/tmp}/gallendar-smoke.XXXXXX")"
trap 'rm -rf "$temp_dir"' EXIT HUP INT TERM
archive="$output_dir/gallendar-$platform-$arch.tar.gz"
checksum="$archive.sha256"
expected="$(awk 'NR == 1 { print $1 }' "$checksum")"
actual="$(shasum -a 256 "$archive" | awk '{ print $1 }')"
[ "$actual" = "$expected" ] || { echo "ERROR: packaged archive checksum mismatch" >&2; exit 1; }

sh scripts/install.sh \
    --archive "$archive" \
    --install-dir "$temp_dir/install" \
    --config-dir "$temp_dir/config" \
    --no-path-update

actual="$($temp_dir/install/bin/gallendar -V)"
[ "$actual" = "gallendar $version" ] || { echo "ERROR: expected gallendar $version, got $actual" >&2; exit 1; }
[ -f "$temp_dir/config/config.toml" ] || { echo "ERROR: installer did not create config" >&2; exit 1; }

printf '# preserve-existing-config\n' > "$temp_dir/config/config.toml"
sh scripts/install.sh \
    --archive "$archive" \
    --install-dir "$temp_dir/install" \
    --config-dir "$temp_dir/config" \
    --no-path-update >/dev/null
grep -Fqx '# preserve-existing-config' "$temp_dir/config/config.toml" || {
    echo "ERROR: installer replaced existing config" >&2
    exit 1
}
echo "Smoke install passed: $actual"
