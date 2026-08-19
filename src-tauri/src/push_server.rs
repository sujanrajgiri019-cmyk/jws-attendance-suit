//! The ADMS push listener.
//!
//! The terminal dials out to this server and posts each punch as it happens.
//! One blocking thread per request is more than enough — a school has a handful
//! of gates, not a thousand.
//!
//! Parsing lives in `zk_core::push` and is unit-tested there; this file is the
//! socket loop and the database writes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection};
use tauri::{AppHandle, Emitter};
use tiny_http::{Header, Response, Server};
use zk_core::push::{self, PushConfig};

pub struct PushListener {
    running: Arc<AtomicBool>,
    port: u16,
}

impl PushListener {
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

/// What the UI is told when a punch arrives.
#[derive(serde::Serialize, Clone)]
pub struct PunchEvent {
    pub enroll_no: i64,
    pub full_name: Option<String>,
    pub punch_time: String,
    pub punch_state: i64,
    pub device_serial: String,
}

/// Start listening. Returns an error immediately if the port is taken, rather
/// than failing silently and leaving the office wondering why nothing arrives.
pub fn start(
    app: AppHandle,
    db: Arc<Mutex<Connection>>,
    port: u16,
) -> Result<PushListener, String> {
    let addr = format!("0.0.0.0:{port}");
    let server = Server::http(&addr).map_err(|e| {
        format!(
            "Could not listen on port {port}: {e}. \
             Another program may be using it, or Windows Firewall is blocking it."
        )
    })?;

    let running = Arc::new(AtomicBool::new(true));
    let flag = running.clone();

    std::thread::Builder::new()
        .name("adms-push".into())
        .spawn(move || {
            tracing::info!("push listener bound to {addr}");
            for mut request in server.incoming_requests() {
                if !flag.load(Ordering::SeqCst) {
                    break;
                }

                // Everything the terminal says is recorded before we answer it.
                // A device that posts punches but never asks for commands looks
                // identical from the outside to one that asks and finds none —
                // and the two have completely different fixes.
                let url = request.url().to_string();
                let method = request.method().as_str().to_string();
                let serial = push::query_param(&url, "SN").unwrap_or_default();
                let table = push::query_param(&url, "table").unwrap_or_default();
                let endpoint = url.split('?').next().unwrap_or(&url).to_string();

                let outcome = handle(&app, &db, &mut request);
                let (response, records, reply) = outcome;

                if let Ok(conn) = db.lock() {
                    zk_core::db::log_device_request(
                        &conn, &serial, &method, &endpoint, &table, 0, records, &reply,
                    );
                }

                if let Err(e) = request.respond(response) {
                    tracing::warn!("failed to answer terminal: {e}");
                }
            }
            tracing::info!("push listener stopped");
        })
        .map_err(|e| format!("could not start the listener thread: {e}"))?;

    Ok(PushListener { running, port })
}

fn text(body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut r = Response::from_string(body);
    // Terminals are fussy: they expect plain text and a definite length.
    if let Ok(h) = Header::from_bytes(&b"Content-Type"[..], &b"text/plain; charset=utf-8"[..]) {
        r.add_header(h);
    }
    r
}

/// Answer one request, and report how many records it carried plus the reply
/// we gave — both of which go into the device log.
type Handled = (Response<std::io::Cursor<Vec<u8>>>, usize, String);

fn handle(
    app: &AppHandle,
    db: &Arc<Mutex<Connection>>,
    request: &mut tiny_http::Request,
) -> Handled {
    let url = request.url().to_string();
    let method = request.method().as_str().to_string();
    let serial = push::query_param(&url, "SN").unwrap_or_default();

    if serial.is_empty() {
        return (text("OK\r\n".into()), 0, "OK (no serial)".into());
    }

    // Any contact at all means the terminal is alive.
    if let Ok(conn) = db.lock() {
        let _ = zk_core::service::touch_device(&conn, &serial);
    }

    // --- handshake -------------------------------------------------------
    if url.starts_with("/iclock/cdata") && method == "GET" {
        tracing::info!("terminal {serial} completed handshake");
        let _ = app.emit("device-online", serial.clone());
        let body = push::handshake_response(&serial, 0, &PushConfig::default());
        return (text(body), 0, "handshake".into());
    }

    // --- records ---------------------------------------------------------
    if url.starts_with("/iclock/cdata") && method == "POST" {
        let table = push::query_param(&url, "table").unwrap_or_default();
        let mut body = String::new();
        if request.as_reader().read_to_string(&mut body).is_err() {
            // A truncated body is not worth failing over; the terminal resends.
            return (text("OK\r\n".into()), 0, "truncated body".into());
        }

        // Fingerprint templates, which arrive as their own table alongside the
        // user list. Stored verbatim so they can be pushed back to a
        // replacement terminal.
        if table.eq_ignore_ascii_case("FINGERTMP") {
            let fps = push::parse_fingerprint_body(&body);
            let mut stored = 0usize;
            if let Ok(conn) = db.lock() {
                for f in &fps {
                    let Ok(enroll) = f.pin.trim().parse::<i64>() else { continue };
                    if conn
                        .execute(
                            "INSERT INTO member_fingerprints
                                (enroll_no, finger_index, template, size, valid, device_serial,
                                 updated_at)
                             VALUES (?1,?2,?3,?4,?5,?6, datetime('now','localtime'))
                             ON CONFLICT(enroll_no, finger_index) DO UPDATE SET
                                 template=excluded.template, size=excluded.size,
                                 valid=excluded.valid, device_serial=excluded.device_serial,
                                 updated_at=excluded.updated_at",
                            params![
                                enroll,
                                f.index as i64,
                                f.template,
                                f.size,
                                f.valid as i64,
                                serial
                            ],
                        )
                        .is_ok()
                    {
                        stored += 1;
                    }
                }
                // Keep the count on the member record in step, so the Members
                // screen shows who is actually enrolled on the sensor.
                let _ = conn.execute(
                    "UPDATE members SET fp_count = (
                         SELECT COUNT(*) FROM member_fingerprints f
                         WHERE f.enroll_no = members.enroll_no AND f.valid = 1)",
                    [],
                );
                tracing::info!("terminal {serial}: {stored} fingerprint templates stored");
                if stored > 0 {
                    let _ = zk_core::db::log_sync(
                        &conn,
                        "Receive fingerprints",
                        &serial,
                        &format!("{stored} templates"),
                        true,
                    );
                }
            }
            let _ = app.emit("transfer-progress", format!("{stored} fingerprint templates"));
            return (text(format!("OK: {stored}\r\n")), stored, format!("OK: {stored} templates"));
        }

        // The user table, usually arriving because the Data Transfer screen
        // asked for it. Handled before ATTLOG so a requested download actually
        // lands instead of being acknowledged and thrown away.
        if table.eq_ignore_ascii_case("USERINFO") || table.eq_ignore_ascii_case("OPERLOG") {
            let users = push::parse_userinfo_body(&body);
            let mut added = 0usize;
            let mut updated = 0usize;
            if !users.is_empty() {
                if let Ok(conn) = db.lock() {
                    for u in &users {
                        let Ok(enroll) = u.pin.trim().parse::<i64>() else { continue };
                        let name = if u.name.trim().is_empty() {
                            format!("Unnamed {enroll}")
                        } else {
                            u.name.trim().to_string()
                        };
                        let exists: bool = conn
                            .query_row(
                                "SELECT 1 FROM members WHERE enroll_no=?1",
                                params![enroll],
                                |_| Ok(true),
                            )
                            .unwrap_or(false);

                        if exists {
                            // Never overwrite a name the office has corrected
                            // here; the terminal's 24-character version is the
                            // worse of the two. Only fill in what is missing.
                            let n = conn
                                .execute(
                                    "UPDATE members
                                       SET card_no = COALESCE(NULLIF(card_no,''), ?2),
                                           privilege = ?3,
                                           device_name = COALESCE(NULLIF(device_name,''), ?4),
                                           updated_at = datetime('now','localtime')
                                     WHERE enroll_no = ?1",
                                    params![enroll, u.card, u.privilege as i64, name],
                                )
                                .unwrap_or(0);
                            updated += n;
                        } else if conn
                            .execute(
                                "INSERT INTO members
                                   (enroll_no, full_name, device_name, card_no, privilege, status)
                                 VALUES (?1, ?2, ?2, ?3, ?4, 'Active')",
                                params![enroll, name, u.card, u.privilege as i64],
                            )
                            .is_ok()
                        {
                            added += 1;
                        }
                    }
                    tracing::info!(
                        "terminal {serial}: {added} users added, {updated} updated"
                    );
                    let _ = zk_core::db::log_sync(
                        &conn,
                        "Receive users",
                        &serial,
                        &format!("{added} added, {updated} updated"),
                        true,
                    );
                    let _ = app.emit("users-received", added as i64);
                    let _ = app.emit(
                        "transfer-progress",
                        format!("{added} users added, {updated} updated"),
                    );
                }
            }
            return (
                text(format!("OK: {}\r\n", users.len())),
                users.len(),
                format!("OK: {} users", users.len()),
            );
        }

        if !table.eq_ignore_ascii_case("ATTLOG") {
            // Anything else is acknowledged so the device moves on.
            return (text("OK\r\n".into()), 0, format!("ignored table {table}"));
        }

        let logs = push::parse_attlog_body(&body);
        if logs.is_empty() {
            return (text("OK: 0\r\n".into()), 0, "OK: 0 (empty)".into());
        }

        let mut accepted = 0usize;
        if let Ok(mut conn) = db.lock() {
            match zk_core::db::insert_punches(&mut conn, &serial, "push", &logs) {
                Ok((a, dup)) => {
                    accepted = a;
                    tracing::info!("terminal {serial}: {a} new punches, {dup} already held");
                    let _ = app.emit(
                        "transfer-progress",
                        format!("{a} new punches received, {dup} already held"),
                    );
                    // Logged so a Download-logs request can tell that its
                    // answer has landed, rather than guessing from a timeout.
                    let _ = zk_core::db::log_sync(
                        &conn,
                        "Receive punches",
                        &serial,
                        &format!("{a} new, {dup} already held"),
                        true,
                    );

                    // Recompute only the days actually touched, so a live punch
                    // updates the dashboard without rebuilding the month.
                    let mut days: Vec<String> = logs.iter().map(|l| l.date()).collect();
                    days.sort();
                    days.dedup();
                    for d in &days {
                        if let Err(e) = zk_core::service::recompute(&mut conn, d, d) {
                            tracing::warn!("recompute for {d} failed: {e}");
                        }
                    }

                    for l in &logs {
                        let enroll: i64 = l.user_id.trim().parse().unwrap_or(0);

                        // Someone scanned whose enrolment number we have never
                        // seen. Create a placeholder rather than dropping the
                        // punch on the floor: the office can put a name to it
                        // later, and the attendance history is preserved from
                        // the first scan because everything joins on the
                        // enrolment number, not the name.
                        if enroll > 0 {
                            let known: bool = conn
                                .query_row(
                                    "SELECT 1 FROM members WHERE enroll_no=?1",
                                    params![enroll],
                                    |_| Ok(true),
                                )
                                .unwrap_or(false);
                            if !known {
                                let placeholder = format!("Unnamed {enroll}");
                                if conn
                                    .execute(
                                        "INSERT OR IGNORE INTO members
                                           (enroll_no, full_name, device_name, status)
                                         VALUES (?1, ?2, ?2, 'Active')",
                                        params![enroll, placeholder],
                                    )
                                    .is_ok()
                                {
                                    tracing::info!("created placeholder member for enrolment {enroll}");
                                    let _ = zk_core::db::audit(
                                        &conn,
                                        "system",
                                        "member.auto_created",
                                        &format!("enrolment {enroll} seen on {serial}"),
                                    );
                                }
                            }
                        }

                        let name: Option<String> = conn
                            .query_row(
                                "SELECT full_name FROM members WHERE enroll_no=?1",
                                params![enroll],
                                |r| r.get(0),
                            )
                            .ok();
                        let _ = app.emit(
                            "punch",
                            PunchEvent {
                                enroll_no: enroll,
                                full_name: name,
                                punch_time: l.timestamp(),
                                punch_state: l.punch as i64,
                                device_serial: serial.clone(),
                            },
                        );
                    }
                }
                Err(e) => tracing::error!("could not store punches from {serial}: {e}"),
            }
        }

        // The device uses this count to advance its own pointer.
        return (
            text(format!("OK: {accepted}\r\n")),
            accepted,
            format!("OK: {accepted} punches"),
        );
    }

