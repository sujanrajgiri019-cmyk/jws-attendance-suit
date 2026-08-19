//! Commands behind the Reports screen and the report email book.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use zk_core::db;
use zk_core::reports::{self, Filters, ReportResult};

use crate::commands::{conn, AppState, R};
use crate::mailer;

// ---------------------------------------------------------------------------
// Running reports
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ReportKind {
    pub key: String,
    pub label: String,
}

#[tauri::command(async)]
pub fn report_kinds() -> R<Vec<ReportKind>> {
    Ok(reports::REPORTS
        .iter()
        .map(|(k, l)| ReportKind { key: (*k).into(), label: (*l).into() })
        .collect())
}

#[tauri::command(async)]
pub fn run_report(state: State<'_, AppState>, key: String, filters: Filters) -> R<ReportResult> {
    let c = conn(&state)?;
    reports::run(&c, &key, &filters).map_err(|e| e.to_string())
}

/// Write a report to disk in the format the office asked for.
///
/// PDF is produced as HTML that the browser prints; a bundled PDF engine would
/// add tens of megabytes to the installer for something Windows already does
/// well from the print dialog.
#[tauri::command(async)]
pub fn export_report(
    state: State<'_, AppState>,
    key: String,
    filters: Filters,
    format: String,
    path: String,
) -> R<String> {
    let c = conn(&state)?;
    let report = reports::run(&c, &key, &filters).map_err(|e| e.to_string())?;
    let brand: String = db::get_setting(&c, "school_name")
        .ok()
        .flatten()
        .unwrap_or_else(|| "Janapremi World School".into());

    let body = match format.as_str() {
        "csv" => report.to_csv(),
        "html" | "pdf" => report.to_html(&brand),
        other => return Err(format!("'{other}' is not a format this app can write.")),
    };

    // A UTF-8 byte-order mark is what makes Excel read Nepali names correctly
    // from a CSV; without it they arrive as mojibake.
    let bytes: Vec<u8> = if format == "csv" {
        let mut v = vec![0xEF, 0xBB, 0xBF];
        v.extend_from_slice(body.as_bytes());
        v
    } else {
        body.into_bytes()
    };

    if let Some(dir) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {dir:?}: {e}"))?;
    }
    std::fs::write(&path, bytes).map_err(|e| format!("Could not write {path}: {e}"))?;
    let _ = db::audit(&c, "admin", "report.export", &format!("{key} to {path}"));
    Ok(path)
}

// ---------------------------------------------------------------------------
// The recipient book
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipient {
    pub id: Option<i64>,
    pub name: String,
    pub email: String,
    pub role: String,
    pub dept_id: Option<i64>,
    #[serde(default)]
    pub dept_name: Option<String>,
    /// Report keys this official is on the list for.
    pub reports: Vec<String>,
    pub active: bool,
}

