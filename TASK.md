# Current tasks

Scope: **Aveni & Thira → Innowise**. Rules: [BUSINESS.md](BUSINESS.md).

## 1. Connect the work calendars

- [ ] Establish the owner emails and Calendar IDs of the Aveni and Thira sources and
  the Innowise destination; choose the primary or secondary destination calendar.
- [ ] Create an OAuth Desktop app client and log in to the work accounts via auth login.
  Aveni and Innowise limit external sharing to free/busy; the service key
  is installed, but the currently required sharing permissions are unavailable for them. Check
  corporate OAuth permission and Thira's restrictions.
- [ ] Fill in ~/.x-cli-gallendar/config.toml from the new template with two routes.
  Keep the key outside the repository. setup does not overwrite an existing config.
- [ ] Run doctor and dry-run; verify that writes are planned only in Innowise.

## 2. Verify the result and launch

- [ ] Verify Innowise availability from a real work scenario according to
  [business/validation.md](business/validation.md).
- [ ] Go through creation, repeated pass, move, deletion, recurrences, and
  restart separately for Aveni → Innowise and Thira → Innowise.
- [ ] Agree on all-day, tentative, the horizon, and how to detect a stop.
- [ ] Measure the delay of up to two minutes with CLI run running. Autostart is deferred for now.

Further releases and scope expansion require a separate request.
The current request authorizes the public RoTorEx/gallendar, the first release v0.1.0,
and installation from GitHub with working update and automatic updates.
