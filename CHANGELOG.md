# Changelog

## [Unreleased]

- Original-title copies now inherit destination calendar visibility instead of being
  forced private, repairing existing managed copies in place while preserving source
  protection, IDs and guest responses. Masked copies remain private.
- Include applied periodic-pass success/failure history in daemon status, excluding
  previews and other configs. Align route template ordering and current documentation.

- Restart a matching loaded macOS daemon after successful manual or automatic updates
  and installation, preserving its configuration and autostart. Stopped jobs remain stopped.
- Check automatic updates before periodic passes, respecting the existing daily limit
  and reloaded config. Add `daemon restart` for explicit graceful reloads.

## [0.2.2] - 2026-10-13

- Added local macOS `daemon start/stop/status` using a persistent login LaunchAgent,
  the installed binary and an absolute config path. Start is idempotent; stop removes
  autostart. launchd supervises crashes while the existing config lock prevents overlap.
- Periodic runs now reload config before each pass and handle SIGTERM gracefully.
  Daemon timing uses poll_seconds; private launchd output logs accompany daily JSONL logs.

## [0.2.1] - 2026-10-13

- Added destination `retired_targets` for recoverable migration from secondary
  mirrors to an explicit primary calendar. Read all calendars before writing,
  verify new copies before deleting old managed copies, and preserve foreign events.
  Retired calendars permit deletion only; source, primary and active IDs are excluded.
- Fixed availability diagnostics to query the account primary even for destination-only
  accounts. Documented the failed secondary availability check, primary configuration,
  invitation replacement and Dato title exclusions for mirrored prefixes.

## [0.2.0] - 2026-10-13

- Simplified launch to `gallendar run` with writes enabled by default; `--dry-run`
  previews and one-shot `sync` still needs `--apply`. Removed availability acknowledgement
  and live-trial flags without changing destination-only write protection.
- Added private daily JSONL logs in CLI home with command/pass identities, full action
  plans, per-operation attempts and verified results, errors, retries and graceful stops.
  Durable logging failures stop further writes; credentials and authorization URLs are excluded.

- Added per-route literal title prefixes for original and masked modes; changing
  a prefix updates the existing copy and preserves its identity and guest responses.

- Set the template polling interval to 600 seconds (10 minutes); `poll_seconds`
  accepts positive integer seconds instead of the former 120-second limit.
  Restart a running CLI after changing the interval.

- Enforced one-way account separation for named calendars and destination-only
  writes at the Google HTTP boundary. Added create/update/delete and original-guest
  preservation checks so named-calendar mode cannot write to source accounts or primary calendars.

- Fixed live Google read-back: omitted transparency means the documented opaque
  default, preventing false mismatches and repeated repairs of already busy copies.

- Added an explicit live-trial apply mode restricted to named secondary calendars;
  it records unacknowledged availability without marking the scheduling check accepted.

- Added global `mirror_calendar_name` for an owned secondary destination found by
  exact name on every pass; missing, duplicate or primary matches stop sync without
  writes. Added explicit idempotent `calendar ensure` and `sync --details` previews.
  Named destinations request additional OAuth scopes only for destination accounts.

- Added per-route colleague email lists (`attendees`) for destination OAuth copies.
  Guest changes update existing copies; invitation updates and cancellations are sent,
  existing RSVP values are preserved, and unchanged passes do not re-invite guests.
  Source attendees are never copied. Service-account destinations and invitation
  feedback into configured accounts are rejected when guests are configured.

- Start with a configurable 7-day rolling synchronization window and reject
  horizons outside 1–30 days before accessing Google.

- Added per-account OAuth Desktop login: auth login/status, PKCE and state,
  email and scope verification, separate private refresh tokens and their refresh.
  TOML selects oauth/service_account globally or per account; the service key
  and existing configs are preserved. Synchronization uses the calendar token.

- Future release builds are limited to macOS Apple Silicon (M chips);
  Intel Mac and Linux were removed from the CI matrix. Published releases are unchanged.

## [0.1.1] - 2026-10-13

- Fixed automatic updates: the version check uses the public GitHub Releases
  redirect instead of the API, so it does not depend on the anonymous API request limit
  and does not require a GitHub token.

## [0.1.0] - 2026-10-13

- Configured the public repository RoTorEx/gallendar; update and the installer use
  it by default. Added a daily background update check when working commands
  start (auto_update in TOML), an error log, a lock against parallel
  updates, and a ban on downgrades. Config and keys are preserved.
- Added build and install checks on main without publishing; a tag releases
  assets for macOS Apple Silicon, Intel, and Linux x86_64.

- Narrowed the current scope to Aveni & Thira → Innowise. Added explicit
  [[routes]] in TOML and a template with two sources and one destination; doctor and sync
  use only the roles of the selected routes. Old configs without routes
  keep the previous behavior. Each route supports masked with a
  configured title and original with the source title; changing the mode updates copies.
  Real access is not configured yet.

- Added a local build via `make build-local` into `./gallendar` and a step-by-step
  guide for connecting Google Cloud and two personal Google Calendar accounts.

- Added a working Rust CLI: setup/config/home/version/update, demo, doctor,
  dry-run, sync, run, and free/busy diagnostics.
- Added Busy copies with private markers, invitation filtering, handling of
  expanded recurrences and exceptions, read-back, and recovery after failures.
- Added native Rust checks, local installation, packaging and smoke install,
  interactive release/release-push, and a GitHub Release workflow like the neighbouring CLIs.

- Recorded the Cross-calendar Busy Sync requirements from the original MVP and ANNEX 01.
- Added a queue for the hypothesis check and ambiguities, development stages,
  a manual GO / NO-GO protocol, and ten acceptance scenarios.
- Refined the first experiment: two personal accounts; access is not configured yet.
- Kept the document whitespace check in the extended `make check`.
