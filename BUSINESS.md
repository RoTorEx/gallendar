# gallendar — product requirements

## Purpose and current stage

A personal self-hosted Rust CLI for one user. Current scope:
**Aveni & Thira → Innowise**. Two work calendars are busy-time
sources; the third work calendar, Innowise, receives Busy copies.
The goal is to block in Innowise the meeting intervals from Aveni and Thira.
The personal calendar, reverse synchronization, and copying between Aveni and Thira
are not part of the current stage.

Implemented: offline demo, access check, dry-run, one-shot and periodic
synchronization. Directions are set through [[routes]] in TOML in the CLI home
directory: ~/.x-cli-gallendar/config.toml. The new template contains two routes into Innowise.
Existing configs without routes keep the previous all-to-all synchronization;
they are not switched automatically. OAuth login and read access are verified for all three
work accounts; eight two-day trial copies were written and read back in the Innowise
Gallendar secondary calendar. The secondary destination failed the real scheduling check: copies exist in Gallendar,
but their intervals remain free in the Innowise primary and absent from a colleague's
scheduling view. The user authorized migration to the explicit Innowise primary
with title exclusions in Dato. Twelve primary copies were applied successfully after
the user deleted the old calendar and its retired cleanup ID was removed. The user
confirmed colleagues can see the resulting busy intervals; their screenshot exposed
private copies hiding titles, corrected by default visibility in original mode. The experiment on two personal accounts was removed from the queue.

Requirement sources: `calendar_busy_sync_mvp_pack.zip` (three documents),
`ANNEX_01_Product_Decisions.txt`, and the plan agreed in the current conversation.
Ambiguities are in [TASK.md](TASK.md), future stages in
[ROADMAP.md](ROADMAP.md).

The current release build platform is macOS Apple Silicon (M chips).
Builds for Intel Mac and Linux are deferred until separately agreed.

## Actors and concepts

- The owner uses three work accounts and configures sharing once.
- Colleagues and booking tools check the availability of a specific account.
- The service reads sources and manages only the Busy copies it created.
- A source calendar contains the original events of one account.
- A copy represents a busy interval with a title according to the route mode.
- The copies calendar is now the explicit Innowise primary destination. Real account
  availability is a separate product check, not a launch prerequisite.

## Behavior

Busy time in Aveni and Thira creates one Busy copy each in Innowise.
Innowise is not a source. The service writes nothing to Aveni and Thira.
Moving and changing duration update the copy; cancellation and deletion remove it.
Repeated passes do not create duplicates. Each route selects a mode:
`mode = "masked"` uses `title` from TOML (default `Busy`),
`mode = "original"` keeps only the source event's title, including an empty one.
With original, the title field is not used. The current work routes use masked
mode with `title = "Busy"` and their existing source prefixes. This keeps source
meeting titles out of destination copies, including invitations for the Aveni
route. Renaming the source or changing the
mode updates the existing copy without creating a new one. Masked does not react
to renaming the source. The new template selects masked with title = "Busy". Each route supports an optional
`prefix`, empty by default, prepended literally to either title mode. The operator
includes the desired separator. Prefix changes update existing copies and preserve
their identities, guest responses and source titles. Both the owner and invited
colleagues see the prefixed title.

Ordinary Busy events, Out of Office, and tentative events are synchronized. Events
marked Free / Available and declined invitations are not synchronized.
The CLI allows both the tentative status and the owner's "maybe" response if the event
is not Free; the final product clarification remains in TASK.md.

All-day events are synchronized only for real unavailability.
Vacation and Out of Office must block time; birthdays, holidays,
informational reminders, and all-day events marked Free must not.
The rule for a conflict between an event's informational meaning and its Busy status
is not defined yet. It must not be silently inferred from the event title. To run the CLI,
the operator explicitly selects the all_day_policy: only Out of Office, or
Busy events excluding the birthday and workingLocation types. This is not automatic
recognition of vacation, holidays, or reminders.

For recurring meetings, the actual state of each occurrence is used:
moving or cancelling one occurrence affects only it; changing the series
updates the resulting schedule.

