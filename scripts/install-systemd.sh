#!/usr/bin/env sh
set -eu

if [ "${1:-}" = --help ]; then
    echo "Usage: sudo sh scripts/install-systemd.sh /path/to/gallendar"
    echo "Install the Linux binary and unit; preserve config/tokens; do not start synchronization."
    exit 0
fi
[ "$(uname -s)" = Linux ] || { echo "ERROR: systemd installation requires Linux" >&2; exit 1; }
[ "$(id -u)" = 0 ] || { echo "ERROR: run this installer as root" >&2; exit 1; }
binary="${1:?binary path is required}"
[ -x "$binary" ] || { echo "ERROR: executable binary is missing" >&2; exit 1; }
"$binary" --version
root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
id gallendar >/dev/null 2>&1 || useradd --system --home-dir /srv/gallendar --shell /usr/sbin/nologin gallendar
install -d -o gallendar -g gallendar -m 0755 /srv/gallendar /srv/gallendar/health
install -d -o gallendar -g gallendar -m 0700 /srv/gallendar/.x-cli-gallendar
install -d -o gallendar -g gallendar -m 0755 /srv/gallendar/.x-cli-gallendar/bin
temporary="/srv/gallendar/.x-cli-gallendar/bin/.gallendar-install-$$"
trap 'rm -f "$temporary"' EXIT HUP INT TERM
install -o gallendar -g gallendar -m 0755 "$binary" "$temporary"
mv "$temporary" /srv/gallendar/.x-cli-gallendar/bin/gallendar
install -o root -g root -m 0644 "$root/deploy/gallendar.service" /etc/systemd/system/gallendar.service
systemctl daemon-reload
echo "Installed gallendar.service; configure OAuth and disable auto_update before starting."
echo "Stop every other writer first, then: sudo systemctl enable --now gallendar.service"
