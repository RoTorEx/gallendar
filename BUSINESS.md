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
Gallendar secondary calendar. Real scheduling availability remains unverified.
The destination is the separate Gallendar calendar inside Innowise, resolved by name
and verified as owned by Innowise. Its effect on real availability still needs checking. The experiment on two personal accounts was removed from the queue.

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
- The copies calendar is the selected Innowise destination; a secondary calendar
  is allowed only after verifying the account's real availability.

## Behavior

Busy time in Aveni and Thira creates one Busy copy each in Innowise.
Innowise is not a source. The service writes nothing to Aveni and Thira.
Moving and changing duration update the copy; cancellation and deletion remove it.
Repeated passes do not create duplicates. Each route selects a mode:
`mode = "masked"` uses `title` from TOML (default `Busy`),
`mode = "original"` keeps only the source event's title, including an empty one.
With original, the title field is not used. Both current work routes now use original
mode at the user's request, including invitations for the Aveni route. Renaming the source or changing the
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
behavior remains unverified. Synchronization works only while CLI run is running, without autostart.

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
  The separate Gallendar calendar is the organizer; the authenticated Innowise owner
  is the creator, not automatically an attendee. The user explicitly declined inviting
  their own Innowise profile because Google could place an invitation in the primary
  calendar and defeat independent hiding in Dato. Keep the owner out of the guest list.
- The source title is copied only with explicit mode = "original".
  With masked, only the configured title is copied. Source descriptions, locations, attendees,
  meeting links, and other context are not copied in any mode.
- Copies stay private and opaque in both modes; original keeps the title
  but does not change Google Calendar visibility permissions.
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

## Conditional copies calendar design

Production acceptance of a separate copies calendar requires verifying that it
blocks Innowise's real availability. The user selected a separate Gallendar calendar inside Innowise as the destination,
so copies can be hidden independently in Dato. Global `mirror_calendar_name` resolves
this exact name in each destination account on every pass. Only a uniquely named,
owned secondary calendar is accepted; missing or ambiguous names stop the pass,
with no automatic fallback to the primary calendar. `calendar ensure` is the separate,
explicit creation operation and never creates events. Named calendars require destination
OAuth with calendar-list read and calendar-management permissions. Explicit target IDs
remain supported when the global name is absent.
Its availability suitability remains unverified; do not claim the Innowise account is
busy merely because a copy exists in this secondary calendar. Independently hiding copies is a secondary convenience.

The user explicitly removed the availability-acknowledgement launch requirement.
`gallendar run` starts applied synchronization without additional flags;
`run --dry-run` previews. One-shot sync remains read-only unless `--apply` is supplied.
Account availability is still an unverified product outcome, not an execution gate.
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
| 10 | minutes between pass starts by default (600 seconds) | Current user decision, superseding the ANNEX two-minute usual-delay target | Configurable positive integer seconds; zero is rejected to prevent a tight loop. Longer intervals delay updates; measure interval plus pass duration, with no guarantee under failures. Validation tests cover zero, legacy values and ten minutes |
| 2 | busy-time sources | User clarification: Aveni and Thira | Check each source independently |

## Current CLI parameters

The config sets the horizon explicitly: the window starts now, and copies outside the window are deleted
after all calendars are read successfully. This is a limitation of the current CLI; the final
product boundary for past and future remains an open task. Recurrence
identities account for the series and the occurrence's original start.

- horizon_days: 1 to 30 days, with 7 days in the template; the current prepared config uses 5 days.
  User decision: start with one rolling week from now and allow configuration
  up to 30 days. Invalid values are rejected before Google access. Reducing the
  window removes managed copies outside it on the next applied pass; source events
  are preserved. Boundary and rolling-window behavior are covered by Rust tests.
- poll_seconds: positive integer seconds between pass starts; the template and current
  config use 600 seconds (10 minutes). There is no additional product maximum.
  The first pass starts immediately. If a pass takes longer than the interval, the
  next one starts after it completes. Config changes take effect after restarting run.
  Real synchronization delay includes waiting for the next pass and its duration;
  failures can increase it further.
- One writer on one host per namespace/config. A local lock
  prevents parallel runs of a config; there is no distributed lock.
- auto_update: true by default; the installed CLI checks GitHub Release
  in the background during working commands at most once per 24 hours. The interval limits
  network requests; an already running run is not restarted. Setup and details are in the README.
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
