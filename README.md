# gallendar

A Rust CLI that mirrors busy time **Aveni & Thira → Innowise**.
Originals stay in the two sources; Innowise receives busy copies with a configured title.
Moves and deletions are reflected in the copies; nothing is written back.

Routes are defined in `~/.x-cli-gallendar/config.toml`. Run `gallendar setup`
for a new template or `gallendar config --edit` for an existing one.
Setup preserves an existing config; the new template is [gallendar.example.toml](gallendar.example.toml).
Aveni and Thira set `source` and `owner_email`; Innowise sets `target` and `owner_email`.
The exact IDs, access, and destination type still have to be configured and verified.

```toml
[[routes]]
from = "aveni"
to = "innowise"
mode = "masked"
title = "Busy"

[[routes]]
from = "thira"
to = "innowise"
mode = "masked"
title = "Busy"
```

`mode = "masked"` displays the `title` value, for example `Busy`, `Unavailable`, or
`Aveni — busy`. Titles can contain any Unicode text. `mode = "original"` keeps the source meeting's title;
`title` is ignored in this mode. Source descriptions, attendees, locations, and links
are not copied. Copies keep Google visibility = private; whether the title is
visible depends on the viewer's permissions. Changing the mode or title updates
existing copies on the next pass. Without mode/title, masked/Busy is used.

Each route also accepts `prefix`, empty by default. It is prepended literally in
both modes: `prefix = "[Aveni] "` with an original title `Standup` displays
`[Aveni] Standup`. Include the desired space or separator in the prefix. Changing
or removing a prefix updates existing copies without changing their identities
or the source events; invited colleagues see the same resulting title.

Use a globally named destination calendar to keep copies separate in calendar viewers
such as Dato:

```toml
# Root section, before [oauth], [[accounts]], or [[routes]]:
mirror_calendar_name = "Gallendar"
horizon_days = 2 # Current live trial; configurable from 1 to 30.
poll_seconds = 600 # Every 10 minutes; restart run after changing it.

# In the innowise account, omit target or use target = "".
# In both routes, use mode = "original" for the complete source title.
```

The CLI searches the destination account's calendar list for this exact name,
including hidden calendars. It requires ownership of a separate calendar and rejects
multiple matches or a primary-calendar match. The name takes precedence over explicit
target IDs. Names are resolved on each pass; missing calendars stop doctor and sync
without creating anything. No fallback writes to the primary calendar occur.
Without `mirror_calendar_name`, explicit target IDs continue to work.

Named destinations add calendar-list read and calendar-management permissions to the
destination's OAuth login only. After enabling the setting, run:

```bash
gallendar auth login --account innowise
gallendar calendar ensure --account innowise
gallendar doctor
gallendar sync --dry-run --details
```

`calendar ensure` reuses an existing uniquely named calendar or explicitly creates
one in Europe/Warsaw and reads its metadata back. It never creates events. Source
logins and service-account support for explicit target IDs are preserved. For an explicitly authorized live verification before real account availability is
accepted, use `gallendar sync --apply --live-trial --details`. This requires a named
secondary calendar and records availability as unacknowledged; it does not establish GO.
Ordinary production apply still requires `--availability-confirmed`.

A dry-run
with `--details` shows mirrored titles, intervals, destination IDs and configured
invitees. Full source titles are visible to invited colleagues when using original mode.
Google treats the separate calendar as organizer and the authenticated owner as creator.
The owner is not automatically a guest. Inviting their own profile could place
an invitation in their primary calendar, so configured account addresses remain
excluded from route attendees to preserve calendar separation.
Separate-calendar visibility in Dato and real account availability are different checks;
hiding Gallendar in Dato does not prove that scheduling tools block the account.

`from` and `to` refer to a `name` in `[[accounts]]`. Only route sources are
read; copies are managed only in their destinations. An empty route list,
unknown names, duplicates, and self-routes are rejected. Without `routes`, old
configs keep the previous "all to all" behavior; for the current scope, define
both routes explicitly. If a route is removed, its copies are deleted on the next
successful pass as long as the destination remains in another route. If a destination
is excluded entirely, it is no longer read and its old copies remain;
plan their cleanup separately. Keep the namespace stable.

## Install from GitHub

```bash
curl -fsSL https://github.com/RoTorEx/gallendar/releases/latest/download/gallendar-install.sh -o /tmp/gallendar-install.sh
sh /tmp/gallendar-install.sh
rm /tmp/gallendar-install.sh
gallendar version
gallendar update
```

