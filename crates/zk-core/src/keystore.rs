//! Encryption at rest for fingerprint templates.
//!
//! ## Why this exists
//!
//! A password can be changed after a leak. A fingerprint cannot — the school
//! cannot issue a member of staff a new finger. And this database is built to
//! be copied: `VACUUM INTO` backups exist precisely so the office can put
//! `attendance.db` on a memory stick and take it home, and the README tells
//! them to. Storing raw templates in that file means every one of those copies
//! is a permanent biometric disclosure waiting to be mislaid.
//!
//! So templates are sealed, and the key lives in a separate file beside the
//! database rather than inside it. A key kept in the `settings` table would
//! travel with every backup and protect nothing.
//!
//! ## The threat this does and does not stop
//!
//! It stops a copied, mailed or mislaid `attendance.db` from yielding
//! templates. That is the realistic exposure for a PC in a staff room.
//!
//! It does not stop someone who has the machine itself, since the key file sits
//! next to the database and the app must be able to read it unattended — the
//! push listener receives fingerprints at three in the morning with nobody to
//! type a passphrase. Binding the key to the machine (Windows DPAPI) would
//! close that gap and would also make the key unrestorable onto a replacement
//! PC, which for a school with one computer and no IT staff trades a small risk
//! for a much likelier disaster.
//!
//! **Back the key file up separately from the database.** Lose it and every
//! stored template is gone — which is survivable, since staff can re-enrol at
//! the terminal — but restoring a backup onto a new machine without it means
//! doing exactly that, for everyone, on the same morning.
//!
//! ## Construction
//!
//! Encrypt-then-MAC, built from the HMAC-SHA256 already in this crate for
//! password hashing:
//!
//! - Three subkeys are derived from the master key with HKDF-SHA256 (RFC 5869):
//!   one to encrypt, one to authenticate, one for change-detection digests.
//!   Separate keys because reusing one across purposes is how these
//!   constructions usually break.
//! - The keystream is `HMAC(enc_key, nonce || counter)` per 32-byte block —
//!   HMAC used as a PRF in counter mode, which is what HKDF-Expand and NIST
//!   SP 800-108 already do. The nonce is 24 random bytes, wide enough that
//!   repetition is not a concern this side of the heat death of the office PC.
//! - The tag is `HMAC(mac_key, version || nonce || aad || ciphertext)`, checked
//!   in constant time *before* anything is decrypted.
//! - The AAD is the enrolment number and finger index, so a sealed template is
//!   bound to the row it belongs to. Moving ciphertext from one person's row to
//!   another's fails to open rather than silently giving them each other's
//!   finger.
//!
//! No new dependency. That is deliberate and not merely tidiness: the school PC
//! builds this application from source, and adding a crate means the next build
//! on a machine without internet fails. `hmac`, `sha2` and `getrandom` are
//! already in the tree for `auth`.

use crate::{Error, Result};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::io::Write;
use std::path::Path;

type HmacSha256 = Hmac<Sha256>;

/// Envelope format version, stored as the first byte of every sealed blob.
pub const ENVELOPE_VERSION: u8 = 1;

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 32;
const BLOCK: usize = 32;

/// Smallest possible sealed blob: version + nonce + one byte + tag.
const MIN_SEALED: usize = 1 + NONCE_LEN + 1 + TAG_LEN;

const KEY_FILE_MAGIC: &str = "jws-attendance biometric key v1";

/// The derived keys used to seal and open templates.
///
/// Deliberately not `Clone`, `Debug` or `Serialize`: the one way this material
/// escapes is by someone adding a derive to make a log line easier to write.
pub struct BioKey {
    enc: [u8; KEY_LEN],
    mac: [u8; KEY_LEN],
    dig: [u8; KEY_LEN],
}

