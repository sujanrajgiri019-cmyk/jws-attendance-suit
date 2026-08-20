//! Bidirectional user and fingerprint synchronisation with a terminal.
//!
//! Two operations, both over the direct protocol on TCP 4370:
//!
//! - **Download** — read the terminal's user table and every fingerprint
//!   template it holds, and merge them into this database.
//! - **Upload** — provision a terminal from this database: write the users,
//!   write their templates, and check they arrived.
//!
//! ## What the ZKTeco SDK calls these
//!
//! Written against the raw protocol rather than `zkemkeeper.dll`, because that
//! DLL is a 32-bit Windows COM object and this application is a Rust binary
//! with no COM host. The sequence is the one every SDK example performs:
//!
//! | `zkemkeeper.dll`           | Here                                          |
//! |----------------------------|-----------------------------------------------|
//! | `Connect_Net(ip, port)`    | `pull::Device::connect`                        |
//! | `SetCommPassword(key)`     | the `comm_key` argument to that call           |
//! | `EnableDevice(idx, false)` | [`DeviceLock::acquire`]                        |
//! | `ReadAllUserID(idx)`       | `CMD_DATA_WRRQ` — staging the table            |
//! | `SSR_GetAllUserInfo(...)`  | [`read_users`] → `proto::parse_user_buffer`    |
//! | `GetUserTmpExStr(...)`     | [`read_templates`] → `fptemp::parse_template_table` |
//! | `SSR_SetUserInfo(...)`     | [`write_user`] → `CMD_SET_USER`                |
//! | `SetUserTmpExStr(...)`     | [`write_template`] → `CMD_USERTEMP_WRQ`        |
//! | `RefreshData(1)`           | `CMD_REFRESH_DATA`, once at the end            |
//! | `EnableDevice(idx, true)`  | `DeviceLock::drop`                             |
//! | `Disconnect()`             | `pull::Device::disconnect`                     |
//!
//! ## Four things worth knowing before changing this
//!
//! **The device lock is released by `Drop`, not by a `finally`.** A terminal
//! left disabled after a failed transfer is the worst outcome this module has:
//! it accepts no fingers at all, gives no reason, and the first to find out is
//! a member of staff at the gate the next morning. [`DeviceLock`] re-enables on
//! every path out, including an early `?` and a panic.
//!
//! **Nothing calls `Device::users()`.** That method disables and re-enables the
//! terminal around its own read, which would unlock the device in the middle of
//! a transfer. This module holds one lock across the whole operation and reads
//! the tables underneath it.
//!
//! **Talking to the device and writing to SQLite are separate calls.** A
//! transfer takes minutes; the database lock is shared with the push listener
//! and every screen in the app. So [`read_device`] does the slow part holding
//! nothing, and [`store_snapshot`] takes the connection only for as long as one
//! transaction needs it. The combined [`download_all_users`] is for tests and
//! anything unattended, where nobody is waiting on the window.
//!
//! **Uploads are verified by reading back.** A terminal will acknowledge a
//! template it has stored in a form it cannot later match against a live
//! finger, so an upload that "succeeded" for 120 people can still mean 120
//! people who cannot get in. Every uploaded template is read back and compared
//! against a keyed digest of what was sent; the number that reaches the office
//! is the verified one, not the acknowledged one.
//!
//! ## The transport caveat
//!
//! On a terminal with its cloud server (ADMS) switched on — which is how the
//! school's K40 Pro is configured — port 4370 commonly accepts a connection and
//! then never answers; `pull::probe` diagnoses exactly that. Everything here
//! therefore needs the terminal's cloud server turned **off** for the duration,
//! or it needs the push channel instead, where templates arrive as
//! `table=FINGERTMP` posts and land in the same table by the same rules.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use rusqlite::{params, Connection, OptionalExtension};

use crate::fptemp::{self, DeviceTemplate};
use crate::keystore::BioKey;
use crate::proto::{self, DeviceUser};
use crate::pull::Device;
use crate::{Error, Result};

// ---------------------------------------------------------------------------
// Progress reporting
// ---------------------------------------------------------------------------

/// Which part of a transfer is running, for the progress line in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Connecting,
    ReadingUsers,
    ReadingTemplates,
    Storing,
    WritingUsers,
    WritingTemplates,
    Verifying,
    Finishing,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Connecting => "Connecting",
            Phase::ReadingUsers => "Reading users",
            Phase::ReadingTemplates => "Reading fingerprints",
            Phase::Storing => "Saving",
            Phase::WritingUsers => "Writing users",
            Phase::WritingTemplates => "Writing fingerprints",
            Phase::Verifying => "Checking",
            Phase::Finishing => "Finishing",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncProgress {
    pub phase: Phase,
    pub current: usize,
    pub total: usize,
    pub message: String,
}

impl SyncProgress {
    /// One line for the transfer pane: `Writing users 25/120 — Sarita Maharjan`.
    pub fn line(&self) -> String {
        if self.total > 0 {
            format!("{} {}/{} — {}", self.phase.label(), self.current, self.total, self.message)
        } else {
            format!("{} — {}", self.phase.label(), self.message)
        }
    }
}

/// Somewhere to send progress. Implemented by the command layer so the window
/// can show a line per step; `zk-core` itself knows nothing about windows.
pub trait Progress {
    fn report(&self, p: &SyncProgress);
}

/// Discards progress. Used by tests and by unattended scheduled transfers.
pub struct Silent;

impl Progress for Silent {
    fn report(&self, _p: &SyncProgress) {}
}

/// Wraps a closure so a caller can pass one without declaring a type.
pub struct Callback<F: Fn(&SyncProgress)>(pub F);

impl<F: Fn(&SyncProgress)> Progress for Callback<F> {
    fn report(&self, p: &SyncProgress) {
        (self.0)(p)
    }
}

fn say(to: &dyn Progress, phase: Phase, current: usize, total: usize, message: impl Into<String>) {
    to.report(&SyncProgress { phase, current, total, message: message.into() });
}

// ---------------------------------------------------------------------------
// Inputs and reports
// ---------------------------------------------------------------------------

/// Which terminal to talk to, and how patiently.
#[derive(Debug, Clone)]
pub struct DeviceTarget {
    pub ip: String,
    pub port: u16,
    /// The device's COMM Key. 0 on an untouched terminal.
    pub comm_key: u32,
    /// Serial number, recorded against everything a transfer stores. Blank is
    /// allowed — it is only ever used for provenance.
    pub serial: String,
    pub timeout: Duration,
    /// If the bulk template table comes back empty, ask for each finger
    /// individually.
    ///
    /// Off by default: it costs one round trip per finger, so a school of 120
    /// staff is 1,200 exchanges with a device that answers at its own pace. Turn
    /// it on for firmware that will not serve the table at all.
    pub allow_slow_fallback: bool,
}

