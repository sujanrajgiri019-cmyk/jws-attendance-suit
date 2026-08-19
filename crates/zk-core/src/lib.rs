//! Core logic for the JWS Attendance Suite.
//!
//! This crate is deliberately free of any Tauri dependency so that the parts
//! that are easy to get subtly wrong — the ZKTeco wire format, the attendance
//! rules engine, the Bikram Sambat calendar — can be unit-tested anywhere,
//! including on a build machine with no access to the school's terminal.

pub mod auth;
pub mod calendar;
pub mod db;
pub mod proto;
pub mod pull;
pub mod push;
pub mod rules;
pub mod service;

use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("device protocol error: {0}")]
    Protocol(String),

    #[error("device did not respond in time")]
    Timeout,

    #[error("device refused the command (comm key wrong?)")]
    Unauthorised,

    #[error("network error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

// Serialising the error to the frontend as a plain string keeps the JS side
// simple; the variant detail is preserved in the message.
impl serde::Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
