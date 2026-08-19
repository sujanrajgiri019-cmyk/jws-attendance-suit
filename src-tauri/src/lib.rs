//! JWS Attendance — application wiring.

mod commands;
mod reporting;
mod scheduling;
mod mailer;
mod push_server;

use std::sync::{Arc, Mutex};
use tauri::{Manager, WindowEvent};

use commands::AppState;

/// Where the app keeps its data. Under `%APPDATA%\JWS Attendance` on Windows.
pub fn app_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

pub fn db_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    app_dir(app).join("attendance.db")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,jws_attendance=debug".into()),
        )
        .with_target(false)
        .init();

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        builder = builder
            .plugin(tauri_plugin_updater::Builder::new().build())
            // A second copy would fight over the database file and the push
            // port, so focus the existing window instead of opening another.
            .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }));
    }

    builder
        .setup(|app| {
            let handle = app.handle().clone();
            let path = db_path(&handle);

            let conn = zk_core::db::open(&path).map_err(|e| {
                // Surfacing this as a startup error is better than a blank
                // window: the office needs to know the file could not be opened.
                format!("Could not open the attendance database at {}: {e}", path.display())
            })?;
            tracing::info!("database ready at {}", path.display());

            let db = Arc::new(Mutex::new(conn));

            app.manage(AppState {
                db: db.clone(),
                push: Mutex::new(None),
                reset: Mutex::new(None),
                started: std::time::Instant::now(),
            });

            // Bring today's attendance up to date on launch, so opening the app
            // after a weekend shows the right picture immediately.
            {
                let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                if let Ok(mut c) = db.lock() {
                    if let Err(e) = zk_core::service::recompute(&mut c, &today, &today) {
                        tracing::warn!("startup recompute failed: {e}");
                    }
                }
            }

            // Start the push listener if that is the configured mode. A failure
            // here is logged, not fatal — the app is still usable in pull mode.
            let mode = db
                .lock()
                .ok()
                .and_then(|c| zk_core::db::get_setting(&c, "connection_mode").ok().flatten())
                .unwrap_or_else(|| "push".into());
            if mode == "push" {
                let port = db
                    .lock()
                    .ok()
                    .and_then(|c| zk_core::db::get_setting(&c, "push_port").ok().flatten())
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(8081);
                match push_server::start(handle.clone(), db.clone(), port) {
                    Ok(l) => {
                        if let Some(state) = app.try_state::<AppState>() {
                            if let Ok(mut g) = state.push.lock() {
                                *g = Some(l);
                            }
                        }
                    }
                    Err(e) => tracing::error!("push listener did not start: {e}"),
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { .. } = event {
                // Checkpoint the WAL so the .db file on disk is complete for
                // whoever copies it to a USB stick.
                if let Some(state) = window.try_state::<AppState>() {
                    if let Ok(c) = state.db.lock() {
                        let _ = c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::dashboard,
            commands::attendance_trend,
            commands::department_stats,
            commands::punch_feed,
            commands::list_members,
            commands::save_member,
            commands::delete_members,
            commands::set_members_department,
            commands::list_departments,
            commands::save_department,
            commands::delete_department,
            commands::list_devices,
            commands::save_device,
            commands::device_ping,
            commands::device_info,
            commands::device_diagnose,
            commands::device_download_logs,
            commands::device_download_users,
            commands::device_upload_users,
            commands::push_start,
            commands::push_stop,
            commands::local_addresses,
            commands::attendance_range,
            commands::recompute,
            commands::override_attendance,
            commands::list_shifts,
            commands::list_timetables,
            commands::list_holidays,
            commands::save_holiday,
            commands::delete_holiday,
            commands::set_member_timetable,
            commands::get_rules,
            commands::set_rules,
            commands::get_settings,
            commands::set_settings,
            commands::browse_table,
            commands::sync_history,
            commands::report_summary,
            commands::export_csv,
            commands::auth_login,
            commands::auth_change_password,
            commands::auth_request_reset,
            commands::auth_verify_reset,
            commands::send_test_mail,
            commands::send_absence_emails,
            commands::backup_now,
            commands::list_backups,
            commands::open_path,
            commands::save_text_file,
            // --- Attendance rules -------------------------------------------
            scheduling::bs_calendar,
            scheduling::get_attendance_rules,
            scheduling::save_attendance_rules,
            // --- Timetables, shifts, schedules ------------------------------
            scheduling::list_timetables_full,
            scheduling::save_timetable,
            scheduling::delete_timetable,
            scheduling::list_shift_cycles,
            scheduling::save_shift,
            scheduling::delete_shift,
            scheduling::shift_grid,
            scheduling::add_shift_item,
            scheduling::delete_shift_item,
            scheduling::clear_shift_grid,
            scheduling::roster,
            scheduling::save_schedule,
            scheduling::delete_schedule,
            scheduling::arrange_shifts,
            scheduling::member_calendar,
            scheduling::department_tree,
            // --- Reports and delivery ---------------------------------------
            reporting::report_kinds,
            reporting::run_report,
            reporting::export_report,
            reporting::list_recipients,
            reporting::save_recipient,
            reporting::delete_recipient,
            reporting::send_report_email,
            reporting::report_mail_log,
        ])
        .run(tauri::generate_context!())
        .expect("JWS Attendance failed to start");
}
