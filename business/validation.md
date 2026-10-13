# Hypothesis check and acceptance

Product rules: [BUSINESS.md](../BUSINESS.md).
Queue: [TASK.md](../TASK.md).

## Current results

**Live writes verified; account availability not accepted yet.** The check applies to
Aveni → Innowise and Thira → Innowise; personal accounts are not involved.

On 2026-10-13, by 15:49 Warsaw (CEST, UTC+02:00), the authorized two-day live
trial was applied to the existing Gallendar secondary calendar owned by Innowise.
All three OAuth owners and read/write roles were checked. The global calendar name
resolved uniquely; no new calendar or primary-calendar copy was created.

- Eight copies were read back: four from each source, with exact original titles
  and intervals. Four Aveni copies contain only the configured colleague; four
  Thira copies contain no guests. RSVP is needsAction; receipt and acceptance
  on the colleague's side were not observed in that initial API-only check.
- The first write exposed Google's omission of default opaque transparency.
  The pass stopped after that saved copy; the comparison was corrected and tested.
  Recovery reused it and created the remaining seven without duplicates.
- Repeated dry-run and the first periodic applied pass reported create=0,
  update=0, delete=0, unchanged=8. Periodic CLI run initially used a
  rolling two-day horizon and 60-second polling; no autostart service was installed.
- Later on 2026-10-13, the user changed polling to ten minutes. The installed CLI
  and active config now accept `poll_seconds = 600`; the process was restarted.
  Dry-run and the first applied pass after restart both reported create=0,
  update=0, delete=0, unchanged=8. All 53 Rust tests and the required checks passed.
  The ten-minute gap between subsequent live passes has not been timed independently.
- Independent API read-back matched every managed copy to its source key,
  title, interval and configured guests. Every copy has Innowise as creator
  and Gallendar as organizer. The Innowise primary has no copies in this namespace;
  the two-day source snapshots have no managed copies or configured-colleague attendees.
- The full required checks passed (52 tests), including source and primary
  write rejection before HTTP, create/update/delete destination isolation,
  original-guest preservation, notification requests and RSVP preservation.

A subsequent user-provided screenshot of the colleague's calendar shows an Aveni
copy with its full title, Gallendar as organizer, the Innowise owner as creator,
and the colleague as the only guest with a pending response. This establishes
visibility on the colleague's side, not acceptance or busy-time aggregation. The
user declined adding their own Innowise profile to the guests to keep invitations
out of the primary calendar.

Route prefixes were then applied to the eight existing copies: four with the Aveni
label and four with the Thira label. Independent before/after API reads verified
stable copy IDs, unchanged guest lists and RSVP values, unchanged source snapshots,
and no managed copies in the Innowise primary. A repeated plan had no changes.
The required checks passed with 54 Rust tests, including prefix changes in both
title modes, prefix removal, an empty original title and guest-response preservation.

**Acceptance remains incomplete.** A colleague's real Find a time view, booking
aggregation, invitation acceptance, Dato hiding, live movement/cancellation
and observed synchronization delay have not been checked. These results establish
saved copies and repeatability, not the Innowise account's aggregated busy time.
The requested copies are intentionally retained for the user's live trial.

## Preparation and sequence

1. Establish the exact Calendar IDs of the two sources and the Innowise destination.
   Choose an observer who sees Innowise availability in a real work
   "Find a time" scenario; without that visibility, the check is incomplete.
2. Establish the baseline availability of a free future interval. A control
   Busy in the Innowise primary calendar must block the interval; deleting it must
   free it. Use synthetic data without sending invitations.
3. Create a Busy in the selected Innowise destination calendar and check specifically
   the availability of the Innowise person, not the calendar's visibility in the combined grid.
   For a secondary calendar, also check hiding it for the owner.
4. Check moving and deleting the block, then the booking pages in use
   and other required tools. If the secondary calendar fails,
   propose the primary calendar; do not change the destination automatically.
5. After the destination is confirmed, check the CLI with two routes: creating
   an Aveni original, dry-run, apply, repeated pass, move, and deletion.
   Repeat for Thira. Copies must appear only in Innowise.
6. Check that an existing Innowise meeting is preserved and not copied
   into the sources; Aveni and Thira receive no writes from the service.

## Additional API diagnostics

When access is available, separately query `freeBusy.query` for the primary calendar
and the copies calendar in the test interval. Check each calendar's errors,
not only a successful HTTP response. Do not use a combined request for both
calendars as proof that copies are automatically accounted for when checking the account.

The Google API returns busy time for the specified calendars:
[Freebusy: query](https://developers.google.com/workspace/calendar/api/v3/reference/freebusy/query).
For appointment schedules, checking secondary calendars depends on a suitable
plan and calendar selection:
[Check your availability across calendars](https://support.google.com/calendar/answer/16287054?hl=en-GB).
Documentation serves as preparation; the outcome is determined by real observation.

## Decision

- **GO:** the selected destination blocks Innowise availability in all
  required scheduling methods; reading both sources, writing,
  and both CLI routes are confirmed.
- **NO-GO:** in a required scenario, the account stays free while a
  Busy copy exists. Abandon this design and propose copies in the primary calendars
  with an explicit UX change, without an automatic switch.
- **Incomplete check:** no access, correct observer, or required
  tool. Record the missing evidence; do not set GO.

## Cleanup and protocol

Delete only events created by the test and empty test calendars if they
are not needed further. Restore the original visibility and access settings.
Check that no test blocks remain and that availability is restored.

After the experiment, replace the current results section with the actual protocol:

- Date; time in Europe/Warsaw with the correct seasonal label CET or CEST.
- Account aliases, roles, calendar types, and observer permissions.
- Tools, plans, and settings that affect the check.
- For each route into Innowise: baseline availability, primary calendar control,
  copy, hiding, move, and deletion; expected and actual state.
- Verified service account permissions and specific organizational blockers.
- GO / NO-GO / incomplete result, limits of applicability, and remaining checks.
- Cleanup performed and any intentionally retained test objects.

Do not save secrets or real event content. Do not record future
or assumed results as completed.

## Full MVP acceptance

| # | Scenario | Expected result |
|---|---|---|
| 1 | Aveni meeting | Innowise is busy; the title matches the route mode |
| 2 | Thira meeting | Innowise is busy; details stay only in the source |
| 3 | Move | Copies follow the source; the old interval is free if nothing else occupies it; no duplicates |
| 4 | Cancellation or deletion | The corresponding copies disappear; time not occupied by other events is freed |
| 5 | No feedback loop | A copy does not produce copies; one original produces one copy per destination |
| 6 | Title mode and privacy | Masked shows the title from TOML; original keeps the title and its changes. Other details are not copied; changing the mode updates the copy |
| 7 | Existing Innowise meetings | Preserved unchanged and do not become sources |
| 8 | External availability | A colleague and required tools see the account as busy; failure blocks acceptance of the design |
| 9 | Directions | Both sources block only Innowise; there are no writes to Aveni and Thira |
| 10 | Autonomy | After setup, ordinary changes are synchronized without manual runs or repeated logins |

Additionally go through Busy/Free, Out of Office, the agreed meaning of tentative,
declined invitations, the agreed all-day event rules, changing
and cancelling one recurrence, changing the series, overlapping events, a repeated
pass, a restart, and source unavailability. Measure delay from the source change
to the observed destination result against the configured interval (currently
ten minutes) plus pass duration.
