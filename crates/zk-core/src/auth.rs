//! Administrator password storage.
//!
//! Passwords are stored as PBKDF2-HMAC-SHA256 with a per-password random salt,
//! never in plain text. The office PC sits in a staff room and the database
//! file is trivially copyable, so a leaked `attendance.db` must not hand over
//! a working login.

use crate::{Error, Result};
use hmac::Hmac;
use sha2::Sha256;

/// Iteration count. High enough to make guessing expensive, low enough that a
/// modest school PC still logs in instantly.
const ITERATIONS: u32 = 120_000;
const SALT_LEN: usize = 16;
const HASH_LEN: usize = 32;

/// The password every fresh install starts with.
pub const DEFAULT_PASSWORD: &str = "Attendance@123";

/// Hash a password, returning `pbkdf2$<iterations>$<salt_hex>$<hash_hex>`.
pub fn hash_password(password: &str) -> Result<String> {
    let mut salt = [0u8; SALT_LEN];
    getrandom(&mut salt)?;
    Ok(hash_with_salt(password, &salt, ITERATIONS))
}

fn hash_with_salt(password: &str, salt: &[u8], iterations: u32) -> String {
    let mut out = [0u8; HASH_LEN];
    pbkdf2::pbkdf2::<Hmac<Sha256>>(password.as_bytes(), salt, iterations, &mut out)
        .expect("HMAC accepts any key length");
    format!("pbkdf2${}${}${}", iterations, hex(salt), hex(&out))
}

/// Check a password against a stored hash.
///
/// Returns `false` for a malformed or empty stored value rather than erroring,
/// so a corrupted settings row locks the account instead of opening it.
pub fn verify_password(password: &str, stored: &str) -> bool {
    let parts: Vec<&str> = stored.split('$').collect();
    if parts.len() != 4 || parts[0] != "pbkdf2" {
        return false;
    }
    let Ok(iterations) = parts[1].parse::<u32>() else {
        return false;
    };
    let Some(salt) = unhex(parts[2]) else {
        return false;
    };
    let Some(expected) = unhex(parts[3]) else {
        return false;
    };
    if expected.len() != HASH_LEN || salt.is_empty() {
        return false;
    }

    let mut actual = [0u8; HASH_LEN];
    if pbkdf2::pbkdf2::<Hmac<Sha256>>(password.as_bytes(), &salt, iterations, &mut actual).is_err() {
        return false;
    }
    constant_time_eq(&actual, &expected)
}

/// Compare without leaking where the first difference is.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Basic strength check, with a message the office can act on.
pub fn check_strength(password: &str) -> Result<()> {
    if password.chars().count() < 8 {
        return Err(Error::Invalid("Password must be at least 8 characters.".into()));
    }
    if password == DEFAULT_PASSWORD {
        return Err(Error::Invalid(
            "That is still the default password. Please choose a different one.".into(),
        ));
    }
    let has_letter = password.chars().any(|c| c.is_alphabetic());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    if !has_letter || !has_digit {
        return Err(Error::Invalid("Password must contain both letters and numbers.".into()));
    }
    Ok(())
}

/// A 6-digit recovery code.
pub fn generate_reset_code() -> Result<String> {
    let mut b = [0u8; 4];
    getrandom(&mut b)?;
    let n = u32::from_le_bytes(b) % 1_000_000;
    Ok(format!("{n:06}"))
}

fn getrandom(buf: &mut [u8]) -> Result<()> {
    getrandom::fill(buf)
        .map_err(|e| Error::Invalid(format!("could not read secure random bytes: {e}")))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_verifies_against_its_own_hash() {
        let h = hash_password("Attendance@123").unwrap();
        assert!(verify_password("Attendance@123", &h));
        assert!(!verify_password("attendance@123", &h), "case matters");
        assert!(!verify_password("wrong", &h));
        assert!(!verify_password("", &h));
    }

    #[test]
    fn the_hash_is_salted_so_two_identical_passwords_differ() {
        let a = hash_password("SamePassword1").unwrap();
        let b = hash_password("SamePassword1").unwrap();
        assert_ne!(a, b, "identical passwords must not produce identical hashes");
        assert!(verify_password("SamePassword1", &a));
        assert!(verify_password("SamePassword1", &b));
    }

    #[test]
    fn the_plain_password_never_appears_in_the_stored_value() {
        let h = hash_password("Attendance@123").unwrap();
        assert!(!h.contains("Attendance"));
        assert!(h.starts_with("pbkdf2$120000$"));
    }

    #[test]
    fn a_corrupt_or_empty_stored_hash_denies_access() {
        for bad in ["", "garbage", "pbkdf2$notanumber$aa$bb", "pbkdf2$1000$zz$yy", "pbkdf2$1000$aa"]
        {
            assert!(!verify_password("anything", bad), "must reject {bad:?}");
        }
    }

    #[test]
    fn strength_rules_give_actionable_messages() {
        assert!(check_strength("Str0ngPass").is_ok());
        assert!(check_strength("short1").unwrap_err().to_string().contains("8 characters"));
        assert!(check_strength(DEFAULT_PASSWORD).unwrap_err().to_string().contains("default"));
        assert!(check_strength("nodigitshere").unwrap_err().to_string().contains("numbers"));
        assert!(check_strength("12345678").unwrap_err().to_string().contains("letters"));
    }

    #[test]
    fn reset_codes_are_six_digits_and_vary() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..50 {
            let c = generate_reset_code().unwrap();
            assert_eq!(c.len(), 6);
            assert!(c.chars().all(|c| c.is_ascii_digit()));
            seen.insert(c);
        }
        assert!(seen.len() > 40, "codes must not repeat constantly");
    }

    #[test]
    fn hex_helpers_round_trip() {
        let b = vec![0u8, 15, 16, 255, 128];
        assert_eq!(unhex(&hex(&b)).unwrap(), b);
        assert!(unhex("abc").is_none(), "odd length");
        assert!(unhex("zz").is_none(), "not hex");
    }
}
