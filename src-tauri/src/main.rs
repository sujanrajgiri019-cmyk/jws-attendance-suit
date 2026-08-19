// Hide the console window on Windows in release builds — the office should see
// an application, not a terminal.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    jws_attendance_lib::run()
}