    // --- device asking for work ------------------------------------------
    if url.starts_with("/iclock/getrequest") {
        if let Ok(conn) = db.lock() {
            let pending: Option<(i64, String)> = conn
                .query_row(
                    "SELECT id, payload FROM device_commands
                     WHERE device_serial = ?1 AND sent_at IS NULL
                     ORDER BY id LIMIT 1",
                    params![serial],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .ok();

            if let Some((id, payload)) = pending {
                let _ = conn.execute(
                    "UPDATE device_commands SET sent_at = datetime('now','localtime') WHERE id=?1",
                    params![id],
                );
                tracing::info!("handed command {id} to {serial}");
                return (
                    text(format!("{payload}\r\n")),
                    1,
                    format!("sent command {id}: {payload}"),
                );
            }
        }
        // The device asked for work and there was none. Logged explicitly,
        // because "asked and found nothing" and "never asked" are the two
        // possibilities that matter and they must be distinguishable.
        return (text("OK\r\n".into()), 0, "no commands pending".into());
    }

    // --- device reporting a command result --------------------------------
    if url.starts_with("/iclock/devicecmd") {
        let mut body = String::new();
        let _ = request.as_reader().read_to_string(&mut body);
        if let Ok(conn) = db.lock() {
            // Body looks like `ID=12&Return=0&CMD=DATA`.
            if let Some(id) = push::query_param(&format!("?{body}"), "ID")
                .and_then(|v| v.trim().parse::<i64>().ok())
            {
                let ret = push::query_param(&format!("?{body}"), "Return").unwrap_or_default();
                let _ = conn.execute(
                    "UPDATE device_commands SET result=?1 WHERE id=?2",
                    params![ret, id],
                );
            }
        }
        return (text("OK\r\n".into()), 0, format!("command result: {body}"));
    }

    (text("OK\r\n".into()), 0, "OK".into())
}

/// Queue a command for a terminal to collect on its next poll.
pub fn queue_command(conn: &Connection, serial: &str, cmd: &push::DeviceCommand) -> Result<i64, String> {
    // The id embedded in the payload has to match the row id the device reports
    // back, so insert a placeholder first and then fill it in.
    conn.execute(
        "INSERT INTO device_commands(device_serial, payload) VALUES(?1, '')",
        params![serial],
    )
    .map_err(|e| e.to_string())?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "UPDATE device_commands SET payload=?1 WHERE id=?2",
        params![cmd.encode(id as u64), id],
    )
    .map_err(|e| e.to_string())?;
    Ok(id)
}
