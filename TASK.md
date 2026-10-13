# Remaining operator checks

Scope: **Aveni & Thira → Innowise**. Rules: [BUSINESS.md](BUSINESS.md).
Implementation and API validation are recorded in [business/validation.md](business/validation.md).

- [ ] Have a colleague recheck full original titles after the default-visibility fix.
  Busy intervals in the primary destination were already confirmed by the user.
- [ ] Observe a real source move, rename, cancellation and recurrence exception for
  each route. Source edits are performed by the user; the CLI must never write sources.
- [ ] Confirm the configured Aveni colleague's invitation acceptance and busy time,
  then observe updates/cancellation without resetting their response.
- [ ] Exercise Dato's title exclusions with one original and its corresponding copy.
- [ ] Measure source-change-to-destination delay during awake operation against the
  configured ten-minute interval plus pass duration. Reboot persistence is configured
  and launchd recovery tested; an actual reboot remains an operator check.

No repository implementation is queued here. VPS work and additional platforms
remain deferred. Further releases or scope expansion require a user request.