## Access and autonomy

Access supports two modes: a service JSON key (service_account, the previous
default mode) and user OAuth (oauth) with separate credentials
for each work account. The mode is selected at the TOML root and can be overridden
in [[accounts]]. OAuth requests event read access for sources and write access for destinations,
verifies the owner's email, and refreshes access tokens using saved refresh
tokens. Logging in to one account does not replace another account's credentials. The service
account key is kept when OAuth is selected; the OAuth client JSON is a separate file.
The authorization procedure and expiration terms are in the README.

In service key mode, the user explicitly shares the required calendars with the
service account. The service needs permission to read the required source data
and manage its own copies. Screenshots of the Aveni and Innowise settings showed
external sharing limited to free/busy: for these settings, the current service
mode does not provide the required access. User OAuth requires
permission for the app to act on behalf of the work account; the organization may restrict
that too. Work-user OAuth login and source reads are verified; destination write permission is
reported by the API and verified by eight live copy read-backs; account scheduling
behavior remains unverified. Synchronization uses the foreground run loop or the local macOS launchd login agent.
The user authorized local background operation and reboot/login persistence; VPS work
remains deferred. `daemon start/stop/status` controls it, with stop removing autostart.
Both modes use poll_seconds and reread config before each pass. Periodic passes
align to wall-clock boundaries: the current 30-minute interval runs at :00 and
:30 in Warsaw. The first pass after starting is immediate; a long pass skips
missed boundaries. The supervisor
restarts a failed worker with a 30-second throttle and permits up to 120 seconds
for graceful shutdown; these limits avoid rapid crash loops and allow ongoing
requests to finish. Stop waits up to 125 seconds: the exit timeout plus five seconds
for launchd to unregister the job. Real launchd start/stop and crash recovery checks
protect this lifecycle. Interrupted writes reconcile on the next start. A per-config
lock prevents concurrent foreground/daemon synchronization. The agent starts at
login, not before login, and cannot synchronize during sleep. Private operation
logs remain in CLI home; launchd stdout/stderr logs are private too. Daemon status
reads applied periodic outcomes for its registered config from the private journals:
last_pass, last_success and last_failure include original UTC timestamps, duration
and error. Previews, one-shot commands and unrelated configs are excluded. Old
failures remain visible after recovery; last_pass determines the latest outcome.
`gallendar status` provides the general local overview without Google calls or
automatic updates, even when config is missing or invalid. It distinguishes the
invoked CLI version from the running daemon version, using the current worker PID
and config to find its startup record. Unknown worker versions remain unknown.
Human-readable times use Europe/Warsaw with the date-specific CET/CEST offset;
structured output retains original UTC timestamps. `--json` supports scripts.

Sharing must be verified for reading Aveni and Thira and writing own copies to Innowise. If organization
policy forbids the required access, that is a separate blocker for the specific
account. The simplest alternative for it is proposed; no general fallback
architecture is built in advance.

## Privacy and invariants

- Each route can set `attendees` to a list of colleague email addresses, empty by default.
  Only these configured colleagues are invited to the destination copies; source guest lists
  are never copied or edited. Guest changes update existing copies without new identities.
  Existing responses are preserved, and unchanged passes do not send invitations again.
  Updates and cancellations notify guests, including removal and rolling-window cleanup.
  Destination OAuth is required; the current service-account mode has no delegation.
  A colleague's calendar visibility and busy status depend on Google invitation settings
  and their response. An invitation does not prove that their time is blocked.
  Configured account addresses are rejected as guests to prevent invitation feedback loops.
  The configured destination calendar is the organizer. In primary mode this is
  Innowise itself; the owner does not need a separate self-invitation. The earlier
  request to keep the primary untouched was superseded by the authorized migration
  after the secondary failed availability. Source protection remains unchanged.
- The source title is copied only with explicit mode = "original".
  With masked, only the configured title is copied. Source descriptions, locations, attendees,
  meeting links, and other context are not copied in any mode.