impl DeviceTarget {
    pub fn new(ip: impl Into<String>, port: u16, comm_key: u32) -> Self {
        DeviceTarget {
            ip: ip.into(),
            port,
            comm_key,
            serial: String::new(),
            timeout: Duration::from_secs(10),
            allow_slow_fallback: false,
        }
    }
}

/// Everything one terminal had to say, read under a single lock.
#[derive(Debug, Clone)]
pub struct DeviceSnapshot {
    pub users: Vec<DeviceUser>,
    pub templates: Vec<DeviceTemplate>,
    /// The sensor's finger algorithm version (9, 10 or 12 in the field).
    pub algo_version: i64,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DownloadReport {
    pub users_seen: usize,
    pub users_added: usize,
    pub users_updated: usize,
    pub templates_seen: usize,
    pub templates_stored: usize,
    pub templates_unchanged: usize,
    /// Templates whose device uid matched no user in the table. They cannot be
    /// attributed to anybody, so they are counted and dropped.
    pub orphan_templates: usize,
    pub algo_version: i64,
    pub notes: Vec<String>,
    pub elapsed_ms: u128,
}

impl DownloadReport {
    pub fn summary(&self) -> String {
        format!(
            "{} users ({} new), {} fingerprints stored, {} already current",
            self.users_seen, self.users_added, self.templates_stored, self.templates_unchanged
        )
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct UploadReport {
    pub users_pushed: usize,
    pub templates_pushed: usize,
    /// Read back off the terminal afterwards and matched. This is the number
    /// that means anything.
    pub templates_verified: usize,
    pub failures: Vec<String>,
    pub elapsed_ms: u128,
}

impl UploadReport {
    pub fn summary(&self) -> String {
        format!(
            "{} users and {} fingerprints written, {} confirmed on the terminal{}",
            self.users_pushed,
            self.templates_pushed,
            self.templates_verified,
            if self.failures.is_empty() {
                String::new()
            } else {
                format!(", {} problem(s)", self.failures.len())
            }
        )
    }
}

/// A member of staff on their way to a terminal.
#[derive(Debug, Clone)]
pub struct OutgoingUser {
    pub enroll_no: i64,
    pub name: String,
    pub privilege: u8,
    pub password: String,
    pub card: u32,
    /// The slot this person was last written to, if they have been before.
    pub stored_uid: Option<u16>,
}

/// A template as this database holds it, unsealed and ready to write.
#[derive(Debug, Clone)]
pub struct StoredTemplate {
    pub enroll_no: i64,
    pub finger: u8,
    pub flag: u8,
    pub template: Vec<u8>,
    /// Keyed digest of `template`. Used to verify the read-back.
    pub mac: String,
}

/// Everything to be sent, read out of the database before the socket is opened.
#[derive(Debug, Clone)]
pub struct UploadBatch {
    pub users: Vec<OutgoingUser>,
    pub templates: HashMap<i64, Vec<StoredTemplate>>,
}

impl UploadBatch {
    pub fn template_count(&self) -> usize {
        self.templates.values().map(|v| v.len()).sum()
    }
}

/// What an upload did, and where each person ended up.
#[derive(Debug, Clone)]
pub struct UploadOutcome {
    pub report: UploadReport,
    /// `(enrolment number, device slot)` for anyone whose slot changed.
    pub assigned: Vec<(i64, u16)>,
}

// ---------------------------------------------------------------------------
// The device lock
// ---------------------------------------------------------------------------

/// Holds the terminal disabled for the length of a transfer.
///
/// The `Drop` implementation is the whole point. `EnableDevice(true)` in a
/// `finally` block is the shape every ZKTeco example uses, and it is one early
/// `return` away from being skipped. This cannot be.
struct DeviceLock<'a> {
    dev: &'a mut Device,
}

impl<'a> DeviceLock<'a> {
    fn acquire(dev: &'a mut Device) -> Result<Self> {
        // Reading a table while somebody is mid-scan risks a half-written record.
        dev.disable()?;
        Ok(DeviceLock { dev })
    }
}

impl Drop for DeviceLock<'_> {
    fn drop(&mut self) {
        if let Err(e) = self.dev.enable() {
            // Nothing useful to do but be loud. A terminal left disabled looks
            // broken to everyone who walks up to it.
            tracing::error!(
                "could not re-enable the terminal after a transfer: {e}. \
                 It may refuse scans until it is restarted."
            );
        }
    }
}

impl std::ops::Deref for DeviceLock<'_> {
    type Target = Device;
    fn deref(&self) -> &Device {
        self.dev
    }
}

impl std::ops::DerefMut for DeviceLock<'_> {
    fn deref_mut(&mut self) -> &mut Device {
        self.dev
    }
}

// ---------------------------------------------------------------------------
// Reading from the terminal
// ---------------------------------------------------------------------------

/// Read the user table without touching the device lock.
///
/// `Device::users()` cannot be used here: it disables and re-enables the
/// terminal around its own read, which would release the lock this transfer
/// holds. It also treats an empty table as an error, which is right for a
/// download the office asked for and wrong here — a blank replacement terminal
/// is precisely what an upload is for.
fn read_users(dev: &mut Device) -> Result<Vec<DeviceUser>> {
    let raw = dev.read_with_buffer(proto::CMD_USER_TEMP_RRQ, proto::FCT_USER)?;
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    proto::parse_user_buffer(&raw)
}

/// Read every fingerprint template in one buffered transfer.
fn read_templates(dev: &mut Device) -> Result<Vec<DeviceTemplate>> {
    let raw = dev.read_with_buffer(fptemp::CMD_DB_RRQ, fptemp::FCT_FINGERTMP)?;
    fptemp::parse_template_table(&raw)
}

/// Ask for each finger of each user separately.
///
/// The slow path, for firmware that will not serve the template table. Errors
/// on an individual finger are skipped rather than fatal — most people have two
/// or three fingers enrolled, so the other seven requests legitimately return
/// nothing.
fn read_templates_one_by_one(
    dev: &mut Device,
    users: &[DeviceUser],
    progress: &dyn Progress,
) -> Vec<DeviceTemplate> {
    let mut out = Vec::new();

    for (i, u) in users.iter().enumerate() {
        let Ok(enroll) = u.user_id.trim().parse::<i64>() else { continue };
        say(
            progress,
            Phase::ReadingTemplates,
            i + 1,
            users.len(),
            format!("{} (one finger at a time)", display_name(u)),
        );

        for finger in 0u8..=9 {
            let Ok(req) = fptemp::build_get_template_request(enroll, finger) else { continue };
            let Ok(raw) = dev.read_bulk(fptemp::CMD_GET_USERTEMP, &req) else { continue };

            let body = fptemp::trim_single_template_reply(&raw);
            if body.is_empty() {
                continue;
            }
            out.push(DeviceTemplate {
                uid: u.uid,
                finger,
                flag: fptemp::FLAG_VALID,
                template: body.to_vec(),
            });
        }
    }
    out
}