impl Drop for BioKey {
    fn drop(&mut self) {
        // Volatile writes so the optimiser cannot decide that scrubbing memory
        // nobody reads again is dead code. It is allowed to think that; it is
        // wrong about why we are doing it.
        for b in self.enc.iter_mut().chain(self.mac.iter_mut()).chain(self.dig.iter_mut()) {
            unsafe { std::ptr::write_volatile(b, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

impl BioKey {
    /// Derive the working keys from 32 bytes of master key material.
    pub fn from_master(master: &[u8]) -> Result<Self> {
        if master.len() < KEY_LEN {
            return Err(Error::Invalid(format!(
                "biometric master key must be at least {KEY_LEN} bytes, got {}",
                master.len()
            )));
        }
        let prk = hkdf_extract(KEY_FILE_MAGIC.as_bytes(), master);
        let mut k = BioKey { enc: [0; KEY_LEN], mac: [0; KEY_LEN], dig: [0; KEY_LEN] };
        hkdf_expand(&prk, b"template-encryption", &mut k.enc);
        hkdf_expand(&prk, b"template-authentication", &mut k.mac);
        hkdf_expand(&prk, b"template-digest", &mut k.dig);
        Ok(k)
    }

    /// Load the key beside the database, creating one on first use.
    ///
    /// The write is atomic — a half-written key file would make every stored
    /// template permanently unreadable, and it would happen during exactly the
    /// power cut this school gets regularly.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if path.exists() {
            let text = std::fs::read_to_string(path)?;
            return Self::from_master(&parse_key_file(&text, path)?);
        }

        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }

        let mut master = [0u8; KEY_LEN];
        getrandom(&mut master)?;

        let body = format!(
            "{KEY_FILE_MAGIC}\n\
             # This key unlocks the fingerprint templates in attendance.db.\n\
             # Keep it OUT of the database backups, and keep a copy somewhere safe:\n\
             # without it the stored templates cannot be recovered and every member\n\
             # of staff has to enrol their fingers at the terminal again.\n\
             {}\n",
            hex(&master)
        );

        let tmp = path.with_extension("key.tmp");
        {
            let mut f = std::fs::File::create(&tmp)?;
            restrict_permissions(&f)?;
            f.write_all(body.as_bytes())?;
            // The rename below is atomic, but only orders against data that has
            // actually reached the disk.
            f.sync_all()?;
        }
        std::fs::rename(&tmp, path)?;

        tracing::info!("created a new biometric key at {}", path.display());
        Self::from_master(&master)
    }

    /// Seal a template for storage, bound to the row it belongs to.
    pub fn seal(&self, enroll_no: i64, finger: u8, plaintext: &[u8]) -> Result<Vec<u8>> {
        if plaintext.is_empty() {
            return Err(Error::Invalid("refusing to seal an empty template".into()));
        }

        let mut nonce = [0u8; NONCE_LEN];
        getrandom(&mut nonce)?;

        let mut out = Vec::with_capacity(1 + NONCE_LEN + plaintext.len() + TAG_LEN);
        out.push(ENVELOPE_VERSION);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(plaintext);
        // Encrypt in place over the ciphertext region.
        let ct_from = 1 + NONCE_LEN;
        self.apply_keystream(&nonce, &mut out[ct_from..]);

        let tag = self.tag(&nonce, enroll_no, finger, &out[ct_from..]);
        out.extend_from_slice(&tag);
        Ok(out)
    }

    /// Open a sealed template. Fails rather than returning anything doubtful.
    pub fn open(&self, enroll_no: i64, finger: u8, sealed: &[u8]) -> Result<Vec<u8>> {
        if sealed.len() < MIN_SEALED {
            return Err(Error::Invalid(format!(
                "sealed template is {} bytes, too short to be one",
                sealed.len()
            )));
        }
        if sealed[0] != ENVELOPE_VERSION {
            return Err(Error::Invalid(format!(
                "sealed template is format version {}, and this build understands {ENVELOPE_VERSION}",
                sealed[0]
            )));
        }

        let nonce = &sealed[1..1 + NONCE_LEN];
        let ct = &sealed[1 + NONCE_LEN..sealed.len() - TAG_LEN];
        let tag = &sealed[sealed.len() - TAG_LEN..];

        let mut n = [0u8; NONCE_LEN];
        n.copy_from_slice(nonce);
        let expected = self.tag(&n, enroll_no, finger, ct);

        // Authenticate before decrypting. Doing it the other way round hands an
        // attacker a decryption oracle, and here it would also mean writing a
        // forged template to a terminal.
        if !constant_time_eq(&expected, tag) {
            return Err(Error::Invalid(
                "a stored fingerprint template failed its integrity check. It was either \
                 altered, or it belongs to a different person or finger than the row it \
                 was found in. It has not been used."
                    .into(),
            ));
        }

        let mut plain = ct.to_vec();
        self.apply_keystream(&n, &mut plain);
        Ok(plain)
    }

    /// A short keyed digest of a *plaintext* template.
    ///
    /// Lets a re-sync answer "is this the same finger we already hold?" without
    /// unsealing anything, and lets an upload verify what the terminal actually
    /// stored. Keyed rather than a plain SHA-256 because an unkeyed digest of a
    /// biometric template lets anyone holding the database confirm a guessed or
    /// separately obtained template — the digest would become the disclosure the
    /// encryption was there to prevent.
    pub fn digest(&self, plaintext: &[u8]) -> String {
        let mut m = new_mac(&self.dig);
        m.update(b"template-digest-v1");
        m.update(plaintext);
        // 16 bytes is far beyond what a collision here would need to be.
        hex(&m.finalize().into_bytes()[..16])
    }

    fn tag(&self, nonce: &[u8; NONCE_LEN], enroll_no: i64, finger: u8, ct: &[u8]) -> [u8; TAG_LEN] {
        let mut m = new_mac(&self.mac);
        m.update(&[ENVELOPE_VERSION]);
        m.update(nonce);
        // The associated data: which row this ciphertext is allowed to sit in.
        m.update(&enroll_no.to_le_bytes());
        m.update(&[finger]);
        m.update(ct);
        let out = m.finalize().into_bytes();
        let mut t = [0u8; TAG_LEN];
        t.copy_from_slice(&out);
        t
    }

    /// XOR `buf` with `HMAC(enc, nonce || counter)`, block by block.
    fn apply_keystream(&self, nonce: &[u8; NONCE_LEN], buf: &mut [u8]) {
        let mut counter: u32 = 0;
        let mut at = 0usize;
        while at < buf.len() {
            let mut m = new_mac(&self.enc);
            m.update(nonce);
            m.update(&counter.to_be_bytes());
            let block = m.finalize().into_bytes();

            let n = (buf.len() - at).min(BLOCK);
            for i in 0..n {
                buf[at + i] ^= block[i];
            }
            at += n;
            counter += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// HKDF (RFC 5869) over HMAC-SHA256
// ---------------------------------------------------------------------------

fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; 32] {
    let mut m = new_mac(salt);
    m.update(ikm);
    let out = m.finalize().into_bytes();
    let mut prk = [0u8; 32];
    prk.copy_from_slice(&out);
    prk
}

fn hkdf_expand(prk: &[u8; 32], info: &[u8], out: &mut [u8]) {
    debug_assert!(out.len() <= 255 * BLOCK, "HKDF cannot expand this far");
    let mut previous: Vec<u8> = Vec::new();
    let mut counter: u8 = 1;
    let mut at = 0usize;

    while at < out.len() {
        let mut m = new_mac(prk);
        m.update(&previous);
        m.update(info);
        m.update(&[counter]);
        let block = m.finalize().into_bytes();

        let n = (out.len() - at).min(BLOCK);
        out[at..at + n].copy_from_slice(&block[..n]);
        previous = block.to_vec();
        at += n;
        counter += 1;
    }
}

fn new_mac(key: &[u8]) -> HmacSha256 {
    <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC accepts a key of any length")
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn parse_key_file(text: &str, path: &Path) -> Result<Vec<u8>> {
    let mut saw_magic = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !saw_magic {
            if line != KEY_FILE_MAGIC {
                return Err(Error::Invalid(format!(
                    "{} does not look like a JWS biometric key file",
                    path.display()
                )));
            }
            saw_magic = true;
            continue;
        }
        let key = unhex(line).ok_or_else(|| {
            Error::Invalid(format!("the key in {} is not valid hex", path.display()))
        })?;
        if key.len() != KEY_LEN {
            return Err(Error::Invalid(format!(
                "the key in {} is {} bytes; {KEY_LEN} were expected",
                path.display(),
                key.len()
            )));
        }
        return Ok(key);
    }
    Err(Error::Invalid(format!(
        "{} contains no key. If it has been truncated, restore it from your own copy — \
         the templates in the database cannot be read without it.",
        path.display()
    )))
}

#[cfg(unix)]
fn restrict_permissions(f: &std::fs::File) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut p = f.metadata()?.permissions();
    p.set_mode(0o600);
    f.set_permissions(p)?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_f: &std::fs::File) -> Result<()> {
    // On Windows the file inherits the ACL of %APPDATA%\JWS Attendance, which is
    // already restricted to the logged-in user and Administrators. Setting a
    // tighter ACL from here would need the winapi surface that this crate has
    // deliberately stayed clear of, and would break the office's own backups.
    Ok(())
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> BioKey {
        BioKey::from_master(&[7u8; 32]).unwrap()
    }

    #[test]
    fn hkdf_matches_rfc_5869_test_case_1() {
        // Pinning the derivation against the published vectors, because a
        // subtly wrong HKDF still produces plausible-looking keys — and would
        // silently make every template written by one build unreadable by the
        // next.
        let ikm = [0x0bu8; 22];
        let salt: Vec<u8> = (0u8..=0x0c).collect();
        let info: Vec<u8> = (0xf0u8..=0xf9).collect();

        let prk = hkdf_extract(&salt, &ikm);
        assert_eq!(
            hex(&prk),
            "077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5"
        );

        let mut okm = [0u8; 42];
        hkdf_expand(&prk, &info, &mut okm);
        assert_eq!(
            hex(&okm),
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
        );
    }

    #[test]
    fn a_template_round_trips() {
        let k = key();
        let plain = b"a vendor blob standing in for a fingerprint".to_vec();
        let sealed = k.seal(41, 6, &plain).unwrap();
        assert_eq!(k.open(41, 6, &sealed).unwrap(), plain);
    }

    #[test]
    fn the_plaintext_does_not_appear_in_the_sealed_blob() {
        let k = key();
        let plain = b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_vec();
        let sealed = k.seal(1, 0, &plain).unwrap();
        assert!(
            !sealed.windows(plain.len()).any(|w| w == &plain[..]),
            "the template survived sealing in the clear"
        );
        // A long run of identical bytes must not produce a repeating pattern,
        // which is what a keystream that ignored the block counter would do.
        let ct = &sealed[1 + NONCE_LEN..sealed.len() - TAG_LEN];
        assert_ne!(&ct[..16], &ct[32..48], "keystream repeats every block");
    }

    #[test]
    fn a_template_cannot_be_moved_to_another_person_or_finger() {
        // The failure this prevents is quiet and serious: two rows swapped, and
        // one member of staff opening the gate as another.
        let k = key();
        let sealed = k.seal(41, 6, b"fingerprint").unwrap();
        assert!(k.open(42, 6, &sealed).is_err(), "opened under the wrong enrolment number");
        assert!(k.open(41, 7, &sealed).is_err(), "opened under the wrong finger");
        assert!(k.open(41, 6, &sealed).is_ok());
    }

    #[test]
    fn tampering_is_detected_rather_than_decrypted() {
        let k = key();
        let sealed = k.seal(41, 6, b"fingerprint template").unwrap();

        for spot in [0usize, 1, 1 + NONCE_LEN, sealed.len() - 1] {
            let mut bad = sealed.clone();
            bad[spot] ^= 0x01;
            assert!(k.open(41, 6, &bad).is_err(), "byte {spot} could be flipped undetected");
        }

        let mut short = sealed.clone();
        short.truncate(sealed.len() - 1);
        assert!(k.open(41, 6, &short).is_err());
        assert!(k.open(41, 6, &[]).is_err());
        assert!(k.open(41, 6, &[0u8; 8]).is_err());
    }

    #[test]
    fn a_different_key_cannot_open_it() {
        let sealed = key().seal(41, 6, b"fingerprint").unwrap();
        let other = BioKey::from_master(&[8u8; 32]).unwrap();
        assert!(other.open(41, 6, &sealed).is_err());
    }

    #[test]
    fn sealing_twice_gives_different_blobs() {
        // A fixed nonce would make identical templates visibly identical in the
        // database, which is most of what encryption is meant to hide.
        let k = key();
        let a = k.seal(41, 6, b"fingerprint").unwrap();
        let b = k.seal(41, 6, b"fingerprint").unwrap();
        assert_ne!(a, b);
        assert_eq!(k.open(41, 6, &a).unwrap(), k.open(41, 6, &b).unwrap());
    }

    #[test]
    fn digests_are_stable_keyed_and_distinguishing() {
        let k = key();
        assert_eq!(k.digest(b"same"), k.digest(b"same"));
        assert_ne!(k.digest(b"same"), k.digest(b"different"));
        // Keyed: another installation must not be able to recognise our
        // templates from their digests.
        let other = BioKey::from_master(&[8u8; 32]).unwrap();
        assert_ne!(k.digest(b"same"), other.digest(b"same"));
        assert_eq!(k.digest(b"x").len(), 32, "16 bytes as hex");
    }

    #[test]
    fn sealing_an_empty_template_is_refused() {
        assert!(key().seal(41, 6, b"").is_err());
    }

    #[test]
    fn odd_sized_templates_round_trip() {
        // Exercises the keystream across block boundaries, where an off-by-one
        // would corrupt only the tail — the kind of bug that looks like a
        // fingerprint that nearly works.
        let k = key();
        for len in [1usize, 31, 32, 33, 64, 65, 600, 1024, 1600] {
            let plain: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let sealed = k.seal(1, 0, &plain).unwrap();
            assert_eq!(k.open(1, 0, &sealed).unwrap(), plain, "failed at {len} bytes");
        }
    }

    #[test]
    fn a_key_file_is_created_once_and_reloaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("biometrics.key");

        let k1 = BioKey::load_or_create(&path).unwrap();
        assert!(path.exists());
        let sealed = k1.seal(41, 6, b"fingerprint").unwrap();

        // Reopening must give the same key, or every stored template is lost on
        // the next restart.
        let k2 = BioKey::load_or_create(&path).unwrap();
        assert_eq!(k2.open(41, 6, &sealed).unwrap(), b"fingerprint".to_vec());

        // No scratch file left behind.
        assert!(!path.with_extension("key.tmp").exists());
    }

    #[test]
    fn a_damaged_key_file_is_refused_rather_than_guessed() {
        let dir = tempfile::tempdir().unwrap();
        for (name, body) in [
            ("wrong-magic.key", "something else\nAABB\n"),
            ("no-key.key", "jws-attendance biometric key v1\n# only comments\n"),
            ("bad-hex.key", "jws-attendance biometric key v1\nnot-hex-at-all\n"),
            ("short.key", "jws-attendance biometric key v1\nAABBCC\n"),
        ] {
            let p = dir.path().join(name);
            std::fs::write(&p, body).unwrap();
            assert!(BioKey::load_or_create(&p).is_err(), "{name} should have been refused");
        }
    }

    #[test]
    fn a_key_file_with_comments_and_blank_lines_loads() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ok.key");
        std::fs::write(
            &p,
            format!("\n# a note\n{KEY_FILE_MAGIC}\n\n# another\n{}\n", "ab".repeat(32)),
        )
        .unwrap();
        assert!(BioKey::load_or_create(&p).is_ok());
    }
}