- Copies stay opaque and block availability. Masked copies remain private; original-title copies use default
  event visibility to follow the destination calendar's existing sharing permissions.
  Private copies show only busy time to calendar readers without private-event
  detail access; invited guests can still see the masked title. Google
  `transparency = "transparent"` means Free and is not a title-visibility setting.
  They do not become public or alter calendar ACLs. Existing private copies in original
  mode are updated in place; missing Google default visibility is treated as default
  to avoid repeated writes. Visibility, titles, times and route guests are reconciled
  on every successful pass within the rolling window. Source writes remain forbidden.
- Busy copies never become a source of new copies.
- Original events and unrelated destination events are preserved.
- Named-calendar mode requires source and destination account sets to be disjoint.
  The Google API adapter allows event writes only to the configured resolved destination
  IDs. In named-calendar mode, source and primary calendar mutation attempts are
  rejected before HTTP; legacy explicit-target mode retains its configured destinations.
  Colleagues are attached only to destination copies; source guests remain untouched.
- One original produces at most one corresponding copy per destination account.
- After deletion or a move, the old interval is freed unless it is occupied by
  another active event.
- Correct availability takes priority over visual tidiness.

## Destination and display

The current work setup uses the explicit Innowise primary Calendar ID with no global
`mirror_calendar_name`. Google does not automatically aggregate secondary-calendar
busy intervals when a colleague checks the primary account. On 2026-10-13, direct
free/busy comparisons and the user's colleague screenshot confirmed this failure.
The accepted destination remains the Innowise primary. Current copies use masked
`Busy` titles with route prefixes; Dato can exclude these titles while showing
source originals. Earlier primary copies used full original titles.
Dato's installed 5.8.2 artifact confirms starts-with title matching, but actual
filtering still requires a user-side check. Other clients need their own display rules.

Each destination can list old secondary IDs in `retired_targets` during migration.
All reads must succeed before writes; active copies are verified before cleanup.
Only owned events in the stable namespace are deleted from an owned retired calendar.
Foreign events and calendar containers remain. Cleanup never creates or updates
retired events and cannot target sources, configured primaries or active destinations.
Partial passes are recoverable; cancellation/new invitations replace old guest copies,
so responses to the old invitations do not transfer. Remove retired IDs only after
successful cleanup, or when the user confirms the retired calendar itself was deleted.
A missing or inaccessible retired calendar is never silently ignored. The deleted
Gallendar secondary is no longer in the current config and is not recreated.

Named secondary destinations remain available as an optional explicit configuration,
without primary fallback. They do not fulfill the current account availability goal.

The user explicitly removed the availability-acknowledgement launch requirement.
`gallendar run` starts applied synchronization without additional flags;
`run --dry-run` previews. One-shot sync remains read-only unless `--apply` is supplied.
The user confirmed primary destination availability with colleagues after migration;
the secondary design has a confirmed negative result. The masked title and busy
time still need a fresh colleague-side observation.
Checking real scheduling scenarios remains useful: the presence of a Busy event
or a successful API request does not prove the account is unavailable. A failing
availability scenario calls for an explicit destination decision; the CLI never
switches to primary calendars automatically.

## Operational journal

The CLI automatically writes private daily JSONL logs in its home `logs/` directory.
They contain command/session/pass identity, effective routes and rolling window,
complete copy-action plans, mutation attempts, Google read-back outcomes, errors,
retry interval and graceful shutdown. This journal is local operational evidence;
real titles and configured guest emails are permitted there for the user's review,
but must not be copied into committed protocols. Credentials, authorization URLs,
HTTP headers and bodies are excluded. Log files use 0600 and the directory 0700.
Records are locked across processes and synced before proceeding; a logging failure
stops execution before the next mutation. Daily UTC partitioning retains history
without a deletion policy; cleanup belongs to the owner. Help flags do not start
an operational session. This replaces the previous manually redirected run.log
as the automatic runtime journal; historical run.log is preserved.

## Numeric decisions