fn display_name(u: &DeviceUser) -> String {
    if u.name.trim().is_empty() {
        format!("#{}", u.user_id)
    } else {
        u.name.trim().to_string()
    }
}

/// Connect, lock, read everything, unlock, disconnect.
///
/// Holds no database connection: this is the part that takes minutes, and the
/// rest of the application needs SQLite while it runs.
pub fn read_device(target: &DeviceTarget, progress: &dyn Progress) -> Result<DeviceSnapshot> {
    let started = Instant::now();

    say(progress, Phase::Connecting, 0, 0, format!("Connecting to {}:{}…", target.ip, target.port));
    let mut dev = Device::connect(&target.ip, target.port, target.comm_key, target.timeout)?;

    let gathered = gather(&mut dev, target, progress);

    // The socket closes whatever happened above; the lock inside `gather` has
    // already re-enabled the terminal by this point.
    let _ = dev.disconnect();

    let mut snap = gathered?;
    snap.elapsed_ms = started.elapsed().as_millis();
    Ok(snap)
}

fn gather(
    dev: &mut Device,
    target: &DeviceTarget,
    progress: &dyn Progress,
) -> Result<DeviceSnapshot> {
    let mut lock = DeviceLock::acquire(dev)?;

    // Which finger algorithm the sensor speaks. A template captured on version
    // 10 means nothing to a version 12 sensor, and the resulting user simply
    // cannot scan — worth recording so that mismatch can be reported rather
    // than discovered at the gate.
    let algo_version =
        lock.param("~ZKFPVersion").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0);

    say(progress, Phase::ReadingUsers, 0, 0, "Reading the user table…");
    let users = read_users(&mut lock)?;

    say(
        progress,
        Phase::ReadingTemplates,
        0,
        users.len(),
        format!("{} users found. Reading fingerprint templates…", users.len()),
    );
    let mut templates = read_templates(&mut lock)?;

    if templates.is_empty() && !users.is_empty() && target.allow_slow_fallback {
        say(
            progress,
            Phase::ReadingTemplates,
            0,
            users.len(),
            "The terminal served no template table — asking finger by finger…",
        );
        templates = read_templates_one_by_one(&mut lock, &users, progress);
    }

    Ok(DeviceSnapshot { users, templates, algo_version, elapsed_ms: 0 })
    // `lock` drops here: EnableDevice(true).
}

// ---------------------------------------------------------------------------
// Download: writing a snapshot into SQLite
// ---------------------------------------------------------------------------

/// Merge a snapshot into the database, all of it or none of it.
pub fn store_snapshot(
    conn: &mut Connection,
    key: &BioKey,
    target: &DeviceTarget,
    snap: &DeviceSnapshot,
) -> Result<DownloadReport> {
    let mut report = DownloadReport {
        users_seen: snap.users.len(),
        templates_seen: snap.templates.len(),
        algo_version: snap.algo_version,
        elapsed_ms: snap.elapsed_ms,
        ..Default::default()
    };

    // The template table keys on the device's internal uid; everything in this
    // application keys on the enrolment number. This is the bridge, and it is
    // why the user table has to be read even when only fingerprints are wanted.
    let mut enroll_of_uid: HashMap<u16, i64> = HashMap::new();
    for u in &snap.users {
        if let Ok(enroll) = u.user_id.trim().parse::<i64>() {
            enroll_of_uid.insert(u.uid, enroll);
        }
    }

    let tx = conn.transaction()?;
    {
        let mut exists = tx.prepare("SELECT 1 FROM members WHERE enroll_no = ?1")?;
        let mut upsert_user = tx.prepare(
            "INSERT INTO members
                 (enroll_no, full_name, device_name, privilege, card_no,
                  device_password, device_uid, is_enabled, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, 'Active')
             ON CONFLICT(enroll_no) DO UPDATE SET
                 device_name     = excluded.device_name,
                 privilege       = excluded.privilege,
                 card_no         = excluded.card_no,
                 device_password = excluded.device_password,
                 device_uid      = excluded.device_uid,
                 updated_at      = datetime('now','localtime')",
        )?;

        for u in &snap.users {
            let Ok(enroll) = u.user_id.trim().parse::<i64>() else {
                report.notes.push(format!(
                    "Skipped a user whose enrolment number was {:?}, which is not a number.",
                    u.user_id
                ));
                continue;
            };

            let already = exists.query_row(params![enroll], |_| Ok(())).optional()?.is_some();

            // On an update the official name and the enabled flag are left
            // alone. The device holds a 24-character truncation of a name the
            // office typed, and letting it win turns "Sarita Maharjan
            // (Pre-Primary)" into "Sarita Maharjan (Pre-" a little more on every
            // sync. Whether somebody is enabled is the office's decision, not
            // the terminal's.
            upsert_user.execute(params![
                enroll,
                if u.name.trim().is_empty() {
                    format!("Unnamed {enroll}")
                } else {
                    u.name.trim().to_string()
                },
                u.name.trim(),
                u.privilege as i64,
                u.card.to_string(),
                u.password,
                u.uid as i64,
            ])?;

            if already {
                report.users_updated += 1;
            } else {
                report.users_added += 1;
            }
        }

        let mut upsert_fp = tx.prepare(
            "INSERT INTO member_fingerprints
                 (enroll_no, finger_index, template, size, valid, device_serial,
                  device_uid, template_enc, template_mac, enc_version, algo_version, updated_at)
             VALUES (?1, ?2, '', ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, datetime('now','localtime'))
             ON CONFLICT(enroll_no, finger_index) DO UPDATE SET
                 template      = '',
                 size          = excluded.size,
                 valid         = excluded.valid,
                 device_serial = excluded.device_serial,
                 device_uid    = excluded.device_uid,
                 template_enc  = excluded.template_enc,
                 template_mac  = excluded.template_mac,
                 enc_version   = 1,
                 algo_version  = excluded.algo_version,
                 updated_at    = datetime('now','localtime')
             WHERE member_fingerprints.template_mac <> excluded.template_mac",
        )?;

        for t in &snap.templates {
            let Some(&enroll) = enroll_of_uid.get(&t.uid) else {
                // A template belonging to a user the terminal did not list.
                // There is nobody to attribute it to, and guessing would attach
                // one person's finger to another's record.
                report.orphan_templates += 1;
                continue;
            };

            let sealed = key.seal(enroll, t.finger, &t.template)?;
            let mac = key.digest(&t.template);

            // Zero rows changed means the `WHERE` found the same digest already
            // stored: this finger has not been re-enrolled since the last sync,
            // so the row is left exactly as it was.
            let changed = upsert_fp.execute(params![
                enroll,
                t.finger as i64,
                t.template.len() as i64,
                t.flag as i64,
                target.serial,
                t.uid as i64,
                sealed,
                mac,
                snap.algo_version,
            ])?;

            if changed > 0 {
                report.templates_stored += 1;
            } else {
                report.templates_unchanged += 1;
            }
        }

        // Keep the Members screen's "fingers enrolled" column honest. A duress
        // finger (flag 3) is deliberately not counted as an ordinary one.
        tx.execute(
            "UPDATE members SET fp_count = (
                 SELECT COUNT(*) FROM member_fingerprints f
                 WHERE f.enroll_no = members.enroll_no AND f.valid = 1)",
            [],
        )?;
    }
    tx.commit()?;

    if report.orphan_templates > 0 {
        report.notes.push(format!(
            "{} fingerprint template(s) belonged to users the terminal did not list, \
             and were not stored.",
            report.orphan_templates
        ));
    }

    record_run(conn, "download", target, |r| {
        r.users_seen = report.users_seen as i64;
        r.users_written = (report.users_added + report.users_updated) as i64;
        r.fp_seen = report.templates_seen as i64;
        r.fp_written = report.templates_stored as i64;
        r.fp_verified = report.templates_stored as i64;
        r.ok = true;
        r.detail = report.summary();
    });

    Ok(report)
}

