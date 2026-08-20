//! Commands for the "Download user info and FP" / "Upload user info and FP"
//! buttons on the Data Transfer screen.
//!
//! Thin, like the rest of `commands`: parameter shuffling, locking, and calls
//! into `zk_core::biosync`. The two things that are not quite trivial and are
//! therefore done here rather than in the core crate:
//!
//! **The database lock is not held while the terminal is talking.** A transfer
//! of 120 staff takes minutes, and the same mutex is wanted by the push
//! listener storing punches and by every screen in the window. So the device
//! half runs holding nothing, and the connection is taken only for the
//! transaction at each end. Doing it the obvious way — one call that takes both
//! — freezes the dashboard for the length of the sync.
//!
//! **`#[tauri::command(async)]` is what keeps the window responsive.** Tauri
//! runs a synchronous command marked this way on a worker thread rather than on
//! the UI thread, which is the same arrangement `device_download_logs` already
//! relies on. There is no need to spawn anything by hand, and doing so would
//! mean giving up `State<'_, AppState>`.

use std::path::PathBuf;

use rusqlite::params;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use zk_core::biosync::{
    self, Callback, DeviceTarget, DownloadReport, SyncProgress, UploadReport,
};
use zk_core::db;
use zk_core::keystore::BioKey;

use crate::commands::{conn, AppState, R};

/// Where the key that unlocks the fingerprint templates lives.
///
/// Beside the database, deliberately *not* inside it: the backup routine copies
/// `attendance.db` onto a memory stick, and a key travelling with it would
/// protect nothing.
pub fn bio_key_path(app: &AppHandle) -> PathBuf {
    crate::app_dir(app).join("biometrics.key")
}

fn load_key(app: &AppHandle) -> R<BioKey> {
    BioKey::load_or_create(&bio_key_path(app)).map_err(|e| {
        format!(
            "The fingerprint key at {} could not be opened: {e}",
            bio_key_path(app).display()
        )
    })
}

/// Feed progress to both the existing transfer pane and anything newer.
///
/// `transfer-progress` is the event the Data Transfer screen already listens
/// to, so these transfers appear in the same place as every other one without
/// the frontend needing to change.
///
/// Written out at each call site rather than returned from a helper: a helper
/// would have to name the closure's type, and `Callback<impl Fn(&SyncProgress)>`
/// is a shape worth avoiding for two uses.
macro_rules! progress_to {
    ($app:expr) => {{
        let handle = $app.clone();
        Callback(move |p: &SyncProgress| {
            let _ = handle.emit("transfer-progress", p.line());
            let _ = handle.emit("bio-sync-progress", p.clone());
        })
    }};
}

fn target_for(state: &State<'_, AppState>, ip: String, port: u16, comm_key: u32) -> R<DeviceTarget> {
    let serial: String = {
        let c = conn(state)?;
        c.query_row("SELECT COALESCE(serial,'') FROM devices WHERE ip=?1", params![ip], |r| r.get(0))
            .unwrap_or_default()
    };

    let mut t = DeviceTarget::new(ip, port, comm_key);
    t.serial = serial;
    t.timeout = std::time::Duration::from_secs(10);
    Ok(t)
}

// ---------------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------------

/// Read a terminal's users and every fingerprint it holds into this database.
#[tauri::command(async)]
pub fn device_download_biometrics(
    app: AppHandle,
    state: State<'_, AppState>,
    ip: String,
    port: u16,
    comm_key: u32,
    slow_fallback: Option<bool>,
) -> R<DownloadReport> {
    let mut target = target_for(&state, ip, port, comm_key)?;
    target.allow_slow_fallback = slow_fallback.unwrap_or(false);
    let key = load_key(&app)?;

    // --- the terminal, holding no database lock --------------------------
    let snapshot = biosync::read_device(&target, &progress_to!(app)).map_err(|e| {
        let _ = app.emit("transfer-progress", format!("The transfer stopped: {e}"));
        e.to_string()
    })?;

    // --- the database, for one transaction -------------------------------
    let report = {
        let mut c = conn(&state)?;
        let report = biosync::store_snapshot(&mut c, &key, &target, &snapshot)
            .map_err(|e| e.to_string())?;
        let _ = db::log_sync(&c, "Download users and FP", &target.serial, &report.summary(), true);
        report
    };

    let _ = app.emit("transfer-progress", report.summary());
    Ok(report)
}

// ---------------------------------------------------------------------------
// Upload
// ---------------------------------------------------------------------------