| Value | Unit and meaning | Reason and source | Change impact, boundaries, and checks |
|---|---|---|---|
| 1 | user | Personal service, original brief | Multiple users change product boundaries; not in the MVP |
| 3 | work accounts | User clarification: Aveni, Thira, Innowise | Two sources and one destination; both routes are checked |
| 1 | destination per original | User clarification: Innowise only | Verify that nothing is written to Aveni and Thira |
| 30 | minutes between scheduled passes by default (1800 seconds) | Current user decision: half-hour clock boundaries are sufficient, superseding the previous ten-minute interval | Configurable positive integer seconds; zero is rejected to prevent a tight loop. The initial pass runs immediately, and subsequent passes align to UTC epoch multiples, which are :00 and :30 in Warsaw for this interval. Longer intervals delay updates; measure interval plus pass duration, with no guarantee under failures. Tests cover zero and clock boundaries for 10, 15, 20 and 30 minutes |
| 2 | busy-time sources | User clarification: Aveni and Thira | Check each source independently |

## Current CLI parameters

The config sets the horizon explicitly: the window starts now, and copies outside the window are deleted
after all calendars are read successfully. This is a limitation of the current CLI; the final
product boundary for past and future remains an open task. Recurrence
identities account for the series and the occurrence's original start.

- horizon_days: 1 to 30 days, with 14 days in the template and current config.
  User decision: use a rolling two-week window from now and allow configuration
  up to 30 days. Invalid values are rejected before Google access. Reducing the
  window removes managed copies outside it on the next applied pass; source events
  are preserved. Boundary and rolling-window behavior are covered by Rust tests.
- poll_seconds: positive integer seconds aligned to UTC epoch boundaries; the template and current
  config use 1800 seconds (30 minutes), giving :00 and :30 in Warsaw. There is no additional product maximum.
  The first pass starts immediately. If a pass crosses a boundary, the next pass starts
  at the next future boundary. Config changes take effect before the next pass; an interval edit does not shorten an existing wait.
  Real synchronization delay includes waiting for the next pass and its duration;
  failures can increase it further.
- One writer on one host per namespace/config. A local lock
  prevents parallel runs of a config; there is no distributed lock.
- auto_update: true by default; the installed CLI checks GitHub Release
  in the background during working commands at most once per 24 hours. The interval limits
  network requests. Periodic workers check before every pass, respecting the same daily
  limit and the current auto_update setting. Successful replacement by manual update,
  automatic update or installation gracefully terminates a matching loaded daemon so
  launchd relaunches the installed binary with the same arguments and autostart.
  Stopped jobs and unrelated executables are not started or signalled. Failed replacement
  and no-op updates do not restart it; failures to request restart after replacement
  are reported as installed-but-not-restarted. A foreground run is not restarted.
  Atomic replacement/restart ordering is tested; isolated real launchd checks verify
  loaded-job preservation, new executable execution and stopped/unrelated isolation.
  Setup and details are in the README.
- The CLI obtains a new service or OAuth access token on each pass.
  OAuth login waits for the local callback for up to 180 seconds: the limited period leaves
  time to choose an account and does not keep an unfinished login open indefinitely; state is verified.
  The network timeout is 30 seconds per request; an error stops the current pass.

Checks of these boundaries are in the Rust tests: repeated pass, source failure,
recovery after an unknown write result, recurrence exceptions,
all-day in Europe/Warsaw across DST, privacy, and HTTP read-back. Basic commands,
local installation, and release follow the neighbouring CLIs; their owners are the README,
Makefile, and scripts, not the future development plan.

## Acceptance and module map

`src/model.rs` owns event rules and the private payload, `src/engine.rs`
owns copy reconciliation, and `src/google.rs` owns the API and read-back.
[business/validation.md](business/validation.md)
contains the hypothesis protocol, GO / NO-GO criteria, and ten MVP acceptance scenarios.
Behavior decisions belong to this document; the verification procedure belongs to the module.

## Outside the MVP

Web interface, mobile app, non-Google providers, multi-user
platform, billing, subscriptions, teams, organizations, general account management,
and extended configuration not needed for the two Aveni/Thira → Innowise routes.
