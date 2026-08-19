//! Tauri command handlers.
//!
//! These are thin: parameter shuffling, locking the connection, and calling
//! into `zk_core`. Anything with a decision in it belongs in the core crate
//! where it can be tested without a window.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::{params, params_from_iter, Connection};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use zk_core::service::{self, MemberInput};
use zk_core::{auth, calendar, db, pull, push};

use crate::mailer;
use crate::push_server::{self, PushListener};

pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub push: Mutex<Option<PushListener>>,
    /// Active password-reset code and the epoch second it expires.
    pub reset: Mutex<Option<(String, i64)>>,
    pub started: std::time::Instant,
}

type R<T> = Result<T, String>;

/// Lock the database, turning a poisoned mutex into a readable message rather
/// than a panic that takes the window down.
fn conn(state: &AppState) -> R<std::sync::MutexGuard<'_, Connection>> {
    state.db.lock().map_err(|_| {
        "The database is in an inconsistent state after an earlier error. \
         Please restart JWS Attendance."
            .to_string()
    })
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn now_epoch() -> i64 {
    chrono::Local::now().timestamp()
}

// ---------------------------------------------------------------------------
// App / dashboard
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct AppInfo {
    pub version: String,
    pub today: String,
    pub today_bs: Option<String>,
    pub db_path: String,
    pub db_size_kb: u64,
    pub schema_version: usize,
    pub push_running: bool,
    pub push_port: u16,
    pub password_is_default: bool,
    pub uptime_secs: u64,
}

#[tauri::command]
pub fn app_info(app: AppHandle, state: State<AppState>) -> R<AppInfo> {
    let c = conn(&state)?;
    let path = crate::db_path(&app);
    let size = std::fs::metadata(&path).map(|m| m.len() / 1024).unwrap_or(0);
    let listener = state.push.lock().map_err(|_| "push state unavailable")?;
    let t = today();

    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        today: t.clone(),
        today_bs: calendar::iso_to_bs(&t).ok().map(|b| b.pretty()),
        db_path: path.display().to_string(),
        db_size_kb: size,
        schema_version: db::version(&c).map_err(|e| e.to_string())?,
        push_running: listener.as_ref().map(|l| l.is_running()).unwrap_or(false),
        push_port: listener.as_ref().map(|l| l.port()).unwrap_or(0),
        password_is_default: db::get_setting(&c, "admin_password_is_default")
            .ok()
            .flatten()
            .as_deref()
            != Some("0"),
        uptime_secs: state.started.elapsed().as_secs(),
    })
}

