# Remaining operator checks

Scope: **Aveni & Thira → Innowise**. Rules: [BUSINESS.md](BUSINESS.md).
Implementation and API validation are recorded in [business/validation.md](business/validation.md).

- [ ] Have a colleague confirm that the current primary-calendar copies block
  time and show the prefixed source title with public event visibility, including
  a colleague whose calendar access is otherwise limited to free/busy.
- [ ] Observe a real source move, rename, cancellation and recurrence exception for
  each route. Source edits are performed by the user; the CLI must never write sources.
- [ ] Confirm the configured Aveni colleague's invitation acceptance and busy time,
  then observe updates/cancellation without resetting their response.
- [ ] Exercise Dato's title exclusions with one original and its corresponding copy.
- [ ] Measure source-change-to-destination delay on the VPS against the configured
  30-minute clock-aligned interval plus pass duration. Systemd boot autostart is
  configured and service restart is verified; an actual host reboot remains an
  operator check.
- [ ] Confirm Google Auth Platform Audience publishing status for unattended OAuth.
  Cloud project access and actual token refresh are verified, but Audience status
  could not be read from the locked desktop/available Cloud APIs.

No repository implementation is queued here. Gallendar 0.2.6 is the sole VPS
writer and the Mac daemon is disabled. Telegram Assistant 6.1.13 reads native
node-tools metrics/history and monitors safe Gallendar health. Both patch
releases and production read-backs are verified; additional platforms remain
deferred.
