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
                let response = handle(&app, &db, &mut request);
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

fn handle(
    app: &AppHandle,
    db: &Arc<Mutex<Connection>>,
    request: &mut tiny_http::Request,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let url = request.url().to_string();
    let method = request.method().as_str().to_string();
    let serial = push::query_param(&url, "SN").unwrap_or_default();

    if serial.is_empty() {
        return text("OK\r\n".into());
    }

    // Any contact at all means the terminal is alive.
    if let Ok(conn) = db.lock() {
        let _ = zk_core::service::touch_device(&conn, &serial);
    }

    // --- handshake -------------------------------------------------------
    if url.starts_with("/iclock/cdata") && method == "GET" {
        tracing::info!("terminal {serial} completed handshake");
        let _ = app.emit("device-online", serial.clone());
        return text(push::handshake_response(&serial, 0, &PushConfig::default()));
    }

    // --- records ---------------------------------------------------------
    if url.starts_with("/iclock/cdata") && method == "POST" {
        let table = push::query_param(&url, "table").unwrap_or_default();
        let mut body = String::new();
        if request.as_reader().read_to_string(&mut body).is_err() {
            // A truncated body is not worth failing over; the terminal resends.
            return text("OK\r\n".into());
        }

        if !table.eq_ignore_ascii_case("ATTLOG") {
            // OPERLOG and friends are acknowledged so the device moves on.
            return text("OK\r\n".into());
        }

        let logs = push::parse_attlog_body(&body);
        if logs.is_empty() {
            return text("OK: 0\r\n".into());
        }

        let mut accepted = 0usize;
        if let Ok(mut conn) = db.lock() {
            match zk_core::db::insert_punches(&mut conn, &serial, "push", &logs) {
                Ok((a, dup)) => {
                    accepted = a;
                    tracing::info!("terminal {serial}: {a} new punches, {dup} already held");

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
        return text(format!("OK: {accepted}\r\n"));
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
                return text(format!("{payload}\r\n"));
            }
        }
        return text("OK\r\n".into());
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
        return text("OK\r\n".into());
    }

    text("OK\r\n".into())
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
