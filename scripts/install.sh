#!/usr/bin/env sh
set -eu

repo="${GALLENDAR_INSTALL_REPO:-${GALLENDAR_RELEASE_REPO:-}}"
version="${GALLENDAR_VERSION:-latest}"
install_dir="${GALLENDAR_INSTALL_DIR:-$HOME/.x-cli-gallendar}"
config_dir="${GALLENDAR_CONFIG_DIR:-$HOME/.x-cli-gallendar}"
archive_path=""
update_path=1

usage() {
    cat <<'EOF'
Usage: gallendar-install.sh [--version VERSION|latest] [--install-dir PATH] [--config-dir PATH] [--archive PATH] [--no-path-update]

Installs a compatible GitHub Release. Existing configuration is preserved.
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version) version="${2:?missing version}"; shift 2 ;;
        --install-dir) install_dir="${2:?missing install directory}"; shift 2 ;;
        --config-dir) config_dir="${2:?missing config directory}"; shift 2 ;;
        --archive) archive_path="${2:?missing archive path}"; shift 2 ;;
        --no-path-update) update_path=0; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "ERROR: unknown option: $1" >&2; usage >&2; exit 1 ;;
    esac
done

case "$(uname -s)" in Darwin) platform=macos ;; Linux) platform=linux ;; *) echo "ERROR: unsupported operating system" >&2; exit 1 ;; esac
case "$(uname -m)" in
    arm64|aarch64) arch=aarch64 ;;
    x86_64|amd64) arch=x86_64 ;;
    *) echo "ERROR: unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

[ "$platform-$arch" != linux-aarch64 ] || { echo "ERROR: Linux ARM releases are not configured; build from source" >&2; exit 1; }
archive="gallendar-$platform-$arch.tar.gz"
temp_dir="$(mktemp -d "${TMPDIR:-/tmp}/gallendar-install.XXXXXX")"
trap 'rm -rf "$temp_dir"' EXIT HUP INT TERM
token="${GH_INSTALLER_TOKEN:-${GH_TOKEN:-${GITHUB_TOKEN:-}}}"

download() {
    download_url=$1
    download_output=$2
    if [ -n "$token" ]; then
        printf 'header = "Authorization: Bearer %s"\n' "$token" |
            curl --config - -fsSL --output "$download_output" "$download_url"
    else
        curl -fsSL --output "$download_output" "$download_url"
    fi
}

if [ -n "$archive_path" ]; then
    cp "$archive_path" "$temp_dir/$archive"
else
    [ -n "$repo" ] || { echo "ERROR: set GALLENDAR_INSTALL_REPO=OWNER/REPO before downloading" >&2; exit 1; }
    if [ "$version" = latest ]; then
        base="https://github.com/$repo/releases/latest/download"
    else
        base="https://github.com/$repo/releases/download/v${version#v}"
    fi
    download "$base/$archive" "$temp_dir/$archive"
    download "$base/$archive.sha256" "$temp_dir/$archive.sha256"
    expected="$(awk 'NR == 1 { print $1 }' "$temp_dir/$archive.sha256")"
    actual="$(shasum -a 256 "$temp_dir/$archive" | awk '{ print $1 }')"
    [ "$actual" = "$expected" ] || { echo "ERROR: archive checksum mismatch" >&2; exit 1; }
fi

tar -xzf "$temp_dir/$archive" -C "$temp_dir" gallendar gallendar.example.toml
[ -x "$temp_dir/gallendar" ] || { echo "ERROR: archive does not contain gallendar" >&2; exit 1; }
[ -f "$temp_dir/gallendar.example.toml" ] || { echo "ERROR: archive does not contain gallendar.example.toml" >&2; exit 1; }

mkdir -p "$install_dir/bin" "$config_dir"
chmod 0700 "$install_dir" "$config_dir"
cp "$temp_dir/gallendar" "$install_dir/bin/.gallendar-install-$$"
chmod 0755 "$install_dir/bin/.gallendar-install-$$"
mv "$install_dir/bin/.gallendar-install-$$" "$install_dir/bin/gallendar"

if [ ! -f "$config_dir/config.toml" ]; then
    cp "$temp_dir/gallendar.example.toml" "$config_dir/config.toml"
    chmod 0600 "$config_dir/config.toml"
    echo "Created $config_dir/config.toml"
else
    echo "Preserved $config_dir/config.toml"
fi

if [ "$update_path" -eq 1 ]; then
    case "${SHELL:-}" in */bash) profile="$HOME/.bashrc" ;; *) profile="$HOME/.zshrc" ;; esac
    if [ "$install_dir" = "$HOME/.x-cli-gallendar" ]; then
        line='export PATH="$HOME/.x-cli-gallendar/bin:$PATH"'
    else
        line="export PATH=\"$install_dir/bin:\$PATH\""
    fi
    touch "$profile"
    grep -Fqx "$line" "$profile" || printf '\n# x-cli-gallendar\n%s\n' "$line" >> "$profile"
fi

echo "Installed $install_dir/bin/gallendar"
echo "Run: export PATH=\"$install_dir/bin:\$PATH\""
