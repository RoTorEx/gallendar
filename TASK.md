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
- [ ] Measure source-change-to-destination delay during awake operation against the
  configured 30-minute clock-aligned interval plus pass duration. Reboot persistence is configured
  and launchd recovery tested; an actual reboot remains an operator check.

Authorized deployment in progress: Linux x86_64 release, sole VPS writer,
Telegram auth/staleness/recovery alerts, node-tools metrics reconnection in the
bot, and verified patch releases. Additional platforms remain deferred.