/// Read a terminal and store the result. Convenience over [`read_device`] and
/// [`store_snapshot`] for tests and unattended runs, where holding the database
/// lock for the length of the transfer costs nothing.
pub fn download_all_users(
    conn: &mut Connection,
    key: &BioKey,
    target: &DeviceTarget,
    progress: &dyn Progress,
) -> Result<DownloadReport> {
    let snap = read_device(target, progress)?;
    say(
        progress,
        Phase::Storing,
        0,
        snap.users.len(),
        format!("Saving {} users and {} fingerprints…", snap.users.len(), snap.templates.len()),
    );
    let report = store_snapshot(conn, key, target, &snap)?;
    say(progress, Phase::Finishing, snap.users.len(), snap.users.len(), report.summary());
    Ok(report)
}

// ---------------------------------------------------------------------------
// Upload
// ---------------------------------------------------------------------------

/// Gather everything to be sent, before the socket is opened.
///
/// `only_members` restricts the transfer to those `members.id` values; `None`
/// sends everyone who is enabled.
pub fn load_upload_batch(
    conn: &Connection,
    key: &BioKey,
    only_members: Option<&[i64]>,
) -> Result<UploadBatch> {
    let users = load_outgoing_users(conn, only_members)?;
    if users.is_empty() {
        return Err(Error::Invalid(
            "There is nobody to send. Add staff on the Members screen, or check that the \
             people you selected are not marked as disabled."
                .into(),
        ));
    }
    let templates = load_outgoing_templates(conn, key, &users)?;
    Ok(UploadBatch { users, templates })
}

fn load_outgoing_users(conn: &Connection, only: Option<&[i64]>) -> Result<Vec<OutgoingUser>> {
    let wanted: Option<HashSet<i64>> = only.map(|ids| ids.iter().copied().collect());

    let mut stmt = conn.prepare(
        "SELECT id, enroll_no,
                COALESCE(NULLIF(TRIM(device_name), ''), full_name),
                privilege,
                COALESCE(device_password, ''),
                COALESCE(card_no, '0'),
                device_uid
           FROM members
          WHERE is_enabled = 1
          ORDER BY enroll_no",
    )?;

    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<i64>>(6)?,
        ))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (id, enroll, name, privilege, password, card, uid) = row?;
        if let Some(w) = &wanted {
            if !w.contains(&id) {
                continue;
            }
        }
        out.push(OutgoingUser {
            enroll_no: enroll,
            name,
            privilege: privilege as u8,
            // Card numbers are text in this database because terminals report
            // them inconsistently. An unreadable one means "no card", which is
            // right — refusing to send the whole person over a badge number
            // would be worse.
            card: card.trim().parse::<u32>().unwrap_or(0),
            password,
            stored_uid: uid.and_then(|v| u16::try_from(v).ok()),
        });
    }
    Ok(out)
}

fn load_outgoing_templates(
    conn: &Connection,
    key: &BioKey,
    users: &[OutgoingUser],
) -> Result<HashMap<i64, Vec<StoredTemplate>>> {
    let enrolled: HashSet<i64> = users.iter().map(|u| u.enroll_no).collect();

    let mut stmt = conn.prepare(
        "SELECT enroll_no, finger_index, valid, template_enc, template_mac, enc_version, template
           FROM member_fingerprints
          ORDER BY enroll_no, finger_index",
    )?;

    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Option<Vec<u8>>>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, String>(6)?,
        ))
    })?;

    let mut out: HashMap<i64, Vec<StoredTemplate>> = HashMap::new();
    for row in rows {
        let (enroll, finger, valid, sealed, mac, enc_version, legacy) = row?;
        if !enrolled.contains(&enroll) {
            continue;
        }
        let finger = match u8::try_from(finger) {
            Ok(f) if f <= 9 => f,
            _ => continue,
        };

        let plain = if enc_version == 1 {
            let Some(blob) = sealed else { continue };
            key.open(enroll, finger, &blob)?
        } else {
            // Written by a build from before templates were sealed, and not yet
            // converted. Readable, so usable — `seal_legacy_templates` moves it
            // at the next opportunity.
            match fptemp::b64::decode(&legacy) {
                Some(b) if !b.is_empty() => b,
                _ => continue,
            }
        };

        let mac = if mac.is_empty() { key.digest(&plain) } else { mac };
        out.entry(enroll).or_default().push(StoredTemplate {
            enroll_no: enroll,
            finger,
            flag: if valid == 0 { fptemp::FLAG_VALID } else { valid as u8 },
            template: plain,
            mac,
        });
    }
    Ok(out)
}

/// Connect, lock, write users, write templates, refresh, verify, unlock.
///
/// Holds no database connection, for the same reason [`read_device`] does not.
pub fn push_batch(
    target: &DeviceTarget,
    key: &BioKey,
    batch: &UploadBatch,
    progress: &dyn Progress,
) -> Result<UploadOutcome> {
    let started = Instant::now();

    say(progress, Phase::Connecting, 0, 0, format!("Connecting to {}:{}…", target.ip, target.port));
    let mut dev = Device::connect(&target.ip, target.port, target.comm_key, target.timeout)?;

    let outcome = push_everything(&mut dev, key, batch, progress);
    let _ = dev.disconnect();

    let mut outcome = outcome?;
    outcome.report.elapsed_ms = started.elapsed().as_millis();
    Ok(outcome)
}

