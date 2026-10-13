# Changelog

## [Unreleased]

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