The installer preserves an existing config and verifies the binary's SHA-256 and version.

## Quick start

```bash
make build-local
./gallendar demo
make run ARGS=demo
make install-local
gallendar --help
gallendar version
gallendar demo
```

`demo` runs without keys or network: four synthetic sources produce 12 copies,
a repeated pass makes zero changes, a move makes three updates, and a deletion makes three
deletions. It checks privacy and stopping before any write on a read error.
This demonstrates the engine; it does not confirm Google behavior.

`make install-local` installs the binary into `~/.x-cli-gallendar/bin`, preserves
an existing config, and adds PATH to the shell profile, like the neighbouring CLIs.
For an already open terminal: `export PATH="$HOME/.x-cli-gallendar/bin:$PATH"`.

## Basic commands

- `setup` (aliases `init`, `setup-config`) — creates the template and, in an interactive terminal, opens
  it in an editor; `setup --no-edit` suits scripts. An existing config
  is preserved. `EDITOR`/`VISUAL` sets the path to the editor executable.
- `config` — config path; `config --edit` opens it in an editor.
- `home` — the application home directory `~/.x-cli-gallendar`.
- `version`, `-V`, `--version` — installed version; `--help` — help.
- `update` — updates the current binary from the latest public
  [GitHub Release](https://github.com/RoTorEx/gallendar/releases) with
  SHA-256 and version verification, atomic replacement, and no downgrades.
  Config and keys are preserved. `--repo OWNER/REPO` and `GALLENDAR_RELEASE_REPO`
  explicitly select another repository.

Automatic updates are enabled via `auto_update = true` at the TOML root (the
default for old configs is also true). When doctor, sync, run,
or availability starts with a valid config, the installed CLI launches a background
check at most once per 24 hours through the public latest-release page,
without a GitHub API token. The release number is checked; the archive is downloaded
only for a new version. Errors do not interfere with the main command and are saved to
`~/.x-cli-gallendar/update.log`; an attempt counts even on error, so the
network request is not repeated on every start. `auto_update = false` disables
this behavior. Help, version/home/config/setup, and the offline demo do not access
the network. Local development binaries are not updated automatically.
A new version is used from the next start; an already running run continues with
the current version. Automatic updates do not create a system service or schedule.


The basic interface and delivery are aligned with the neighbours `linkdeck`, `hop`, `shelve`.

## Authorization: service key or OAuth

Both methods are supported. An old config without `auth` uses the service key;
an existing `service_account_key` is preserved. At the TOML root, choose:

```toml
auth = "service_account"
service_account_key = "secrets/service-account.json"
```

This mode requires sharing sources with event-detail read access and the destination with write access.
In the checked Aveni and Innowise settings, external sharing is limited to free/busy;
in that case the service key does not suit the current reading of events and writing
of copies. The key was verified successfully, but calendar access is not confirmed.

For user OAuth, create in the same Cloud project, in Google Auth platform:
[Branding](https://console.cloud.google.com/auth/branding) → Get started,
audience External; [Audience](https://console.cloud.google.com/auth/audience) →
Test users for the first login or In production for regular operation.
In [Clients](https://console.cloud.google.com/auth/clients), choose Create client →
Desktop app and download the JSON. This is a different file from the service account key.
Store it in `~/.x-cli-gallendar/secrets/oauth-client.json` with permissions 0600.
API access may require approval from the work organization's administrator.

In an existing config, change the root `auth` and add `[oauth]` before
the first `[[accounts]]`. Other root parameters must stay above `[oauth]`:

```toml
auth = "oauth"
# Other root parameters, including service_account_key, namespace, and horizon_days.

[oauth]
client = "secrets/oauth-client.json"
tokens_dir = "secrets/oauth"

# Followed by the existing [[accounts]] and [[routes]].
```

Relative paths are resolved from the config.toml directory. Fill in the real
`owner_email` for the three accounts and keep the explicit routes. Login is possible before
the Calendar IDs are filled in; doctor/sync/run require valid IDs.

```bash
gallendar auth login --account aveni
gallendar auth login --account thira
gallendar auth login --account innowise
gallendar auth status
gallendar doctor
gallendar sync --dry-run
```

In each browser window that opens, choose the corresponding work account.
The CLI checks the email and granted scopes before saving. Sources request
event read access, destinations request event write access; free/busy
and email verification are also required. OAuth uses PKCE, state, and a local callback on 127.0.0.1.
`--no-browser` prints the link without opening a browser; it must be opened on the same
computer. Waiting for login is limited to 180 seconds. Cancellation, a wrong account,
or insufficient scopes keep the previous local tokens.

For mixed access, set `auth = "oauth"` or `auth = "service_account"`
inside the relevant `[[accounts]]`: this overrides the root mode for that account only.
`auth status` checks the presence and consistency of local credentials; it does not
verify that Google access is valid. It prints no secrets and does not access the network.

Each account is stored in a separate file `tokens_dir/<name>.json` (0600),
the directory is 0700. The refresh token is used to obtain a new access token on
each pass; the browser is not opened again. Logging in to another account keeps
the other tokens. In Testing mode, Google limits the Calendar refresh token
to seven days; In production removes this limit, but access revocation and corporate
restrictions may require logging in again.
[Google expiration rules](https://developers.google.com/identity/protocols/oauth2#expiration).

To move to a server, log in on the Mac and transfer the OAuth client,
the token directory, and the TOML over SSH, keeping permissions and relative paths. Do not run two
writers of the same namespace at the same time. Prebuilt server builds are deferred for now;
the current delivery platform is Apple Silicon. There is no autostart: run works while
the process is alive. After verifying the destination's real availability:

```bash
gallendar run --apply --availability-confirmed
```

Optional colleagues are configured independently for each route:

```toml
[[routes]]
from = "aveni"
to = "innowise"
mode = "masked"
title = "Busy"
attendees = ["colleague@example.com", "another@example.com"]
```

Keep the separate Thira route; leaving its attendees empty keeps its copies without guests.
`attendees = []` is the default. Only these configured emails are invited to destination copies;
original meetings and their guest lists are untouched. Use `mirror_calendar_name` to send both routes into a separate calendar by name,
or configure its explicit Calendar ID as `target` if using the earlier OAuth scopes.
A secondary calendar's effect on Innowise availability must be verified before applying the plan.
The existing Innowise OAuth login already
has the required event-write scope; no colleague login or key is needed. Destination OAuth is
required because this service-account mode does not implement domain-wide delegation.
Plain email addresses are required; duplicates and configured account addresses are rejected
(the latter prevents invitation feedback into source calendars).

On the next applied pass, new colleagues are invited to every copy in the current window,
including existing copies. Removing an email removes that guest. Source moves, cancellations,
and deletion of copies outside the window send updates or cancellations to guests. Google
requests use `sendUpdates=all` whenever guests are involved; without guests they keep `none`.
Unchanged copies send no repeated invitations. Existing RSVP values are preserved during
updates, and a decline is not forcibly accepted. Google invitation settings and the colleague's
response determine whether the event appears and blocks their calendar:
[Google Calendar invitation behavior](https://developers.google.com/workspace/calendar/api/v3/reference/events/insert).
Guests see the configured mirrored title and interval; no Google Meet link is added.

After editing the TOML, review `gallendar sync --dry-run`, then use the existing applied-sync
command after the availability check. Restart an already running `gallendar run` to load changes.

## Reference Google guide for the earlier experiment

This guide is kept for reference on Google Cloud and sharing setup.
Its two-way TOML and personal experiment belong to the earlier scope.
For the current scenario, use the new template above: read Aveni/Thira,
write only Innowise, and verify with [business/validation.md](business/validation.md).

We start with two personal Google accounts A and B. If there is no second personal account
yet, it will be needed to check availability from another user's side.
Work accounts are added after the first experiment. External access is not
configured yet; the commands below are run by the owner in their own accounts.

### 1. Check the local binary

In the project directory:

```bash
make build-local
./gallendar --version
./gallendar demo
```

This is an optimized build for the current Mac, without a release commit, tag, or
publication. The `./gallendar` file is not included in Git. To install into PATH:

```bash
make install-local
export PATH="$HOME/.x-cli-gallendar/bin:$PATH"
gallendar --version
```

After source changes, `make build-local` updates `./gallendar`, and
`make install-local` updates the installed copy. The commands below use `gallendar`;
`./gallendar` from the project can be used instead. Both copies share the config.

### 2. Create a Google Cloud project and enable the API

Open the [Google Cloud Console](https://console.cloud.google.com/) as personal
account A. In the project picker, choose New project, set a name, for example
`gallendar-personal`, create the project, and make sure it is selected.

In APIs & Services → Library, find **Google Calendar API** and click Enable.
[API page](https://console.cloud.google.com/apis/library/calendar-json.googleapis.com).
The CLI itself runs locally; there is no need to create a VM or Cloud Run for this step.
[API enablement documentation](https://developers.google.com/workspace/calendar/api/quickstart/python#enable_the_api).

### 3. Create a service account and download the JSON

Open IAM & Admin → Service Accounts → Create service account.
Set a name, for example `gallendar-sync`, and finish creation. Leave additional
Cloud project access roles empty at this stage: event access
is granted in Google Calendar. Copy the new account's email, of the form
`gallendar-sync@PROJECT_ID.iam.gserviceaccount.com`.
[Creating a service account](https://docs.cloud.google.com/iam/docs/service-accounts-create).

Open this account → Keys → Add key → Create new key → JSON → Create.
The browser downloads a JSON file. It contains a private key: keep it local,
do not send it in chat, and do not include it in Git.
[Creating a JSON key](https://docs.cloud.google.com/iam/docs/keys-create-delete).

If creating a service account or key is forbidden by organization policy,
save the error text and stop this step. This is a concrete blocker;
do not change corporate restrictions just to continue the guide.

### 4. Put the key in the application directory

In the terminal, replacing the file name with the one actually downloaded:

```bash
mkdir -p "$HOME/.x-cli-gallendar/secrets"
chmod 700 "$HOME/.x-cli-gallendar/secrets"
mv -n "$HOME/Downloads/DOWNLOADED_KEY_NAME.json" \
  "$HOME/.x-cli-gallendar/secrets/service-account.json"
chmod 600 "$HOME/.x-cli-gallendar/secrets/service-account.json"
```

`mv -n` keeps an already existing key. If one exists, make sure the correct
service account is selected before using another key. The config path
may be absolute; `~` is not expanded in TOML. The path
`secrets/service-account.json` is resolved from the `config.toml` directory.

### 5. Prepare calendars in both accounts

Open [Google Calendar](https://calendar.google.com/) in the browser separately
as A and B. In each account, on the left next to Other calendars, click `+` → Create new
calendar. Name the calendar, for example `gallendar Busy — A` / `gallendar Busy — B`.
Create it as the account owner so that the calendar belongs to them.
[Creating a calendar](https://support.google.com/calendar/answer/37095).

Then, in each account, open Settings and sharing of the relevant calendar →
Shared with / Share with specific people or groups → Add people and groups.
Add the **service account email** from step 3 and grant:

| Calendar | Permission for the service account |
|---|---|
| Primary calendar with originals | See all event details — read events |
| New copies calendar | Make changes to events — change events |

Repeat for A and B. The service account does not require a manual Gmail login;
the CLI uses the downloaded key. Actual access is verified in step 7.
[Sharing permissions](https://support.google.com/calendar/answer/37082?hl=en).

In the settings of each of the four calendars, open **Integrate calendar →
Calendar ID** and copy the ID. The primary ID often equals the owner's email;
a secondary calendar ID usually ends with `@group.calendar.google.com`.
Use exactly the Calendar ID field, not the iCal URL or the browser link.
[Where to find the Calendar ID](https://support.google.com/calendar/answer/44105?hl=en-GB).

For the experiment, additionally share A's primary calendar with account B,
and B's primary with A, with the **See only free/busy** permission. These are viewer permissions,
separate from the service account's permissions. They are needed to compare guest availability.
[Checking another user's availability](https://support.google.com/calendar/answer/6294878).

### 6. Fill in the local config

```bash
gallendar setup --no-edit
gallendar config
nano "$HOME/.x-cli-gallendar/config.toml"
```

If the config already exists, setup preserves it. Fill it in following the sample,
replacing all `REPLACE` values with real emails and Calendar IDs:

```toml
service_account_key = "secrets/service-account.json"
namespace = "gallendar-personal"
horizon_days = 7
poll_seconds = 600
all_day_policy = "out_of_office_only"

[[accounts]]
name = "personal-a"
owner_email = "REPLACE-A@gmail.com"
source = "REPLACE-A@gmail.com"
target = "REPLACE-A@group.calendar.google.com"

[[accounts]]
name = "personal-b"
owner_email = "REPLACE-B@gmail.com"
source = "REPLACE-B@gmail.com"
target = "REPLACE-B@group.calendar.google.com"
```

owner_email is the person's email, not the service account's. source is that person's
originals calendar; target is their separate copies calendar. Do not use
`primary`: for a service account it refers to a different calendar.

Ordinary events with timed start and end suit the first test.
`out_of_office_only` skips ordinary all-day events and keeps all-day events
only of the Out of Office type. If ordinary Busy all-day events need to be copied, the
operator explicitly chooses `busy`; the birthday and workingLocation types are still
excluded. The values 7 days and 600 seconds (10 minutes) are configurable parameters.
Keep the namespace stable after synchronization starts.

By default, the config is at `~/.x-cli-gallendar/config.toml`.
`GALLENDAR_CONFIG` sets another path, and `--config PATH` takes precedence over it.

### 7. Check the integration without writing

```bash
gallendar doctor
gallendar sync --dry-run
```

Expected: `ready: true`, source_readable and target_writable for both accounts.
A source may have reader; a destination needs writer or owner
(writerWithoutPrivateAccess is also supported). doctor checks roles,
but does not create events yet and does not prove account availability.

Dry-run shows create/update/delete/unchanged. These are events from both sources
over the whole selected horizon, so create may be more than one.
Before writing, check that the correct source calendars and destinations are configured.
Original content does not appear in the output.

Typical causes of errors:

| Result | What to check |
|---|---|
| Config error / placeholders | All REPLACE values, emails, explicit Calendar IDs, and required fields are filled in |
| Cannot read service-account key | The file exists, the path is resolved from the config, TOML does not use `~` |
| Invalid service-account JSON | The downloaded JSON is the service account's |
| Authorization error | The key is valid, the service account is enabled, the system clock is correct |
| HTTP 403 when reading events | Calendar API is enabled in the key's project; source/target have the correct permissions |
| HTTP 404 | The Calendar ID is correct; the calendar is shared with the right service account |
| ready: false | The source has event read access and the target has event change access |
| Another gallendar process | Stop run for this config before a separate sync |

For an unknown error, save the command output without the JSON key content;
do not infer integration readiness from UI settings alone.

### 8. Check the availability hypothesis

Choose a free future interval within the horizon. Create a draft
event in A, add B as a guest, and open Find a time; do not send the invitation.
Check B's row specifically, not the combined calendar grid.

First create a temporary Busy in B's primary calendar: A should see B
as busy. Delete the control event and make sure B is free again.
Then create the same temporary `Busy` only in B's copies calendar and repeat
the observation from A. Check hiding the copies calendar, moving, and deleting the block;
repeat in the opposite direction. Booking pages in use are checked
separately. Full protocol: [business/validation.md](business/validation.md).

If B stays free, this is a NO-GO for secondary calendars: authorization may
work, but the main product result is not achieved. Do not confirm
availability-confirmed; first reconsider the destination of the Busy copies.

Remove the manual test blocks before synchronization. The CLI does not manage manually
created events without its private markers. Success of this check on personal
accounts does not replace checking the work accounts.

### 9. Check writing and enable periodic operation

Only after GO, create an ordinary Busy event in source A, without guests,
in a free future interval. Review the dry-run and then apply:

```bash
gallendar sync --dry-run
gallendar sync --apply --availability-confirmed
```

`Busy` should appear in B's copies calendar; the source keeps the original.
The CLI confirms read-back; additionally look at the event in Google Calendar
and at B's availability from the other account. A repeated sync does not create a duplicate.
Move the original, repeat sync, and check that the copy moved; delete the original,
repeat sync, and check that its copy disappeared. Repeat B → A.

After that, run in the terminal:

```bash
gallendar run --apply --availability-confirmed
```

The process works while it is open and running; Ctrl-C stops it. For background
operation after closing the terminal, a separate launch setup on the Mac
or a VPS will be needed — a local foreground run does not provide this by itself.

The availability diagnostic command requires --account NAME and --start/--end
in RFC3339 with an explicit UTC offset. For user times, use
Europe/Warsaw and the offset for the selected date. It shows source busy time
and copies busy time separately, without proving the combined account availability.

`--json` gives machine-readable JSON for diagnostics and paths/version; run emits one
JSON object per line. update and opening the editor do not support this flag.

## Synchronization and hypothesis check

Manual experiment: [business/validation.md](business/validation.md).
Current checks and ambiguities: [TASK.md](TASK.md).
After the external availability check, the write commands are available:

```bash
gallendar sync --apply --availability-confirmed
gallendar run --apply --availability-confirmed
```

The flag confirms the check performed by the operator; the CLI cannot prove
"Find a time" behavior on behalf of a colleague. Without `--apply`, both modes only read and
show change counts. `run` starts passes at the configured interval,
writes errors to stderr, and retries on the next pass; Ctrl-C ends the process.
The authorization key is refreshed at the start of each pass.

`availability --account NAME --start RFC3339 --end RFC3339` shows the busy time
of the source and of the copies calendar separately. Times require an explicit UTC offset;
this request does not confirm the combined account availability.

The engine expands recurrences through the Google API, accounts for moved and cancelled
occurrences, and does not copy Free events, invitations declined by the owner, birthday, or
workingLocation. Tentative events and the owner's "maybe" response are allowed
if the event is not Free. All-day events require an explicit policy:
`out_of_office_only` or `busy` (the second option includes ordinary Busy all-day events).
All-day copies are represented as a timed interval from midnight to midnight in the
source's time zone, so that different destination calendar time zones do not shift busy time.

Mapping state is stored in private markers on the Google copies with hashes,
without source IDs or content; the copy title is set by the route. A repeated pass finds previously saved
copies after a lost response or a restart. Only copies of its own
namespace are managed; source and unrelated events are not changed.

All sources and destinations are read with all pages before writing. A read
error cancels the whole plan. After each write, the result is read back;
on error, the pass stops, already saved changes remain, and the next
pass restores the mappings. New busy time is created before stale
copies are deleted. Run one instance on one host with one config:
a file lock prevents parallel runs of this config; there is no distributed
lock between VPSs.

The protected window runs from the current moment `horizon_days` ahead. Copies outside it
are deleted on a successful pass. Set `horizon_days` to an integer from 1 to 30;
the template starts with 7 days. Values outside that range are rejected before Google access.
`poll_seconds` sets a positive integer interval between pass starts; the template uses
600 seconds (10 minutes). The first pass starts immediately. If a pass takes longer
than the interval, the next starts after it completes. Stop and restart `gallendar run`
after changing the config. A source change can wait up to the configured interval
plus pass duration under normal operation; real delay remains to be measured.

## Checks and build

```bash
make install       # dependencies from Cargo.lock
make check         # fmt-check, check, clippy, tests, release build, shell syntax
make build
make build-release
make build-local   # optimized binary in ./gallendar without publication
make test
make lint
make fmt
make version
make smoke-install # archive + SHA-256 + install into an isolated directory
```

Builds and packages are in `~/construction_side/gallendar` by default;
`CARGO_TARGET_DIR` and `DIST_DIR` can be overridden. After the build, the release binary
is at `~/construction_side/gallendar/target/release/gallendar`.

## Releases

Public repository: [RoTorEx/gallendar](https://github.com/RoTorEx/gallendar).
After committing changes and a separate release
request, run interactively:

```bash
make release
# Enter the MAJOR.MINOR.PATCH version at the command prompt.
make release-push
```

As with the neighbours: `make release` checks for a clean `main`, fetches tags, runs
checks, updates Cargo.toml/Cargo.lock and CHANGELOG, creates a separate commit
and an annotated tag. It does not publish by itself. `make release-push` pushes `main`
and annotated tags without force; the release tag must point to HEAD.

The current release build scope is macOS Apple Silicon (M chips) only,
`aarch64-apple-darwin`. Intel Mac and Linux are deferred; adding platforms requires
separate agreement. Previously published assets remain available.
A push to main and a manual run check the build without publishing. A tag runs
version verification, packaging, smoke install, and a GitHub Release with the archive
`gallendar-macos-aarch64.tar.gz`, SHA-256, and `gallendar-install.sh`.
The public installer uses `RoTorEx/gallendar` by default and accepts `--version`, `--archive`, `--install-dir`,
`--config-dir`, `--no-path-update`; the repository can be overridden
via `GALLENDAR_INSTALL_REPO=OWNER/REPO`. Configs are preserved on update.

## Documents and Kernel

- [BUSINESS.md](BUSINESS.md) — product purpose, behavior, and boundaries.
- [ROADMAP.md](ROADMAP.md) — development state and remaining stages.
- [CHANGELOG.md](CHANGELOG.md) — notable changes.
- [AGENTS.md](AGENTS.md) — routing of working instructions.

```bash
make vibe-pull
# If the Kernel path has changed:
make vibe-kernel-set KERNEL=/absolute/path/to/vibecoding-kernel
make vibe-pull
```