/// Record an upload: remember the slots and write the history line.
pub fn finish_upload(
    conn: &mut Connection,
    target: &DeviceTarget,
    batch: &UploadBatch,
    outcome: &UploadOutcome,
) -> Result<()> {
    // Remember which slot each person ended up in, so the next upload reuses it
    // rather than creating a second copy of the same member of staff.
    if !outcome.assigned.is_empty() {
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare("UPDATE members SET device_uid = ?2 WHERE enroll_no = ?1")?;
            for (enroll, uid) in &outcome.assigned {
                stmt.execute(params![enroll, *uid as i64])?;
            }
        }
        tx.commit()?;
    }

    record_run(conn, "upload", target, |r| {
        r.users_seen = batch.users.len() as i64;
        r.users_written = outcome.report.users_pushed as i64;
        r.fp_seen = batch.template_count() as i64;
        r.fp_written = outcome.report.templates_pushed as i64;
        r.fp_verified = outcome.report.templates_verified as i64;
        r.ok = outcome.report.failures.is_empty();
        r.detail = outcome.report.summary();
    });
    Ok(())
}

/// Provision a terminal from this database, start to finish.
pub fn upload_all_users(
    conn: &mut Connection,
    key: &BioKey,
    target: &DeviceTarget,
    only_members: Option<&[i64]>,
    progress: &dyn Progress,
) -> Result<UploadReport> {
    let batch = load_upload_batch(conn, key, only_members)?;
    let outcome = push_batch(target, key, &batch, progress)?;
    finish_upload(conn, target, &batch, &outcome)?;
    say(
        progress,
        Phase::Finishing,
        batch.users.len(),
        batch.users.len(),
        outcome.report.summary(),
    );
    Ok(outcome.report)
}

fn push_everything(
    dev: &mut Device,
    key: &BioKey,
    batch: &UploadBatch,
    progress: &dyn Progress,
) -> Result<UploadOutcome> {
    let mut report = UploadReport::default();
    let mut assigned: Vec<(i64, u16)> = Vec::new();

    let mut lock = DeviceLock::acquire(dev)?;

    // Read what is already there before writing anything. Without this, someone
    // who exists on the terminal under a different slot number gets a second
    // slot, and the terminal ends up holding two of them — which surfaces later
    // as punches that do not join to anybody.
    say(
        progress,
        Phase::WritingUsers,
        0,
        batch.users.len(),
        "Reading the terminal's current users…",
    );
    let existing = read_users(&mut lock)?;

    let mut uid_of_enroll: HashMap<i64, u16> = HashMap::new();
    let mut used: HashSet<u16> = HashSet::new();
    for u in &existing {
        used.insert(u.uid);
        if let Ok(e) = u.user_id.trim().parse::<i64>() {
            uid_of_enroll.insert(e, u.uid);
        }
    }
    let mut next_free: u16 = 1;
    let mut uid_for: HashMap<i64, u16> = HashMap::new();

    // --- users ------------------------------------------------------------
    for (i, u) in batch.users.iter().enumerate() {
        say(progress, Phase::WritingUsers, i + 1, batch.users.len(), u.name.clone());

        let uid = match resolve_uid(u, &uid_of_enroll, &mut used, &mut next_free) {
            Ok(uid) => uid,
            Err(e) => {
                report.failures.push(format!("{} ({}): {e}", u.name, u.enroll_no));
                continue;
            }
        };
        uid_for.insert(u.enroll_no, uid);

        let record = DeviceUser {
            uid,
            user_id: u.enroll_no.to_string(),
            name: u.name.clone(),
            privilege: u.privilege,
            password: u.password.clone(),
            card: u.card,
            group_id: "1".to_string(),
        };

        match write_user(&mut lock, &record) {
            Ok(()) => {
                report.users_pushed += 1;
                if u.stored_uid != Some(uid) {
                    assigned.push((u.enroll_no, uid));
                }
            }
            Err(e) => {
                // A terminal that has stopped talking will not start again by
                // being asked 119 more times, and each of those costs a full
                // timeout. A refusal of one record is different: the other 119
                // people should still get across.
                if is_fatal(&e) {
                    return Err(e);
                }
                report.failures.push(format!("{} ({}): {e}", u.name, u.enroll_no));
            }
        }
    }

    // --- templates --------------------------------------------------------
    let total_fp = batch.template_count();
    let mut done = 0usize;
    let mut sent: Vec<(u16, u8, String)> = Vec::new();

    for u in &batch.users {
        let Some(&uid) = uid_for.get(&u.enroll_no) else { continue };
        let Some(fingers) = batch.templates.get(&u.enroll_no) else { continue };

        for t in fingers {
            done += 1;
            say(
                progress,
                Phase::WritingTemplates,
                done,
                total_fp,
                format!("{} — finger {}", u.name, t.finger),
            );

            let record =
                DeviceTemplate { uid, finger: t.finger, flag: t.flag, template: t.template.clone() };

            match write_template(&mut lock, &record) {
                Ok(()) => {
                    report.templates_pushed += 1;
                    sent.push((uid, t.finger, t.mac.clone()));
                }
                Err(e) => {
                    if is_fatal(&e) {
                        return Err(e);
                    }
                    report
                        .failures
                        .push(format!("{} ({}) finger {}: {e}", u.name, u.enroll_no, t.finger));
                }
            }
        }
    }

    // --- commit -----------------------------------------------------------
    // `RefreshData(1)`: make the terminal re-read its own tables so the new
    // users can scan without being restarted. Sent once, at the end — the
    // per-user refresh `Device::set_user` performs is fine for a single edit
    // and wasteful a hundred times over.
    say(progress, Phase::Verifying, 0, sent.len(), "Asking the terminal to reload its data…");
    let (h, _) = lock.command(proto::CMD_REFRESH_DATA, &[])?;
    if h.command != proto::CMD_ACK_OK {
        report.failures.push(format!(
            "the terminal did not accept the refresh command (reply {})",
            h.command
        ));
    }

    // --- verify -----------------------------------------------------------
    // The step that makes the rest trustworthy. Everything above was
    // acknowledged; this is what was actually stored.
    if !sent.is_empty() {
        say(progress, Phase::Verifying, 0, sent.len(), "Reading the fingerprints back…");
        match read_templates(&mut lock) {
            Ok(on_device) => {
                let mut have: HashMap<(u16, u8), String> = HashMap::new();
                for t in &on_device {
                    have.insert((t.uid, t.finger), key.digest(&t.template));
                }
                for (uid, finger, mac) in &sent {
                    if have.get(&(*uid, *finger)) == Some(mac) {
                        report.templates_verified += 1;
                    }
                }
                if report.templates_verified < sent.len() {
                    report.failures.push(format!(
                        "{} of {} fingerprints could not be confirmed on the terminal after \
                         writing. Those staff may not be able to scan.",
                        sent.len() - report.templates_verified,
                        sent.len()
                    ));
                }
            }
            Err(e) => report.failures.push(format!(
                "the fingerprints were written but could not be read back to check them: {e}"
            )),
        }
    }

    Ok(UploadOutcome { report, assigned })
    // `lock` drops here: EnableDevice(true).
}

