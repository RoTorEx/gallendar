# Development direction

Current result: **Aveni & Thira → Innowise** — a busy-time mirror of two
work calendars into a third, without meeting content or reverse synchronization.
We stop at this scope. Queue: [TASK.md](TASK.md).

## Implemented

Rust CLI: setup/config/home/version/update, demo, doctor, dry-run, sync, and run.
TOML can define sources, the destination, and explicit [[routes]].
The new template defines Aveni → Innowise and Thira → Innowise.
There is service and user OAuth access, separate per-account login,
and automatic refresh of OAuth access tokens. There is recurrence handling, private Busy copies, read-back, and recovery.
The existing four-account offline demo is kept as an engine check.

## Remaining for a working result

Configure access to the three work accounts, choose the destination calendar,
verify Innowise's real availability and both routes with CLI run running. Autostart is deferred.
Access and operation are not confirmed yet. Public repository:
[RoTorEx/gallendar](https://github.com/RoTorEx/gallendar); releases are available on the
[Releases](https://github.com/RoTorEx/gallendar/releases) page.

The personal calendar, all-to-all synchronization, a web interface, and other providers
are not part of the current stage. Update and automatic updates are part of the current release delivery.
