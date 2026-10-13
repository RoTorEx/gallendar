# Development direction

Current scope: **Aveni & Thira → Innowise**. Source calendars remain read-only;
copies go to the explicit Innowise primary calendar. Queue: [TASK.md](TASK.md).

## Implemented

Rust CLI with setup/config/home/version/update, demo, doctor, previews, one-shot
sync and periodic runs. TOML defines account credentials, directed routes,
original or masked titles, project prefixes, colleague guests, a rolling window
of one to thirty days and polling seconds. The active config uses five days and
ten minutes. OAuth and service-account authentication are supported; per-account
OAuth logins and refresh are verified.

Reconciliation updates existing copies for changes, preserves stable identities
and guest responses, and verifies saved destination state. Primary copies block
Innowise availability, confirmed by the user's colleagues. The failed secondary
calendar experiment is retired; its deleted calendar is not recreated. Original
mode uses calendar-default visibility; masked mode remains private. Source meeting
context beyond the chosen title is excluded.

A local macOS login daemon supports start/stop/status and graceful reloads,
config reload between passes, crash recovery, persistent autostart, private logs
and applied-sync health reporting. Successful updates reload a matching active
daemon. Public delivery supports Apple Silicon only; config and credentials are
preserved during installation. Repository: [RoTorEx/gallendar](https://github.com/RoTorEx/gallendar).

## Remaining validation and deferred scope

Fresh colleague-side title visibility, Dato filtering, guest invitation acceptance
and live source edits remain operator/observer checks, as listed in TASK.md.
API verification alone does not establish the colleague's actual UI or RSVP.

VPS deployment, pre-login operation, Intel/Linux builds, personal calendars,
reverse or all-to-all synchronization, a web UI and other calendar providers
are deferred. No additional scope is authorized by these directions.