/// Whether an error means the connection is gone rather than one record being
/// refused.
///
/// The distinction is the difference between a transfer that reports two
/// problem rows and one that spends forty minutes timing out per person.
fn is_fatal(e: &Error) -> bool {
    matches!(e, Error::Timeout | Error::Io(_))
}

/// Pick the slot a user should occupy on the terminal.
///
/// In order of preference: the slot they already occupy there, the slot we last
/// wrote them to (if nobody has taken it), then the lowest free one.
fn resolve_uid(
    u: &OutgoingUser,
    on_device: &HashMap<i64, u16>,
    used: &mut HashSet<u16>,
    next_free: &mut u16,
) -> Result<u16> {
    if let Some(&uid) = on_device.get(&u.enroll_no) {
        used.insert(uid);
        return Ok(uid);
    }
    if let Some(uid) = u.stored_uid {
        if uid != 0 && !used.contains(&uid) {
            used.insert(uid);
            return Ok(uid);
        }
    }
    // uid 0 is not a usable slot on these terminals, so the search starts at 1
    // and a wrap back to 0 means the device is full.
    while *next_free != 0 && used.contains(next_free) {
        *next_free = next_free.wrapping_add(1);
    }
    if *next_free == 0 {
        return Err(Error::Invalid("the terminal has no free user slots left".into()));
    }
    let uid = *next_free;
    used.insert(uid);
    *next_free = next_free.wrapping_add(1);
    Ok(uid)
}

/// `SSR_SetUserInfo`: create or update one user.
///
/// Unlike `Device::set_user`, this does **not** refresh after every record —
/// the refresh happens once when the batch is done.
fn write_user(dev: &mut Device, u: &DeviceUser) -> Result<()> {
    let data = proto::encode_user_72(u);
    let (h, _) = dev.command(proto::CMD_SET_USER, &data)?;
    if h.command != proto::CMD_ACK_OK {
        return Err(Error::Protocol(format!("the terminal refused this user (reply {})", h.command)));
    }
    Ok(())
}

