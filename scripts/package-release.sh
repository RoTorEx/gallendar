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

host="$(rustc -vV | sed -n 's/^host: //p')"
[ "$host" = "$target" ] || { echo "ERROR: packaging target differs from the built host; build on the target runner" >&2; exit 1; }

binary="${CARGO_TARGET_DIR:?CARGO_TARGET_DIR is required}/release/gallendar"
[ -x "$binary" ] || { echo "ERROR: missing release binary: $binary" >&2; exit 1; }
actual_version="$("$binary" -V)"
[ "$actual_version" = "gallendar $version" ] || {
    echo "ERROR: expected gallendar $version, got $actual_version" >&2
    exit 1
}

mkdir -p "$output_dir"
stage_dir="$(mktemp -d "${TMPDIR:-/tmp}/gallendar-package.XXXXXX")"
trap 'rm -rf "$stage_dir"' EXIT HUP INT TERM
cp "$binary" "$stage_dir/gallendar"
cp gallendar.example.toml "$stage_dir/gallendar.example.toml"

archive="$output_dir/gallendar-$platform-$arch.tar.gz"
tar -czf "$archive" -C "$stage_dir" gallendar gallendar.example.toml
shasum -a 256 "$archive" > "$archive.sha256"
echo "$archive"
