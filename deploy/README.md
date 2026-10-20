# Linux VPS operation

The supported server is Linux x86_64 with systemd. GitHub Release builds use
Ubuntu 22.04 for compatibility with Debian 13. The Mac remains a manual fallback;
only one machine writes a given namespace. Running the same schedule on multiple
machines is not coordination: new copies have random IDs and can race.

## Install and switch the writer

Download `gallendar-linux-x86_64.tar.gz` and its `.sha256` from the intended GitHub
Release, verify the checksum, and extract it into a private staging directory.
Use this release's repository scripts/unit, then run from its checkout:

```sh
sudo sh scripts/install-systemd.sh /path/to/extracted/gallendar
```

The installer stages the binary and hardened system unit without starting it.
It preserves existing configuration and credentials. The dedicated `gallendar`
user owns `/srv/gallendar/.x-cli-gallendar`; no login shell or Docker socket is
required. The service starts at boot without a desktop login and restarts after
30 seconds on exit. Shutdown allows 120 seconds for requests to finish. Its
256 MiB memory, 20% CPU and 32-task ceilings constrain a lightweight personal
sync without reserving host capacity.

Keep a private recovery copy of the Mac config, OAuth files and launchd plist.
Transfer the config, OAuth client JSON and per-account token directory over SSH,
preserving relative paths. Use the same OAuth client and namespace. Configure
`auto_update = false` on the server: version replacement and service restart are
an explicit release operation. Do not copy unrelated service-account keys.
Config/client/token files must be 0600 and secret directories 0700, owned by
`gallendar`. Never print tokens, copy them into Git, or mount them into the bot.

Before changing the writer, verify the copied configuration and run `doctor`
and `sync --dry-run` as `gallendar` with `HOME=/srv/gallendar`. A preview never
writes Google events. Compare namespace, routes, titles, guests and horizon
against the source config; investigate unexpected deletions before proceeding.

On the Mac run `gallendar daemon stop` and verify `running=false` and
`enabled=false`. Only then enable the server:

```sh
sudo systemctl enable --now gallendar.service
sudo -u gallendar env HOME=/srv/gallendar /srv/gallendar/.x-cli-gallendar/bin/gallendar status --json
sudo systemctl status gallendar.service
```

Verify a successful applied pass and a subsequent no-change preview. Check that
service restart produces a new worker and another success without duplicates.
An enabled unit establishes boot configuration, not a completed host reboot;
do not reboot a shared VPS just to verify this service.

## Health and alerts

The unit publishes `/srv/gallendar/health/health.json` atomically after applied
periodic passes. Set `GALLENDAR_HEALTH_DIR` to select a directory explicitly;
without it no public health snapshot is written. Dry runs and one-shot commands
never advance the health file. It retains last success across restarts and
contains only version, UTC timestamps, interval, safe error kind and account
alias. It excludes credentials, event titles and raw HTTP diagnostics. Snapshot
publication failure stops the worker so stale state cannot claim fresh success.

The existing Telegram bot mounts only this health directory read-only. It
checks every 30 seconds, immediately reports an explicit `reauth_required` or
missing/invalid snapshot, and reports other failures after two poll intervals
without success. A transient network timeout is not a reauthorization request.
At the current 1800-second interval the overdue threshold is one hour, plus
up to 30 seconds for monitoring. Persistent notification state suppresses
repeats across normal bot restarts; recovery sends once after a fresh success.
The bot cannot notify during a total VPS/Telegram-network outage; external
host heartbeat monitoring owns that boundary.

Private journals remain in `/srv/gallendar/.x-cli-gallendar/logs`. Use `journalctl
-u gallendar.service` for supervisor output. `status --json` reads applied
periodic outcomes and the actual worker version; it does not call Google.

## Reauthorize an account

First stop the server with `sudo systemctl stop gallendar.service`. On the Mac,
keep its daemon stopped and run `gallendar auth login --account ACCOUNT` using
the same client/config. Transfer only that account's new token over SSH into
the existing `secrets/oauth/ACCOUNT.json` with atomic replacement, owner
`gallendar` and mode 0600. Other account tokens remain unchanged. Run server
`doctor`, restart the service, and verify a successful pass and recovery alert.
No browser or Google password is stored on the VPS. Corporate policy can
require administrator action instead of another login.

For normal unattended use, verify Google Auth Platform Audience is In production.
Testing tokens for Calendar expire after seven days; changing the status does
not constitute a read-back of existing token validity. Actual token refresh and
calendar reads are checked by `doctor` and applied passes.

## Update or return to the Mac

For an update, stop the server, retain its binary/config for recovery, install
the verified new release, start it and verify health. Automatic server updates
are disabled so a running worker cannot silently keep an old executable.

To return to the Mac, stop and disable the server first, verify its MainPID is
zero, synchronize any rotated/reissued account tokens back over SSH if needed,
then run Mac `doctor`, preview and `gallendar daemon start`. Preserve the same
namespace so existing copies are reconciled rather than recreated. Never start
the Mac while the server remains an active writer.