/// `SetUserTmpExStr`: write one fingerprint template.
fn write_template(dev: &mut Device, t: &DeviceTemplate) -> Result<()> {
    let data = fptemp::encode_template_record(t)?;
    let (h, _) = dev.command(fptemp::CMD_USERTEMP_WRQ, &data)?;
    if h.command != proto::CMD_ACK_OK {
        return Err(Error::Protocol(format!(
            "the terminal refused this fingerprint (reply {})",
            h.command
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Converting templates written before they were sealed
// ---------------------------------------------------------------------------

/// Seal any templates still stored as plain base64.
///
/// Run at start-up, once the key is available. Rows whose base64 will not decode
/// are left exactly as they are and reported: a template that cannot be decoded
/// cannot be sealed, and deleting it would throw away the only copy of
/// somebody's finger over a parsing disagreement.
pub fn seal_legacy_templates(conn: &mut Connection, key: &BioKey) -> Result<usize> {
    let pending: Vec<(i64, i64, i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, enroll_no, finger_index, template
               FROM member_fingerprints
              WHERE enc_version = 0 AND template <> ''",
        )?;
        let rows =
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };

    if pending.is_empty() {
        return Ok(0);
    }

    let mut sealed_count = 0usize;
    let mut undecodable = 0usize;

    let tx = conn.transaction()?;
    {
        let mut update = tx.prepare(
            "UPDATE member_fingerprints
                SET template_enc = ?2, template_mac = ?3, enc_version = 1,
                    size = ?4, template = '', updated_at = datetime('now','localtime')
              WHERE id = ?1",
        )?;

        for (id, enroll, finger, b64) in &pending {
            let finger = match u8::try_from(*finger) {
                Ok(f) if f <= 9 => f,
                _ => continue,
            };
            let plain = match fptemp::b64::decode(b64) {
                Some(b) if !b.is_empty() => b,
                _ => {
                    undecodable += 1;
                    continue;
                }
            };

            let blob = key.seal(*enroll, finger, &plain)?;
            update.execute(params![id, blob, key.digest(&plain), plain.len() as i64])?;
            sealed_count += 1;
        }
    }
    tx.commit()?;

    if undecodable > 0 {
        tracing::warn!(
            "{undecodable} fingerprint template(s) could not be decoded and were left in \
             their original form"
        );
    }
    if sealed_count > 0 {
        tracing::info!("sealed {sealed_count} fingerprint template(s)");
    }
    Ok(sealed_count)
}

// ---------------------------------------------------------------------------
// Sync history
// ---------------------------------------------------------------------------

#[derive(Default)]
struct RunRecord {
    users_seen: i64,
    users_written: i64,
    fp_seen: i64,
    fp_written: i64,
    fp_verified: i64,
    ok: bool,
    detail: String,
}

/// Record one transfer in `bio_sync_runs`.
///
/// Never fails the caller: losing a history line must not lose a transfer that
/// actually happened.
fn record_run(
    conn: &Connection,
    direction: &str,
    target: &DeviceTarget,
    fill: impl FnOnce(&mut RunRecord),
) {
    let mut r = RunRecord::default();
    fill(&mut r);

    let _ = conn.execute(
        "INSERT INTO bio_sync_runs
             (finished_at, direction, transport, device_serial, device_ip,
              users_seen, users_written, fp_seen, fp_written, fp_verified, ok, detail)
         VALUES (datetime('now','localtime'), ?1, 'tcp', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            direction,
            target.serial,
            target.ip,
            r.users_seen,
            r.users_written,
            r.fp_seen,
            r.fp_written,
            r.fp_verified,
            r.ok as i64,
            r.detail,
        ],
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// These exercise everything that does not need a terminal on the desk: the
// database side of both directions, the slot allocator, and the conversion of
// templates written by an older build. The socket plumbing waits for hardware,
// exactly as `pull` does.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn key() -> BioKey {
        BioKey::from_master(&[42u8; 32]).unwrap()
    }

    fn target() -> DeviceTarget {
        let mut t = DeviceTarget::new("192.168.100.99", 4370, 0);
        t.serial = "GED7253800740".into();
        t
    }

    fn user(uid: u16, enroll: &str, name: &str) -> DeviceUser {
        DeviceUser {
            uid,
            user_id: enroll.into(),
            name: name.into(),
            privilege: 0,
            password: String::new(),
            card: 0,
            group_id: "1".into(),
        }
    }

    fn template(uid: u16, finger: u8, body: &[u8]) -> DeviceTemplate {
        DeviceTemplate { uid, finger, flag: fptemp::FLAG_VALID, template: body.to_vec() }
    }

    fn snapshot(users: Vec<DeviceUser>, templates: Vec<DeviceTemplate>) -> DeviceSnapshot {
        DeviceSnapshot { users, templates, algo_version: 10, elapsed_ms: 0 }
    }

    #[test]
    fn a_download_stores_users_and_sealed_fingerprints() {
        let mut conn = db::open_memory().unwrap();
        let k = key();
        let snap = snapshot(
            vec![user(1, "41", "Sarita Maharjan"), user(2, "12", "Bikash Shrestha")],
            vec![
                template(1, 0, b"sarita-left-index"),
                template(1, 1, b"sarita-right-index"),
                template(2, 0, b"bikash-left-index"),
            ],
        );

        let report = store_snapshot(&mut conn, &k, &target(), &snap).unwrap();
        assert_eq!(report.users_added, 2);
        assert_eq!(report.templates_stored, 3);

        // No template may be anywhere in the database in the clear.
        let plain: i64 = conn
            .query_row("SELECT COUNT(*) FROM member_fingerprints WHERE template <> ''", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(plain, 0, "a template was stored unsealed");

        let blob: Vec<u8> = conn
            .query_row(
                "SELECT template_enc FROM member_fingerprints WHERE enroll_no=41 AND finger_index=0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            !blob.windows(17).any(|w| w == b"sarita-left-index"),
            "the template survived into the database in the clear"
        );
        assert_eq!(k.open(41, 0, &blob).unwrap(), b"sarita-left-index".to_vec());

        // The Members screen's count must agree.
        let fp_count: i64 = conn
            .query_row("SELECT fp_count FROM members WHERE enroll_no=41", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fp_count, 2);
    }

    #[test]
    fn a_second_download_leaves_unchanged_fingers_alone() {
        // Re-syncing daily must not rewrite every row: the point of the digest
        // is to tell a re-enrolled finger from an untouched one.
        let mut conn = db::open_memory().unwrap();
        let k = key();
        let snap = snapshot(vec![user(1, "41", "Sarita")], vec![template(1, 0, b"sarita-left")]);

        let first = store_snapshot(&mut conn, &k, &target(), &snap).unwrap();
        assert_eq!(first.templates_stored, 1);

        let second = store_snapshot(&mut conn, &k, &target(), &snap).unwrap();
        assert_eq!(second.templates_stored, 0);
        assert_eq!(second.templates_unchanged, 1);
        assert_eq!(second.users_updated, 1);
        assert_eq!(second.users_added, 0);

        // A re-enrolled finger is stored.
        let redone =
            snapshot(vec![user(1, "41", "Sarita")], vec![template(1, 0, b"sarita-left-REDONE")]);
        let third = store_snapshot(&mut conn, &k, &target(), &redone).unwrap();
        assert_eq!(third.templates_stored, 1);
    }

    #[test]
    fn a_download_does_not_overwrite_the_office_s_own_name() {
        // The device holds a 24-character truncation. Letting it win means the
        // real name erodes a little on every sync.
        let mut conn = db::open_memory().unwrap();
        conn.execute(
            "INSERT INTO members(enroll_no, full_name, device_name) \
             VALUES(41, 'Sarita Maharjan (Pre-Primary)', 'Sarita Maharjan (Pre-')",
            [],
        )
        .unwrap();

        store_snapshot(
            &mut conn,
            &key(),
            &target(),
            &snapshot(vec![user(1, "41", "Sarita Maharjan (Pre-")], vec![]),
        )
        .unwrap();

        let full: String = conn
            .query_row("SELECT full_name FROM members WHERE enroll_no=41", [], |r| r.get(0))
            .unwrap();
        assert_eq!(full, "Sarita Maharjan (Pre-Primary)", "the official name was truncated");
    }

    #[test]
    fn a_template_for_an_unknown_user_is_counted_not_guessed() {
        // Attaching it to whoever happens to be nearby would give one member of
        // staff another's fingerprint.
        let mut conn = db::open_memory().unwrap();
        let report = store_snapshot(
            &mut conn,
            &key(),
            &target(),
            &snapshot(vec![user(1, "41", "Sarita")], vec![template(99, 0, b"nobody-knows-whose")]),
        )
        .unwrap();

        assert_eq!(report.orphan_templates, 1);
        assert_eq!(report.templates_stored, 0);
        assert!(!report.notes.is_empty(), "a dropped template must be reported");
        let n: i64 =
            conn.query_row("SELECT COUNT(*) FROM member_fingerprints", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn a_user_with_a_non_numeric_enrolment_number_is_skipped_and_named() {
        let mut conn = db::open_memory().unwrap();
        let report =
            store_snapshot(&mut conn, &key(), &target(), &snapshot(vec![user(1, "ABC", "Nobody")], vec![]))
                .unwrap();
        assert_eq!(report.users_added, 0);
        assert_eq!(report.notes.len(), 1);
    }

    #[test]
    fn a_download_is_recorded_in_the_history() {
        let mut conn = db::open_memory().unwrap();
        store_snapshot(
            &mut conn,
            &key(),
            &target(),
            &snapshot(vec![user(1, "41", "Sarita")], vec![template(1, 0, b"x")]),
        )
        .unwrap();

        let (dir, seen, ok): (String, i64, i64) = conn
            .query_row(
                "SELECT direction, users_seen, ok FROM bio_sync_runs ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((dir.as_str(), seen, ok), ("download", 1, 1));
    }

    #[test]
    fn templates_come_back_out_ready_to_upload() {
        let mut conn = db::open_memory().unwrap();
        let k = key();
        store_snapshot(
            &mut conn,
            &k,
            &target(),
            &snapshot(
                vec![user(1, "41", "Sarita")],
                vec![template(1, 0, b"sarita-left-index"), template(1, 3, b"sarita-thumb")],
            ),
        )
        .unwrap();

        let batch = load_upload_batch(&conn, &k, None).unwrap();
        assert_eq!(batch.users.len(), 1);
        assert_eq!(batch.users[0].enroll_no, 41);
        assert_eq!(batch.users[0].stored_uid, Some(1));
        assert_eq!(batch.template_count(), 2);

        let mine = &batch.templates[&41];
        let first = mine.iter().find(|t| t.finger == 0).unwrap();
        assert_eq!(first.template, b"sarita-left-index".to_vec());
        assert_eq!(first.mac, k.digest(b"sarita-left-index"));
    }

    #[test]
    fn a_disabled_member_is_not_sent_to_the_terminal() {
        let conn = db::open_memory().unwrap();
        conn.execute(
            "INSERT INTO members(enroll_no, full_name, is_enabled) VALUES(41,'Present',1),
                                                                        (42,'Departed',0)",
            [],
        )
        .unwrap();
        let batch = load_upload_batch(&conn, &key(), None).unwrap();
        assert_eq!(batch.users.len(), 1);
        assert_eq!(batch.users[0].enroll_no, 41);
    }

    #[test]
    fn an_upload_can_be_restricted_to_chosen_members() {
        let conn = db::open_memory().unwrap();
        conn.execute(
            "INSERT INTO members(id, enroll_no, full_name) VALUES(1,41,'A'),(2,42,'B'),(3,43,'C')",
            [],
        )
        .unwrap();
        let batch = load_upload_batch(&conn, &key(), Some(&[1, 3])).unwrap();
        assert_eq!(batch.users.iter().map(|u| u.enroll_no).collect::<Vec<_>>(), vec![41, 43]);
    }

    #[test]
    fn an_upload_with_nobody_to_send_says_so() {
        let conn = db::open_memory().unwrap();
        assert!(load_upload_batch(&conn, &key(), None).is_err());
    }

    #[test]
    fn the_device_name_is_preferred_when_the_office_set_one() {
        let conn = db::open_memory().unwrap();
        conn.execute(
            "INSERT INTO members(enroll_no, full_name, device_name)
             VALUES(41, 'Sarita Maharjan (Pre-Primary)', 'Sarita M')",
            [],
        )
        .unwrap();
        assert_eq!(load_upload_batch(&conn, &key(), None).unwrap().users[0].name, "Sarita M");

        // ...and the full name is used when it is blank, rather than sending an
        // empty label to the terminal's screen.
        conn.execute("UPDATE members SET device_name = '   ' WHERE enroll_no=41", []).unwrap();
        assert_eq!(
            load_upload_batch(&conn, &key(), None).unwrap().users[0].name,
            "Sarita Maharjan (Pre-Primary)"
        );
    }

    #[test]
    fn uid_allocation_reuses_the_slot_the_terminal_already_has() {
        // Writing an existing person to a new slot leaves two of them on the
        // device, and their punches stop joining to one record.
        let mut used: HashSet<u16> = HashSet::new();
        let mut next: u16 = 1;
        let on_device: HashMap<i64, u16> = [(41i64, 7u16)].into_iter().collect();
        used.insert(7);

        let u = OutgoingUser {
            enroll_no: 41,
            name: "Sarita".into(),
            privilege: 0,
            password: String::new(),
            card: 0,
            stored_uid: Some(3),
        };
        assert_eq!(resolve_uid(&u, &on_device, &mut used, &mut next).unwrap(), 7);
    }

    #[test]
    fn uid_allocation_never_hands_out_the_same_slot_twice() {
        let mut used: HashSet<u16> = HashSet::new();
        let mut next: u16 = 1;
        let on_device: HashMap<i64, u16> = HashMap::new();

        let mut seen = HashSet::new();
        for enroll in 0..50i64 {
            let u = OutgoingUser {
                enroll_no: enroll,
                name: format!("Staff {enroll}"),
                privilege: 0,
                password: String::new(),
                card: 0,
                // Everybody claims slot 5; only the first can have it.
                stored_uid: Some(5),
            };
            let uid = resolve_uid(&u, &on_device, &mut used, &mut next).unwrap();
            assert!(uid != 0, "uid 0 is not a usable slot");
            assert!(seen.insert(uid), "slot {uid} was handed out twice");
        }
    }

    #[test]
    fn templates_written_before_sealing_are_converted_in_place() {
        let mut conn = db::open_memory().unwrap();
        let k = key();

        // The shape a build from before migration 005 left behind.
        conn.execute("INSERT INTO members(enroll_no, full_name) VALUES(41,'Sarita')", []).unwrap();
        conn.execute(
            "INSERT INTO member_fingerprints(enroll_no, finger_index, template, size, valid)
             VALUES(41, 0, ?1, 17, 1)",
            params![fptemp::b64::encode(b"sarita-left-index")],
        )
        .unwrap();
        // ...and one that will not decode, which must survive untouched.
        conn.execute(
            "INSERT INTO member_fingerprints(enroll_no, finger_index, template, size, valid)
             VALUES(41, 1, 'not!valid!base64', 5, 1)",
            [],
        )
        .unwrap();

        assert_eq!(seal_legacy_templates(&mut conn, &k).unwrap(), 1);

        let (blob, enc, plain): (Vec<u8>, i64, String) = conn
            .query_row(
                "SELECT template_enc, enc_version, template FROM member_fingerprints
                  WHERE enroll_no=41 AND finger_index=0",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(enc, 1);
        assert_eq!(plain, "", "the plaintext was left behind after sealing");
        assert_eq!(k.open(41, 0, &blob).unwrap(), b"sarita-left-index".to_vec());

        // The undecodable row is still there, still in its original form.
        let (enc1, plain1): (i64, String) = conn
            .query_row(
                "SELECT enc_version, template FROM member_fingerprints
                  WHERE enroll_no=41 AND finger_index=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(enc1, 0);
        assert_eq!(plain1, "not!valid!base64");

        // Running again is a no-op, not a second pass over the same rows.
        assert_eq!(seal_legacy_templates(&mut conn, &k).unwrap(), 0);
    }

    #[test]
    fn a_legacy_template_is_still_uploadable_before_it_is_sealed() {
        let conn = db::open_memory().unwrap();
        conn.execute("INSERT INTO members(enroll_no, full_name) VALUES(41,'Sarita')", []).unwrap();
        conn.execute(
            "INSERT INTO member_fingerprints(enroll_no, finger_index, template, size, valid)
             VALUES(41, 0, ?1, 17, 1)",
            params![fptemp::b64::encode(b"sarita-left-index")],
        )
        .unwrap();

        let batch = load_upload_batch(&conn, &key(), None).unwrap();
        assert_eq!(batch.templates[&41][0].template, b"sarita-left-index".to_vec());
    }

    #[test]
    fn a_progress_line_reads_the_way_the_office_expects() {
        let p = SyncProgress {
            phase: Phase::WritingUsers,
            current: 25,
            total: 120,
            message: "Sarita Maharjan".into(),
        };
        assert_eq!(p.line(), "Writing users 25/120 — Sarita Maharjan");

        let q = SyncProgress {
            phase: Phase::Connecting,
            current: 0,
            total: 0,
            message: "Connecting to 192.168.100.99:4370…".into(),
        };
        assert_eq!(q.line(), "Connecting — Connecting to 192.168.100.99:4370…");
    }

    #[test]
    fn a_transfer_against_a_dead_address_fails_without_hanging() {
        // 203.0.113.0/24 is TEST-NET-3: reserved, unroutable.
        let mut t = DeviceTarget::new("203.0.113.1", 4370, 0);
        t.timeout = Duration::from_millis(300);
        let started = Instant::now();
        assert!(read_device(&t, &Silent).is_err());
        assert!(started.elapsed() < Duration::from_secs(3), "the timeout was not honoured");
    }
}