#[tauri::command]
pub fn dashboard(state: State<AppState>, date: Option<String>) -> R<service::DashboardStats> {
    let c = conn(&state)?;
    service::dashboard(&c, &date.unwrap_or_else(today)).map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct TrendPoint {
    pub date: String,
    pub present: i64,
    pub late: i64,
    pub absent: i64,
}

#[tauri::command]
pub fn attendance_trend(state: State<AppState>, days: i64) -> R<Vec<TrendPoint>> {
    let c = conn(&state)?;
    let days = days.clamp(1, 120);
    let mut stmt = c
        .prepare(
            "SELECT work_date,
                sum(CASE WHEN status='Present' THEN 1 ELSE 0 END),
                sum(CASE WHEN status IN ('Late','HalfDay') THEN 1 ELSE 0 END),
                sum(CASE WHEN status IN ('Absent','MissingPunch') THEN 1 ELSE 0 END)
             FROM attendance
             WHERE status NOT IN ('Holiday','WeeklyOff')
             GROUP BY work_date ORDER BY work_date DESC LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![days], |r| {
            Ok(TrendPoint {
                date: r.get(0)?,
                present: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                late: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                absent: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
            })
        })
        .map_err(|e| e.to_string())?;
    let mut v = rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    v.reverse();
    Ok(v)
}

#[derive(Serialize)]
pub struct DeptStat {
    pub id: i64,
    pub name: String,
    pub colour: String,
    pub total: i64,
    pub present: i64,
    pub rate: f64,
}

#[tauri::command]
pub fn department_stats(state: State<AppState>, date: Option<String>) -> R<Vec<DeptStat>> {
    let c = conn(&state)?;
    let d = date.unwrap_or_else(today);
    let mut stmt = c
        .prepare(
            "SELECT dep.id, dep.name, dep.colour,
                    count(m.id),
                    sum(CASE WHEN a.status IN ('Present','Late','HalfDay') THEN 1 ELSE 0 END)
             FROM departments dep
             LEFT JOIN members m ON m.dept_id = dep.id AND m.status <> 'Inactive'
             LEFT JOIN attendance a ON a.member_id = m.id AND a.work_date = ?1
             WHERE dep.active = 1
             GROUP BY dep.id ORDER BY dep.id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![d], |r| {
            let total: i64 = r.get(3)?;
            let present: i64 = r.get::<_, Option<i64>>(4)?.unwrap_or(0);
            Ok(DeptStat {
                id: r.get(0)?,
                name: r.get(1)?,
                colour: r.get(2)?,
                total,
                present,
                rate: if total > 0 { present as f64 / total as f64 * 100.0 } else { 0.0 },
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct FeedItem {
    pub enroll_no: i64,
    pub full_name: String,
    pub dept_name: Option<String>,
    pub punch_time: String,
    pub punch_state: i64,
}

#[tauri::command]
pub fn punch_feed(state: State<AppState>, limit: i64) -> R<Vec<FeedItem>> {
    let c = conn(&state)?;
    let mut stmt = c
        .prepare(
            "SELECT p.enroll_no, COALESCE(m.full_name,'Unknown (' || p.enroll_no || ')'),
                    d.name, p.punch_time, p.punch_state
             FROM punches p
             LEFT JOIN members m ON m.enroll_no = p.enroll_no
             LEFT JOIN departments d ON d.id = m.dept_id
             ORDER BY p.punch_time DESC LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![limit.clamp(1, 500)], |r| {
            Ok(FeedItem {
                enroll_no: r.get(0)?,
                full_name: r.get(1)?,
                dept_name: r.get(2)?,
                punch_time: r.get(3)?,
                punch_state: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Members / departments
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_members(
    state: State<AppState>,
    search: Option<String>,
    dept_id: Option<i64>,
    status: Option<String>,
) -> R<Vec<service::Member>> {
    let c = conn(&state)?;
    service::list_members(&c, search.as_deref(), dept_id, status.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_member(state: State<AppState>, member: MemberInput) -> R<i64> {
    let c = conn(&state)?;
    let id = service::save_member(&c, &member).map_err(|e| e.to_string())?;

    // Push the change to every terminal so the device stays in step with the
    // office without anyone remembering to run an upload.
    if db::rule_bool(&c, "auto_push_members", true) {
        let cmd = push::DeviceCommand::SetUser {
            pin: member.enroll_no.to_string(),
            name: member
                .device_name
                .clone()
                .unwrap_or_else(|| member.full_name.chars().take(24).collect()),
            privilege: member.privilege as u8,
            password: member.device_password.clone().unwrap_or_default(),
            card: member.card_no.clone().unwrap_or_else(|| "0".into()),
        };
        for serial in device_serials(&c) {
            let _ = push_server::queue_command(&c, &serial, &cmd);
        }
    }
    Ok(id)
}

fn device_serials(c: &Connection) -> Vec<String> {
    let Ok(mut s) =
        c.prepare("SELECT serial FROM devices WHERE active=1 AND serial IS NOT NULL")
    else {
        return Vec::new();
    };
    s.query_map([], |r| r.get::<_, String>(0))
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

#[tauri::command]
pub fn delete_members(state: State<AppState>, ids: Vec<i64>) -> R<usize> {
    let c = conn(&state)?;
    // Collect enrolment numbers before deleting so the terminals can be told.
    let mut pins = Vec::new();
    for id in &ids {
        if let Ok(e) =
            c.query_row("SELECT enroll_no FROM members WHERE id=?1", params![id], |r| {
                r.get::<_, i64>(0)
            })
        {
            pins.push(e);
        }
    }
    let n = service::delete_members(&c, &ids).map_err(|e| e.to_string())?;
    for pin in pins {
        let cmd = push::DeviceCommand::DeleteUser { pin: pin.to_string() };
        for serial in device_serials(&c) {
            let _ = push_server::queue_command(&c, &serial, &cmd);
        }
    }
    Ok(n)
}

#[tauri::command]
pub fn set_members_department(state: State<AppState>, ids: Vec<i64>, dept_id: i64) -> R<usize> {
    let c = conn(&state)?;
    service::set_members_department(&c, &ids, dept_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_departments(state: State<AppState>) -> R<Vec<service::Department>> {
    let c = conn(&state)?;
    service::list_departments(&c).map_err(|e| e.to_string())
}

#[derive(Deserialize)]
pub struct DeptInput {
    pub id: Option<i64>,
    pub name: String,
    pub code: String,
    pub colour: String,
    pub head_member_id: Option<i64>,
    pub default_timetable_id: Option<i64>,
}

#[tauri::command]
pub fn save_department(state: State<AppState>, dept: DeptInput) -> R<i64> {
    let c = conn(&state)?;
    service::save_department(
        &c,
        dept.id,
        &dept.name,
        &dept.code,
        &dept.colour,
        dept.head_member_id,
        dept.default_timetable_id,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_department(state: State<AppState>, id: i64) -> R<()> {
    let c = conn(&state)?;
    service::delete_department(&c, id).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_devices(state: State<AppState>) -> R<Vec<service::Device>> {
    let c = conn(&state)?;
    service::list_devices(&c).map_err(|e| e.to_string())
}

#[derive(Deserialize)]
pub struct DeviceInput {
    pub id: Option<i64>,
    pub name: String,
    pub machine_no: i64,
    pub model: String,
    pub ip: String,
    pub port: i64,
    pub comm_key: i64,
    pub location: Option<String>,
}

#[tauri::command]
pub fn save_device(state: State<AppState>, device: DeviceInput) -> R<i64> {
    let c = conn(&state)?;
    service::save_device(
        &c,
        device.id,
        &device.name,
        device.machine_no,
        &device.model,
        &device.ip,
        device.port,
        device.comm_key,
        device.location.as_deref(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn device_ping(ip: String, port: u16) -> R<u128> {
    pull::ping(&ip, port, Duration::from_secs(3)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn device_info(ip: String, port: u16, comm_key: u32) -> R<pull::DeviceInfo> {
    let mut d = pull::Device::connect(&ip, port, comm_key, Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    let info = d.info().map_err(|e| e.to_string())?;
    let _ = d.disconnect();
    Ok(info)
}

#[derive(Serialize)]
pub struct PullResult {
    pub fetched: usize,
    pub accepted: usize,
    pub duplicates: usize,
    pub recomputed: usize,
}

/// Pull-mode fetch of the terminal's stored attendance log.
#[tauri::command]
pub fn device_download_logs(
    state: State<AppState>,
    ip: String,
    port: u16,
    comm_key: u32,
    serial: String,
    clear_after: bool,
) -> R<PullResult> {
    let mut dev = pull::Device::connect(&ip, port, comm_key, Duration::from_secs(8))
        .map_err(|e| e.to_string())?;
    let logs = dev.attendance().map_err(|e| e.to_string())?;

    let mut c = conn(&state)?;
    let (accepted, duplicates) =
        db::insert_punches(&mut c, &serial, "pull", &logs).map_err(|e| e.to_string())?;

    let mut days: Vec<String> = logs.iter().map(|l| l.date()).collect();
    days.sort();
    days.dedup();
    let mut recomputed = 0;
    if let (Some(first), Some(last)) = (days.first(), days.last()) {
        recomputed = service::recompute(&mut c, first, last).map_err(|e| e.to_string())?;
    }

    // Only clear the terminal after the records are safely committed.
    if clear_after && accepted > 0 {
        dev.clear_attendance().map_err(|e| e.to_string())?;
    }
    let _ = dev.disconnect();

    let _ = db::log_sync(
        &c,
        "Download logs",
        &serial,
        &format!("{} records · {accepted} new", logs.len()),
        true,
    );
    Ok(PullResult { fetched: logs.len(), accepted, duplicates, recomputed })
}

#[tauri::command]
pub fn device_download_users(
    state: State<AppState>,
    ip: String,
    port: u16,
    comm_key: u32,
) -> R<usize> {
    let mut dev = pull::Device::connect(&ip, port, comm_key, Duration::from_secs(8))
        .map_err(|e| e.to_string())?;
    let users = dev.users().map_err(|e| e.to_string())?;
    let _ = dev.disconnect();

    let c = conn(&state)?;
    let mut added = 0usize;
    for u in &users {
        let Ok(enroll) = u.user_id.trim().parse::<i64>() else { continue };
        let exists: bool = c
            .query_row("SELECT 1 FROM members WHERE enroll_no=?1", params![enroll], |_| Ok(true))
            .unwrap_or(false);
        if exists {
            continue;
        }
        let name = if u.name.trim().is_empty() {
            format!("Unnamed {enroll}")
        } else {
            u.name.clone()
        };
        c.execute(
            "INSERT INTO members(enroll_no, full_name, device_name, privilege, card_no, status)
             VALUES(?1,?2,?3,?4,?5,'Active')",
            params![enroll, name, u.name, u.privilege as i64, u.card.to_string()],
        )
        .map_err(|e| e.to_string())?;
        added += 1;
    }
    let _ = db::log_sync(&c, "Download users", &ip, &format!("{added} new users merged"), true);
    Ok(added)
}

/// Queue every selected member for upload to the terminals.
#[tauri::command]
pub fn device_upload_users(state: State<AppState>, member_ids: Vec<i64>) -> R<usize> {
    let c = conn(&state)?;
    let serials = device_serials(&c);
    if serials.is_empty() {
        return Err("No terminal with a serial number is registered yet.".into());
    }

    let mut queued = 0;
    for id in &member_ids {
        let row = c.query_row(
            "SELECT enroll_no, COALESCE(device_name, full_name), privilege,
                    COALESCE(device_password,''), COALESCE(card_no,'0')
             FROM members WHERE id=?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        );
        let Ok((enroll, name, priv_, pass, card)) = row else { continue };
        let cmd = push::DeviceCommand::SetUser {
            pin: enroll.to_string(),
            name,
            privilege: priv_ as u8,
            password: pass,
            card,
        };
        for s in &serials {
            push_server::queue_command(&c, s, &cmd)?;
            queued += 1;
        }
    }
    let _ = db::log_sync(&c, "Upload users", "all", &format!("{queued} commands queued"), true);
    Ok(queued)
}

// ---------------------------------------------------------------------------
// Push listener
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn push_start(app: AppHandle, state: State<AppState>, port: Option<u16>) -> R<u16> {
    let mut guard = state.push.lock().map_err(|_| "push state unavailable")?;
    if let Some(l) = guard.as_ref() {
        if l.is_running() {
            return Ok(l.port());
        }
    }
    let port = port.unwrap_or_else(|| {
        conn(&state)
            .ok()
            .and_then(|c| db::get_setting(&c, "push_port").ok().flatten())
            .and_then(|v| v.parse().ok())
            .unwrap_or(8081)
    });
    let l = push_server::start(app, state.db.clone(), port)?;
    let p = l.port();
    *guard = Some(l);
    Ok(p)
}

#[tauri::command]
pub fn push_stop(state: State<AppState>) -> R<()> {
    let guard = state.push.lock().map_err(|_| "push state unavailable")?;
    if let Some(l) = guard.as_ref() {
        l.stop();
    }
    Ok(())
}

/// The LAN addresses to point the terminal's "Server Address" at.
#[tauri::command]
pub fn local_addresses() -> R<Vec<String>> {
    // Resolving the machine's own hostname is enough on a school LAN and
    // avoids pulling in a network-interface crate.
    let mut out = Vec::new();
    if let Ok(hostname) = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")) {
        use std::net::ToSocketAddrs;
        if let Ok(addrs) = (hostname.as_str(), 0u16).to_socket_addrs() {
            for a in addrs {
                if a.is_ipv4() && !a.ip().is_loopback() {
                    let s = a.ip().to_string();
                    if !out.contains(&s) {
                        out.push(s);
                    }
                }
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Attendance
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn attendance_range(
    state: State<AppState>,
    from: String,
    to: String,
    dept_id: Option<i64>,
    member_id: Option<i64>,
    with_bs: Option<bool>,
) -> R<Vec<service::AttendanceRow>> {
    let c = conn(&state)?;
    service::attendance_range(&c, &from, &to, dept_id, member_id, with_bs.unwrap_or(false))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn recompute(state: State<AppState>, from: String, to: String) -> R<usize> {
    let mut c = conn(&state)?;
    service::recompute(&mut c, &from, &to).map_err(|e| e.to_string())
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn override_attendance(
    state: State<AppState>,
    member_id: i64,
    work_date: String,
    status: String,
    in_time: Option<String>,
    out_time: Option<String>,
    remark: Option<String>,
) -> R<()> {
    let c = conn(&state)?;
    service::override_attendance(
        &c,
        member_id,
        &work_date,
        &status,
        in_time.as_deref(),
        out_time.as_deref(),
        remark.as_deref(),
    )
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Shifts, timetables, holidays
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_shifts(state: State<AppState>) -> R<Vec<serde_json::Value>> {
    let c = conn(&state)?;
    rows_to_json(
        &c,
        "SELECT id,name,code,start_time,end_time,late_grace,early_grace,break_min,
                min_full_day,half_day_after,absent_after,overnight,count_ot,min_ot_block,
                (SELECT count(*) FROM timetable_days td WHERE td.shift_id = shifts.id) AS used_in
         FROM shifts WHERE active=1 ORDER BY id",
        &[],
    )
}

#[tauri::command]
pub fn list_timetables(state: State<AppState>) -> R<Vec<serde_json::Value>> {
    let c = conn(&state)?;
    rows_to_json(
        &c,
        "SELECT t.id, t.name,
                (SELECT count(*) FROM members m WHERE m.timetable_id = t.id) AS assigned,
                (SELECT group_concat(COALESCE(s.code,'-')) FROM timetable_days td
                   LEFT JOIN shifts s ON s.id = td.shift_id
                  WHERE td.timetable_id = t.id ORDER BY td.weekday) AS days
         FROM timetables t WHERE t.active=1 ORDER BY t.id",
        &[],
    )
}

#[tauri::command]
pub fn list_holidays(state: State<AppState>) -> R<Vec<serde_json::Value>> {
    let c = conn(&state)?;
    rows_to_json(
        &c,
        "SELECT id,name,from_date,to_date,applies_to,paid,
                (julianday(to_date) - julianday(from_date) + 1) AS days
         FROM holidays ORDER BY from_date",
        &[],
    )
}

#[derive(Deserialize)]
pub struct HolidayInput {
    pub id: Option<i64>,
    pub name: String,
    pub from_date: String,
    pub to_date: String,
    pub applies_to: Option<String>,
    pub paid: Option<bool>,
}

#[tauri::command]
pub fn save_holiday(state: State<AppState>, holiday: HolidayInput) -> R<i64> {
    if holiday.to_date < holiday.from_date {
        return Err("The end date is before the start date.".into());
    }
    let c = conn(&state)?;
    let applies = holiday.applies_to.unwrap_or_else(|| "all".into());
    let paid = holiday.paid.unwrap_or(true) as i64;
    match holiday.id {
        Some(id) => {
            c.execute(
                "UPDATE holidays SET name=?1,from_date=?2,to_date=?3,applies_to=?4,paid=?5
                 WHERE id=?6",
                params![holiday.name, holiday.from_date, holiday.to_date, applies, paid, id],
            )
            .map_err(|e| e.to_string())?;
            Ok(id)
        }
        None => {
            c.execute(
                "INSERT INTO holidays(name,from_date,to_date,applies_to,paid)
                 VALUES(?1,?2,?3,?4,?5)",
                params![holiday.name, holiday.from_date, holiday.to_date, applies, paid],
            )
            .map_err(|e| e.to_string())?;
            Ok(c.last_insert_rowid())
        }
    }
}

#[tauri::command]
pub fn delete_holiday(state: State<AppState>, id: i64) -> R<()> {
    let c = conn(&state)?;
    c.execute("DELETE FROM holidays WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn set_member_timetable(state: State<AppState>, member_id: i64, timetable_id: Option<i64>) -> R<()> {
    let c = conn(&state)?;
    c.execute(
        "UPDATE members SET timetable_id=?1 WHERE id=?2",
        params![timetable_id, member_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Rules & settings
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_rules(state: State<AppState>) -> R<serde_json::Map<String, serde_json::Value>> {
    let c = conn(&state)?;
    kv_map(&c, "SELECT key, value FROM rules")
}

#[tauri::command]
pub fn set_rules(
    state: State<AppState>,
    rules: std::collections::HashMap<String, String>,
) -> R<usize> {
    let c = conn(&state)?;
    for (k, v) in &rules {
        db::set_rule(&c, k, v).map_err(|e| e.to_string())?;
    }
    db::audit(&c, "admin", "rules.update", &format!("{} rules changed", rules.len()))
        .map_err(|e| e.to_string())?;
    Ok(rules.len())
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> R<serde_json::Map<String, serde_json::Value>> {
    let c = conn(&state)?;
    let mut m = kv_map(&c, "SELECT key, value FROM settings")?;
    // Never hand the SMTP password to the UI; show only whether one is set.
    if let Some(v) = m.get_mut("smtp_pass") {
        let has = v.as_str().map(|s| !s.is_empty()).unwrap_or(false);
        *v = serde_json::Value::String(if has { "********".into() } else { String::new() });
    }
    m.remove("admin_password_hash");
    Ok(m)
}

#[tauri::command]
pub fn set_settings(
    state: State<AppState>,
    settings: std::collections::HashMap<String, String>,
) -> R<usize> {
    let c = conn(&state)?;
    for (k, v) in &settings {
        // Guard against the masked placeholder overwriting the real password.
        if k == "smtp_pass" && v == "********" {
            continue;
        }
        if k == "admin_password_hash" {
            continue;
        }
        db::set_setting(&c, k, v).map_err(|e| e.to_string())?;
    }
    Ok(settings.len())
}

fn kv_map(c: &Connection, sql: &str) -> R<serde_json::Map<String, serde_json::Value>> {
    let mut stmt = c.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;
    let mut m = serde_json::Map::new();
    for row in rows {
        let (k, v) = row.map_err(|e| e.to_string())?;
        m.insert(k, serde_json::Value::String(v));
    }
    Ok(m)
}

// ---------------------------------------------------------------------------
// Database browser
// ---------------------------------------------------------------------------

/// Tables the Database tab is allowed to read.
///
/// A whitelist rather than free-form SQL: the browser is for looking, and an
/// arbitrary-query box in a school office is a way to lose data by accident.
const BROWSABLE: &[&str] = &[
    "members", "departments", "attendance", "punches", "devices", "shifts", "timetables",
    "timetable_days", "holidays", "leaves", "rules", "settings", "audit_log", "sync_log",
    "device_commands",
];

#[tauri::command]
pub fn browse_table(
    state: State<AppState>,
    table: String,
    limit: Option<i64>,
    offset: Option<i64>,
) -> R<serde_json::Value> {
    if !BROWSABLE.contains(&table.as_str()) {
        return Err(format!("'{table}' is not a table you can browse."));
    }
    let c = conn(&state)?;
    let total: i64 = c
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let rows = rows_to_json(
        &c,
        &format!("SELECT * FROM {table} LIMIT ?1 OFFSET ?2"),
        &[&limit.unwrap_or(500).clamp(1, 5000), &offset.unwrap_or(0).max(0)],
    )?;
    Ok(serde_json::json!({ "total": total, "rows": rows }))
}

fn rows_to_json(
    c: &Connection,
    sql: &str,
    args: &[&dyn rusqlite::ToSql],
) -> R<Vec<serde_json::Value>> {
    let mut stmt = c.prepare(sql).map_err(|e| e.to_string())?;
    let cols: Vec<String> = stmt.column_names().into_iter().map(String::from).collect();
    let rows = stmt
        .query_map(params_from_iter(args.iter().copied()), |r| {
            let mut o = serde_json::Map::new();
            for (i, name) in cols.iter().enumerate() {
                let v = match r.get_ref(i)? {
                    rusqlite::types::ValueRef::Null => serde_json::Value::Null,
                    rusqlite::types::ValueRef::Integer(n) => serde_json::json!(n),
                    rusqlite::types::ValueRef::Real(f) => serde_json::json!(f),
                    rusqlite::types::ValueRef::Text(t) => {
                        serde_json::Value::String(String::from_utf8_lossy(t).into_owned())
                    }
                    rusqlite::types::ValueRef::Blob(b) => {
                        serde_json::Value::String(format!("<{} bytes>", b.len()))
                    }
                };
                o.insert(name.clone(), v);
            }
            Ok(serde_json::Value::Object(o))
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn sync_history(state: State<AppState>, limit: i64) -> R<Vec<serde_json::Value>> {
    let c = conn(&state)?;
    rows_to_json(
        &c,
        "SELECT ts, job, device, result, ok FROM sync_log ORDER BY id DESC LIMIT ?1",
        &[&limit.clamp(1, 500)],
    )
}

// ---------------------------------------------------------------------------
// Reports
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ReportRow {
    pub member_id: i64,
    pub enroll_no: i64,
    pub full_name: String,
    pub dept_name: Option<String>,
    pub designation: Option<String>,
    pub working_days: i32,
    pub present: i32,
    pub late: i32,
    pub half_day: i32,
    pub absent: i32,
    pub leave: i32,
    pub worked_min: i32,
    pub ot_min: i32,
    pub late_min: i32,
    pub rate: f64,
}

#[tauri::command]
pub fn report_summary(
    state: State<AppState>,
    from: String,
    to: String,
    dept_id: Option<i64>,
) -> R<Vec<ReportRow>> {
    let c = conn(&state)?;
    let members =
        service::list_members(&c, None, dept_id, None).map_err(|e| e.to_string())?;

    let mut out = Vec::with_capacity(members.len());
    for m in members {
        let s = service::summary_for(&c, &from, &to, m.id).map_err(|e| e.to_string())?;
        out.push(ReportRow {
            member_id: m.id,
            enroll_no: m.enroll_no,
            full_name: m.full_name,
            dept_name: m.dept_name,
            designation: m.designation,
            working_days: s.working_days,
            present: s.present,
            late: s.late,
            half_day: s.half_day,
            absent: s.absent,
            leave: s.leave,
            worked_min: s.worked_min,
            ot_min: s.ot_min,
            late_min: s.late_min,
            rate: s.rate(),
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn export_csv(
    state: State<AppState>,
    from: String,
    to: String,
    dept_id: Option<i64>,
    with_bs: Option<bool>,
) -> R<String> {
    let c = conn(&state)?;
    let rows = service::attendance_range(&c, &from, &to, dept_id, None, with_bs.unwrap_or(false))
        .map_err(|e| e.to_string())?;

    let esc = |s: &str| {
        if s.contains(',') || s.contains('"') || s.contains('\n') {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };

    let mut csv = String::from(
        "Date,Nepali Date,Enroll,Name,Department,Designation,In,Out,Worked,Late,Early,Overtime,Status,Remark\n",
    );
    for r in rows {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            r.work_date,
            esc(r.work_date_bs.as_deref().unwrap_or("")),
            r.enroll_no,
            esc(&r.full_name),
            esc(r.dept_name.as_deref().unwrap_or("")),
            esc(r.designation.as_deref().unwrap_or("")),
            r.in_time.as_deref().unwrap_or(""),
            r.out_time.as_deref().unwrap_or(""),
            zk_core::rules::fmt_duration(r.worked_min as i32),
            r.late_min,
            r.early_min,
            zk_core::rules::fmt_duration(r.ot_min as i32),
            r.status,
            esc(r.remark.as_deref().unwrap_or("")),
        ));
    }
    Ok(csv)
}

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn auth_login(state: State<AppState>, username: String, password: String) -> R<bool> {
    let c = conn(&state)?;
    let user = db::get_setting(&c, "admin_username").ok().flatten().unwrap_or_default();
    if username.trim() != user {
        return Ok(false);
    }
    let stored = db::get_setting(&c, "admin_password_hash").ok().flatten().unwrap_or_default();

    // A blank hash means the install has never had a password set: accept the
    // documented default once, then persist it so later logins go through the
    // normal path.
    if stored.is_empty() {
        if password == auth::DEFAULT_PASSWORD {
            let h = auth::hash_password(&password).map_err(|e| e.to_string())?;
            db::set_setting(&c, "admin_password_hash", &h).map_err(|e| e.to_string())?;
            db::set_setting(&c, "admin_password_is_default", "1").map_err(|e| e.to_string())?;
            let _ = db::audit(&c, &user, "auth.login", "signed in with the default password");
            return Ok(true);
        }
        return Ok(false);
    }

    let ok = auth::verify_password(&password, &stored);
    let _ = db::audit(
        &c,
        &user,
        if ok { "auth.login" } else { "auth.login_failed" },
        "administrator console",
    );
    Ok(ok)
}

#[tauri::command]
pub fn auth_change_password(state: State<AppState>, current: String, new: String) -> R<()> {
    let c = conn(&state)?;
    let stored = db::get_setting(&c, "admin_password_hash").ok().flatten().unwrap_or_default();

    let current_ok = if stored.is_empty() {
        current == auth::DEFAULT_PASSWORD
    } else {
        auth::verify_password(&current, &stored)
    };
    if !current_ok {
        return Err("The current password is not correct.".into());
    }

    auth::check_strength(&new).map_err(|e| e.to_string())?;
    let h = auth::hash_password(&new).map_err(|e| e.to_string())?;
    db::set_setting(&c, "admin_password_hash", &h).map_err(|e| e.to_string())?;
    db::set_setting(&c, "admin_password_is_default", "0").map_err(|e| e.to_string())?;
    db::audit(&c, "admin", "auth.password_changed", "").map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn auth_request_reset(state: State<AppState>) -> R<String> {
    let c = conn(&state)?;
    let to = db::get_setting(&c, "recovery_email")
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .ok_or("No recovery email address is configured.")?;

    let code = auth::generate_reset_code().map_err(|e| e.to_string())?;
    let school = db::get_setting(&c, "school_name").ok().flatten().unwrap_or_default();
    let address = db::get_setting(&c, "school_address").ok().flatten().unwrap_or_default();

    let s = mailer::settings_from_db(&c)?;
    mailer::send(
        &s,
        &to,
        "JWS Attendance — password reset code",
        &mailer::reset_code_mail(&school, &address, &code),
    )?;

    *state.reset.lock().map_err(|_| "reset state unavailable")? =
        Some((code, now_epoch() + 15 * 60));
    db::audit(&c, "admin", "auth.reset_requested", &to).map_err(|e| e.to_string())?;

    // Mask the address in the confirmation shown on screen.
    let masked = match to.split_once('@') {
        Some((u, d)) => format!("{}***@{d}", &u[..u.len().min(3)]),
        None => "the recovery address".into(),
    };
    Ok(masked)
}

#[tauri::command]
pub fn auth_verify_reset(state: State<AppState>, code: String, new_password: String) -> R<()> {
    let mut guard = state.reset.lock().map_err(|_| "reset state unavailable")?;
    let Some((expected, expires)) = guard.clone() else {
        return Err("No reset is in progress. Request a new code first.".into());
    };
    if now_epoch() > expires {
        *guard = None;
        return Err("That code has expired. Request a new one.".into());
    }
    if code.trim() != expected {
        return Err("That code is not correct.".into());
    }

    auth::check_strength(&new_password).map_err(|e| e.to_string())?;
    let c = conn(&state)?;
    let h = auth::hash_password(&new_password).map_err(|e| e.to_string())?;
    db::set_setting(&c, "admin_password_hash", &h).map_err(|e| e.to_string())?;
    db::set_setting(&c, "admin_password_is_default", "0").map_err(|e| e.to_string())?;
    db::audit(&c, "admin", "auth.password_reset", "via emailed code").map_err(|e| e.to_string())?;
    *guard = None;
    Ok(())
}

// ---------------------------------------------------------------------------
// Email
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn send_test_mail(state: State<AppState>, to: Option<String>) -> R<String> {
    let c = conn(&state)?;
    let s = mailer::settings_from_db(&c)?;
    let school = db::get_setting(&c, "school_name").ok().flatten().unwrap_or_default();
    let address = db::get_setting(&c, "school_address").ok().flatten().unwrap_or_default();
    let dest = to.unwrap_or_else(|| s.user.clone());
    mailer::send(
        &s,
        &dest,
        "JWS Attendance — test message",
        &mailer::template(
            &school,
            &address,
            "Email is working",
            "<p style=\"font-size:14px\">If you are reading this, JWS Attendance can send mail \
             from this computer. Absence notices and password reset codes will go out normally.</p>",
        ),
    )?;
    Ok(dest)
}

#[derive(Serialize)]
pub struct MailRun {
    pub sent: usize,
    pub failed: usize,
    pub skipped_no_email: usize,
    pub errors: Vec<String>,
}

#[tauri::command]
pub fn send_absence_emails(state: State<AppState>, date: Option<String>) -> R<MailRun> {
    let c = conn(&state)?;
    let day = date.unwrap_or_else(today);
    let list = service::absentees(&c, &day).map_err(|e| e.to_string())?;

    let s = mailer::settings_from_db(&c)?;
    let school = db::get_setting(&c, "school_name").ok().flatten().unwrap_or_default();
    let address = db::get_setting(&c, "school_address").ok().flatten().unwrap_or_default();
    let bs = calendar::iso_to_bs(&day).map(|b| b.pretty()).unwrap_or_default();

    let mut run = MailRun { sent: 0, failed: 0, skipped_no_email: 0, errors: Vec::new() };
    for m in list {
        let Some(addr) = m.email.as_deref().filter(|e| e.contains('@')) else {
            run.skipped_no_email += 1;
            continue;
        };
        let body = mailer::absence_notice(&school, &address, &m.full_name, &day, &bs);
        match mailer::send(&s, addr, "Attendance not recorded", &body) {
            Ok(()) => run.sent += 1,
            Err(e) => {
                run.failed += 1;
                // Keep the list short; one representative error is enough to act on.
                if run.errors.len() < 3 {
                    run.errors.push(format!("{}: {e}", m.full_name));
                }
            }
        }
    }
    let _ = db::log_sync(
        &c,
        "Absence emails",
        &day,
        &format!("{} sent, {} failed, {} without an address", run.sent, run.failed, run.skipped_no_email),
        run.failed == 0,
    );
    Ok(run)
}

// ---------------------------------------------------------------------------
// Backup
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn backup_now(app: AppHandle, state: State<AppState>) -> R<String> {
    let c = conn(&state)?;
    let dir = db::get_setting(&c, "backup_dir")
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::app_dir(&app).join("backup"));

    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
    let dest = dir.join(format!("attendance-{stamp}.db"));

    // VACUUM INTO takes a consistent snapshot even while the push listener is
    // writing, which a plain file copy would not.
    c.execute("VACUUM INTO ?1", params![dest.to_string_lossy()])
        .map_err(|e| format!("backup failed: {e}"))?;
    let _ = db::audit(&c, "admin", "backup", &dest.display().to_string());
    Ok(dest.display().to_string())
}

#[tauri::command]
pub fn list_backups(app: AppHandle, state: State<AppState>) -> R<Vec<serde_json::Value>> {
    let c = conn(&state)?;
    let dir = db::get_setting(&c, "backup_dir")
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::app_dir(&app).join("backup"));

    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("db") {
                continue;
            }
            let meta = e.metadata().ok();
            let modified = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .map(|t| {
                    let dt: chrono::DateTime<chrono::Local> = t.into();
                    dt.format("%Y-%m-%d %H:%M").to_string()
                })
                .unwrap_or_default();
            out.push(serde_json::json!({
                "file": p.file_name().and_then(|s| s.to_str()).unwrap_or(""),
                "path": p.display().to_string(),
                "size_kb": meta.map(|m| m.len() / 1024).unwrap_or(0),
                "modified": modified,
            }));
        }
    }
    out.sort_by(|a, b| b["file"].as_str().cmp(&a["file"].as_str()));
    Ok(out)
}

#[tauri::command]
pub fn open_path(app: AppHandle, path: String) -> R<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_text_file(path: String, contents: String) -> R<()> {
    std::fs::write(&path, contents).map_err(|e| format!("could not write {path}: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    // The command layer is deliberately thin; the behaviour it wraps is tested
    // in zk-core. What is worth checking here is the table whitelist, because
    // it is the one place this file makes a security decision.
    use super::BROWSABLE;

    #[test]
    fn browsable_tables_do_not_include_anything_unexpected() {
        assert!(BROWSABLE.contains(&"members"));
        assert!(BROWSABLE.contains(&"attendance"));
        assert!(!BROWSABLE.contains(&"sqlite_master"));
        for t in BROWSABLE {
            assert!(
                t.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{t} would need quoting when interpolated"
            );
        }
    }
}
