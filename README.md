# JWS Attendance

Staff attendance management for **Janapremi World School**, Madhyapur Thimi-3, Kaushaltar, Bhaktapur.

A Windows desktop application that collects fingerprint scans from a ZKTeco terminal, turns them into daily attendance according to the school's rules, and produces the reports the office actually prints.

---

## Getting it running on the school computer

You need to do this once, on a machine with an internet connection.

```powershell
powershell -ExecutionPolicy Bypass -File scripts\setup-windows.ps1
```

That installs Rust, Node.js, the Visual Studio C++ build tools and the WebView2 runtime, then builds and tests everything. It takes a while the first time — the C++ build tools alone are about 2 GB.

Then:

| Command | What it does |
|---|---|
| `npm start` | Runs the app with live reload, for working on it |
| `npm run package` | Builds the Windows installer |
| `npm run check` | Runs every test and builds the frontend |

The installer appears in `src-tauri\target\release\bundle\nsis\`. It embeds the WebView2 runtime, so it installs on a school PC that has never been online.

---

## Connecting the K40 Pro

The app defaults to **push mode**: the terminal dials out and sends each scan the moment it happens. Nothing polls, so attendance appears on the dashboard within a second of someone putting their finger on the sensor.

On the terminal keypad:

1. **M/OK** → **Comm.** → **Ethernet** — confirm the fixed IP and that DHCP is off.
2. **Comm.** → **Cloud Server Setting**
3. **Server Mode** → `ADMS`
4. **Server Address** → the LAN address of the computer running this app
5. **Server Port** → `8081`
6. **HTTPS** → **off** — the listener speaks plain HTTP on the school network
7. Save. The terminal reconnects within a minute.

The Devices screen shows the computer's own address and repeats these steps, so nobody has to find this file.

When Windows asks, **allow JWS Attendance through the firewall on private networks**. Without that the terminal cannot reach it.

**Pull mode** is kept as a fallback. *Data Transfer → Fetch punch records* asks the terminal over TCP 4370 for everything it has stored, which is how you recover scans missed during a power cut. It is safe to run at any time — records already held are skipped.

Your terminal, for reference:

| | |
|---|---|
| Model | ZKTeco K40 Pro |
| Serial | `GED7253800740` |
| MAC | `00:17:61:10:c0:77` |
| Firmware | Ver 8.0.4.3-20230515 |
| IP | `192.168.100.99`, port 4370 |

---

## How the app is put together

```
crates/zk-core/        Everything that can be got wrong, and is therefore tested
  proto.rs             ZKTeco binary protocol (TCP 4370)
  push.rs              ADMS push protocol (HTTP)
  pull.rs              TCP client built on proto.rs
  rules.rs             The attendance engine
  calendar.rs          Bikram Sambat conversion
  db.rs                SQLite schema and migrations
  service.rs           Members, departments, recompute, reports
  auth.rs              Password hashing
  migrations/          Numbered SQL, applied in order

src-tauri/             The desktop shell — deliberately thin
  commands.rs          Tauri command handlers, one line each
  push_server.rs       The HTTP listener the terminal talks to
  mailer.rs            Gmail SMTP

src/                   The interface: HTML, CSS and vanilla JavaScript
  js/pages/            One module per screen
  js/demo.js           In-memory backend, so the UI runs in a plain browser
```

The split is the important part. Anything that makes a decision — how a scan becomes a day of attendance, what date a report is stamped with, whether a password is correct — lives in `zk-core`, which has no Tauri dependency and can be tested anywhere with `cargo test`. The desktop layer only moves data between the window and those functions.

### Some decisions worth knowing about

**Raw punches are never edited.** The `punches` table is exactly what the terminal reported. Daily attendance is *derived* from it into the `attendance` table. Change a rule, a shift or a holiday and the app replays the raw scans — history is never rewritten in place.

**Corrections survive recalculation.** When the office fixes a day by hand it is marked `manual`, and a rebuild leaves it alone. A locked month is untouched too. Losing a deliberate correction to an automated rebuild would be the worst thing this app could do.

**Re-importing is safe.** Terminals resend their whole buffer after any network hiccup, so `(device, enrolment, timestamp)` is unique. Duplicates are counted and discarded.

**SQLite, one file, WAL mode.** Backups use `VACUUM INTO`, which takes a consistent snapshot even while punches are arriving — a plain file copy would not.

**Bikram Sambat dates are a lookup, not a formula.** Nepali month lengths are published each year and follow no arithmetic rule. The table covers 2000–2090 BS and was checked against published year-start dates; anything outside it returns nothing rather than a guess. A wrong date on a payroll sheet is worse than a missing one.

**The database uses `rusqlite`, not `tauri-plugin-sql`.** The plugin's model is the frontend issuing SQL, which would scatter the attendance logic across JavaScript and make it untestable. Migrations are still numbered files applied in order, tracked by SQLite's own `user_version`.

---

## Tests

```
npm run check          Everything
cargo test --workspace The core: protocol, rules, calendar, database
npm test               Frontend helpers
npm run test:ui        End-to-end, drives every screen in a real browser
```

The end-to-end suite runs against `demo.js`, so it needs neither Windows nor a terminal on the desk. It catches the failure that matters most: a screen that throws and renders nothing.

A few of the tests exist because they caught real bugs during development, and are worth keeping for that reason:

- **`overnight_shift_does_not_go_backwards`** — the night guard's 05:40 scan sorted *before* his 18:55 arrival, inverting the whole shift.
- **`a_punctual_person_on_the_default_shift_is_a_full_day`** — 09:00–16:00 with a 40-minute break yields 380 worked minutes, so a 390-minute full-day threshold would have marked *every* member of staff as a half day.
- **`a_manual_correction_survives_recompute`** — protects the office's hand corrections from automated rebuilds.
- **`rejects_undecodable_attlog_rather_than_inventing_dates`** — terminals do not announce their record width; guessing wrong would import a year of garbage rather than fail.

---

## Before handing the computer to office staff

1. **Change the password.** A banner nags until you do. The default is `Attendance@123` and it is written in this file, so it is not a secret.
2. **Add the Gmail app password** under *Settings → Email & Alerts*. It is a 16-character app password from the Google account, not the account password. Nothing can be emailed without it.
3. **Set a backup folder** under *Settings → Backup*, ideally somewhere that gets copied off the machine.
4. **Check the holiday calendar.** Dashain, Tihar and the rest are seeded for 2083 BS; confirm the dates against the school's published calendar before the year gets going.

---

## Updates

Updates come from GitHub Releases and are signed. A package not signed with the school's key is refused, and the database is backed up before anything is applied.

To set up signing, once:

```powershell
npm run tauri signer generate -- -w $env:USERPROFILE\.tauri\jws.key
```

Put the **public** key into `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`, and keep the private key off this machine. Until that is done the updater is configured but inert — the placeholder in the config is not a valid key, which is deliberate: it fails closed.
