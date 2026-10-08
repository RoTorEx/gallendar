#!/usr/bin/env sh
set -eu

fail() { echo "ERROR: $*" >&2; exit 1; }
[ "$#" -eq 0 ] || fail "use: make release"
[ "$(git branch --show-current)" = main ] || fail "release must run from main"
[ -z "$(git status --porcelain)" ] || fail "commit changes before releasing"
git remote get-url origin >/dev/null 2>&1 || fail "configure the intended origin repository before releasing"
git fetch origin main --tags
git merge-base --is-ancestor origin/main HEAD || fail "local main is behind or diverged from origin/main"
current="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
printf "Current version: %s\nRelease version (MAJOR.MINOR.PATCH): " "$current"
read -r release_version
printf '%s\n' "$release_version" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' || fail "invalid version"
tag="v$release_version"
! git rev-parse --verify "refs/tags/$tag" >/dev/null 2>&1 || fail "$tag already exists"
python3 - "$current" "$release_version" <<'PYVERSION'
import sys
old, new = (tuple(map(int, v.split('.'))) for v in sys.argv[1:])
if new < old:
    raise SystemExit('ERROR: release version must not decrease')
PYVERSION
make check
python3 - "$release_version" <<'PYUPDATE'
from pathlib import Path
from datetime import datetime
from zoneinfo import ZoneInfo
import re, sys
version = sys.argv[1]
p = Path('Cargo.toml')
s, count = re.subn(r'^version = "[^"\n]+"$', 'version = "' + version + '"', p.read_text(), count=1, flags=re.M)
if count != 1:
    raise SystemExit('ERROR: cannot update package version')
c = Path('CHANGELOG.md')
text = c.read_text()
marker = '## [Unreleased]'
if text.count(marker) != 1:
    raise SystemExit('ERROR: expected exactly one Unreleased section')
date = datetime.now(ZoneInfo('Europe/Warsaw')).date().isoformat()
p.write_text(s)
c.write_text(text.replace(marker, marker + '\n\n## [' + version + '] - ' + date, 1))
PYUPDATE
# Refresh the package version entry in Cargo.lock without updating dependencies.
cargo check --offline
make check
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "build: release $tag"
git tag -a "$tag" -m "Release $release_version"
echo "Prepared $tag; run make release-push to publish the tag"