#[tauri::command(async)]
pub fn list_recipients(state: State<'_, AppState>) -> R<Vec<Recipient>> {
    let c = conn(&state)?;
    let mut s = c
        .prepare(
            "SELECT r.id, r.name, r.email, r.role, r.dept_id, d.name AS dept_name,
                    r.reports, r.active
             FROM report_recipients r
             LEFT JOIN departments d ON d.id = r.dept_id
             ORDER BY r.active DESC, r.name",
        )
        .map_err(|e| e.to_string())?;
    let rows = s
        .query_map([], |x| {
            let raw: String = x.get("reports")?;
            Ok(Recipient {
                id: x.get("id")?,
                name: x.get("name")?,
                email: x.get("email")?,
                role: x.get("role")?,
                dept_id: x.get("dept_id")?,
                dept_name: x.get("dept_name")?,
                // A hand-edited database should not break the screen.
                reports: serde_json::from_str(&raw).unwrap_or_default(),
                active: x.get::<_, i64>("active")? != 0,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

/// Enough of a check to catch a typed mistake, without pretending to validate
/// deliverability — only sending can do that.
fn check_email(e: &str) -> Result<(), String> {
    let e = e.trim();
    let Some((user, host)) = e.split_once('@') else {
        return Err(format!("'{e}' is missing an @."));
    };
    if user.is_empty() || !host.contains('.') || host.starts_with('.') || host.ends_with('.') {
        return Err(format!("'{e}' does not look like an email address."));
    }
    if e.contains(' ') {
        return Err("An email address cannot contain a space.".into());
    }
    Ok(())
}

#[tauri::command(async)]
pub fn save_recipient(state: State<'_, AppState>, r: Recipient) -> R<i64> {
    if r.name.trim().is_empty() {
        return Err("Give the recipient a name.".into());
    }
    check_email(&r.email)?;
    for k in &r.reports {
        if !reports::REPORTS.iter().any(|(key, _)| key == k) {
            return Err(format!("'{k}' is not a report this app produces."));
        }
    }

    let c = conn(&state)?;
    let json = serde_json::to_string(&r.reports).unwrap_or_else(|_| "[]".into());
    let email = r.email.trim().to_lowercase();

    let id = match r.id {
        Some(id) => {
            c.execute(
                "UPDATE report_recipients SET name=?1, email=?2, role=?3, dept_id=?4,
                    reports=?5, active=?6 WHERE id=?7",
                params![r.name.trim(), email, r.role, r.dept_id, json, r.active as i64, id],
            )
            .map_err(|e| dup_email(e, &email))?;
            id
        }
        None => {
            c.execute(
                "INSERT INTO report_recipients (name, email, role, dept_id, reports, active)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![r.name.trim(), email, r.role, r.dept_id, json, r.active as i64],
            )
            .map_err(|e| dup_email(e, &email))?;
            c.last_insert_rowid()
        }
    };
    let _ = db::audit(&c, "admin", "recipient.save", &format!("{} <{}>", r.name, email));
    Ok(id)
}

fn dup_email(e: rusqlite::Error, email: &str) -> String {
    if e.to_string().contains("UNIQUE") {
        format!("{email} is already in the list.")
    } else {
        e.to_string()
    }
}

#[tauri::command(async)]
pub fn delete_recipient(state: State<'_, AppState>, id: i64) -> R<()> {
    let c = conn(&state)?;
    let who: Option<String> =
        c.query_row("SELECT email FROM report_recipients WHERE id=?1", params![id], |r| r.get(0))
            .ok();
    c.execute("DELETE FROM report_recipients WHERE id=?1", params![id])
        .map_err(|e| e.to_string())?;
    let _ = db::audit(&c, "admin", "recipient.delete", &who.unwrap_or_default());
    Ok(())
}

// ---------------------------------------------------------------------------
// Sending
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct SendOutcome {
    pub sent: usize,
    pub failed: usize,
    pub details: Vec<String>,
}

/// Email a report to chosen officials.
///
/// The report is built once and sent to everyone, except where a recipient is
/// tied to a department — those get the same report narrowed to their own
/// staff, so a head of department is not handed the whole school's figures.
#[tauri::command(async)]
pub fn send_report_email(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    filters: Filters,
    recipient_ids: Vec<i64>,
    note: Option<String>,
) -> R<SendOutcome> {
    if recipient_ids.is_empty() {
        return Err("Choose at least one person to send this to.".into());
    }

    let c = conn(&state)?;
    // Fails here, once, with an instruction — rather than once per recipient.
    let settings = mailer::settings_from_db(&c)?;

    let brand: String = db::get_setting(&c, "school_name")
        .ok()
        .flatten()
        .unwrap_or_else(|| "Janapremi World School".into());

    let mut out = SendOutcome { sent: 0, failed: 0, details: Vec::new() };

    for id in recipient_ids {
        let who: Option<(String, String, Option<i64>)> = c
            .query_row(
                "SELECT name, email, dept_id FROM report_recipients WHERE id=?1 AND active=1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        let Some((name, email, dept_id)) = who else {
            out.failed += 1;
            out.details.push(format!("Recipient {id} is not in the list, or is switched off."));
            continue;
        };

        // Narrow to their department if they have one.
        let mut f = filters.clone();
        if let Some(d) = dept_id {
            f.dept_id = Some(d);
        }

        let report = match reports::run(&c, &key, &f) {
            Ok(r) => r,
            Err(e) => {
                out.failed += 1;
                out.details.push(format!("{email}: {e}"));
                continue;
            }
        };

        let mut html = report.to_html(&brand);
        if let Some(n) = note.as_deref().filter(|n| !n.trim().is_empty()) {
            // Insert the covering note above the table rather than appending
            // it, so it is the first thing read.
            let esc = n
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('\n', "<br>");
            html = html.replace(
                "<table>",
                &format!(
                    "<p style=\"background:#fff7ed;border-left:3px solid #F16522;\
                     padding:10px 12px;margin:0 0 16px\">{esc}</p><table>"
                ),
            );
        }

        let subject = format!("{brand} — {} ({} to {})", report.title, f.from, f.to);
        let result = mailer::send(&settings, &email, &subject, &html);

        let ok = result.is_ok();
        let detail = match &result {
            Ok(_) => format!("Sent to {name} <{email}>"),
            Err(e) => format!("{email}: {e}"),
        };
        if ok {
            out.sent += 1;
        } else {
            out.failed += 1;
        }
        out.details.push(detail.clone());

        let _ = c.execute(
            "INSERT INTO report_email_log (email, report_key, from_date, to_date, ok, detail)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![email, key, f.from, f.to, ok as i64, detail],
        );
    }

    let _ = db::audit(
        &c,
        "admin",
        "report.email",
        &format!("{key}: {} sent, {} failed", out.sent, out.failed),
    );
    let _ = app.emit_to_all("report-mail-done", out.sent as i64);
    Ok(out)
}

/// Small shim so the command above does not depend on Tauri's emit trait being
/// in scope everywhere.
trait EmitAll {
    fn emit_to_all(&self, event: &str, payload: i64) -> Result<(), tauri::Error>;
}
impl EmitAll for AppHandle {
    fn emit_to_all(&self, event: &str, payload: i64) -> Result<(), tauri::Error> {
        use tauri::Emitter;
        self.emit(event, payload)
    }
}

#[tauri::command(async)]
pub fn report_mail_log(state: State<'_, AppState>, limit: i64) -> R<Vec<serde_json::Value>> {
    let c = conn(&state)?;
    let mut s = c
        .prepare(
            "SELECT ts, email, report_key, from_date, to_date, ok, detail
             FROM report_email_log ORDER BY id DESC LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = s
        .query_map(params![limit.clamp(1, 500)], |r| {
            Ok(serde_json::json!({
                "ts": r.get::<_, String>(0)?,
                "email": r.get::<_, String>(1)?,
                "report": reports::title_of(&r.get::<_, String>(2)?),
                "from_date": r.get::<_, String>(3)?,
                "to_date": r.get::<_, String>(4)?,
                "ok": r.get::<_, i64>(5)? == 1,
                "detail": r.get::<_, Option<String>>(6)?,
            }))
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}
