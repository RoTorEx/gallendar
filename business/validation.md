# Hypothesis check and acceptance

Product rules: [BUSINESS.md](../BUSINESS.md).
Queue: [TASK.md](../TASK.md).

## Current results

**Not verified.** Access is not configured yet. The current check applies to
Aveni → Innowise and Thira → Innowise; personal accounts are not needed.

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
pass, a restart, and source unavailability. Measure the usual delay of up to
two minutes from the source change to the observed destination result.