/// Provision a terminal from this database.
///
/// An empty `member_ids` means everybody who is enabled — which is what
/// "Upload user info and FP" does when nothing is selected.
#[tauri::command(async)]
pub fn device_upload_biometrics(
    app: AppHandle,
    state: State<'_, AppState>,
    ip: String,
    port: u16,
    comm_key: u32,
    member_ids: Vec<i64>,
) -> R<UploadReport> {
    let target = target_for(&state, ip, port, comm_key)?;
    let key = load_key(&app)?;
    let only = if member_ids.is_empty() { None } else { Some(member_ids.as_slice()) };

    // --- gather, briefly holding the lock --------------------------------
    let batch = {
        let c = conn(&state)?;
        biosync::load_upload_batch(&c, &key, only).map_err(|e| e.to_string())?
    };

    let _ = app.emit(
        "transfer-progress",
        format!(
            "Sending {} staff and {} fingerprints to {}…",
            batch.users.len(),
            batch.template_count(),
            target.ip
        ),
    );

    // --- the terminal, holding no database lock --------------------------
    let outcome = biosync::push_batch(&target, &key, &batch, &progress_to!(app)).map_err(|e| {
        let _ = app.emit("transfer-progress", format!("The transfer stopped: {e}"));
        e.to_string()
    })?;

    // --- record it -------------------------------------------------------
    {
        let mut c = conn(&state)?;
        biosync::finish_upload(&mut c, &target, &batch, &outcome).map_err(|e| e.to_string())?;
        let _ = db::log_sync(
            &c,
            "Upload users and FP",
            &target.serial,
            &outcome.report.summary(),
            outcome.report.failures.is_empty(),
        );
    }

    // Every failure gets its own line. A transfer that half-worked is the case
    // that costs a day, and "118 of 120" with no names attached is not enough
    // to act on.
    for line in &outcome.report.failures {
        let _ = app.emit("transfer-progress", line.clone());
    }
    let _ = app.emit("transfer-progress", outcome.report.summary());

    Ok(outcome.report)
}

// ---------------------------------------------------------------------------
// History and key status
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct BioSyncRun {
    pub id: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub direction: String,
    pub transport: String,
    pub device_serial: String,
    pub device_ip: String,
    pub users_seen: i64,
    pub users_written: i64,
    pub fp_seen: i64,
    pub fp_written: i64,
    pub fp_verified: i64,
    pub ok: bool,
    pub detail: String,
}

#[tauri::command(async)]
pub fn bio_sync_history(state: State<'_, AppState>, limit: Option<i64>) -> R<Vec<BioSyncRun>> {
    let c = conn(&state)?;
    let mut stmt = c
        .prepare(
            "SELECT id, started_at, finished_at, direction, transport, device_serial, device_ip,
                    users_seen, users_written, fp_seen, fp_written, fp_verified, ok, detail
               FROM bio_sync_runs
              ORDER BY id DESC
              LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![limit.unwrap_or(50).clamp(1, 500)], |r| {
            Ok(BioSyncRun {
                id: r.get(0)?,
                started_at: r.get(1)?,
                finished_at: r.get(2)?,
                direction: r.get(3)?,
                transport: r.get(4)?,
                device_serial: r.get(5)?,
                device_ip: r.get(6)?,
                users_seen: r.get(7)?,
                users_written: r.get(8)?,
                fp_seen: r.get(9)?,
                fp_written: r.get(10)?,
                fp_verified: r.get(11)?,
                ok: r.get::<_, i64>(12)? != 0,
                detail: r.get(13)?,
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<std::result::Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct BioKeyStatus {
    pub key_path: String,
    pub key_exists: bool,
    pub sealed: i64,
    /// Rows still stored as plain base64 by a build from before migration 005.
    pub unsealed: i64,
}

/// What the Settings screen needs to tell the office whether the key exists and
/// whether anything is still stored in the clear.
#[tauri::command(async)]
pub fn bio_key_status(app: AppHandle, state: State<'_, AppState>) -> R<BioKeyStatus> {
    let path = bio_key_path(&app);
    let c = conn(&state)?;

    let sealed: i64 = c
        .query_row("SELECT COUNT(*) FROM member_fingerprints WHERE enc_version = 1", [], |r| {
            r.get(0)
        })
        .unwrap_or(0);
    let unsealed: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM member_fingerprints WHERE enc_version = 0 AND template <> ''",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    Ok(BioKeyStatus {
        key_exists: path.exists(),
        key_path: path.display().to_string(),
        sealed,
        unsealed,
    })
}

/// Seal anything a previous build left in the clear.
///
/// Called once at start-up. Failure is logged rather than fatal: templates that
/// stay unsealed are still usable, and refusing to open the app over it would
/// be a much worse outcome than the one it is guarding against.
pub fn seal_legacy_on_start(app: &AppHandle, db: &std::sync::Mutex<rusqlite::Connection>) {
    let key = match BioKey::load_or_create(&bio_key_path(app)) {
        Ok(k) => k,
        Err(e) => {
            tracing::warn!("no biometric key, fingerprint templates stay unsealed: {e}");
            return;
        }
    };
    let Ok(mut c) = db.lock() else { return };
    match biosync::seal_legacy_templates(&mut c, &key) {
        Ok(0) => {}
        Ok(n) => tracing::info!("sealed {n} fingerprint template(s) written by an earlier build"),
        Err(e) => tracing::warn!("could not seal existing fingerprint templates: {e}"),
    }
}
