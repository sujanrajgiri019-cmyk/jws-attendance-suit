//! Fingerprint template transfer over the direct protocol (TCP 4370).
//!
//! [`crate::proto`] carries the user table and the attendance log. This module
//! adds the third table on the terminal: the fingerprint templates themselves.
//!
//! ## What the ZKTeco SDK calls these
//!
//! The Windows `zkemkeeper.dll` Standalone SDK wraps the same packets in COM
//! methods. The mapping, because every ZKTeco example on the internet is
//! written against those names:
//!
//! | `zkemkeeper.dll`            | Here                                        |
//! |-----------------------------|---------------------------------------------|
//! | `ReadAllTemplate`           | [`CMD_DB_RRQ`] + [`FCT_FINGERTMP`] buffer   |
//! | `GetUserTmpExStr`           | [`parse_template_table`] over that buffer   |
//! | `SSR_GetUserTmpStr`         | [`build_get_template_request`] (one finger) |
//! | `SetUserTmpExStr`           | [`encode_template_record`] + `CMD_USERTEMP_WRQ` |
//! | `SSR_DelUserTmpExt`         | [`build_delete_template_request`]           |
//! | `RefreshData(1)`            | `proto::CMD_REFRESH_DATA`                   |
//!
//! There is no COM object involved here — that DLL is 32-bit Windows-only and
//! would drag a .NET or C++ host into a Rust application that currently has
//! neither. These are the packets the DLL sends, which is what the terminal
//! actually understands.
//!
//! ## The template blob itself
//!
//! Never interpreted. It is a vendor-encoded minutiae set whose format changes
//! between finger algorithm versions, and the only thing this application ever
//! needs to do with one is hand it back to a terminal unchanged. Re-encoding it
//! — even "harmlessly", even just to normalise base64 padding — risks producing
//! a template the sensor rejects, and that failure only shows up as a member of
//! staff whose finger stops working.
//!
//! Everything in this file is a pure function over bytes, for the same reason
//! `proto` is: the school's terminal is not reachable from a build machine, so
//! the wire format is pinned by unit tests over synthetic fixtures and only the
//! socket plumbing waits for real hardware.

use crate::{Error, Result};

// ---------------------------------------------------------------------------
// Command codes
// ---------------------------------------------------------------------------

/// Read a whole table out of the device's database.
///
/// The user table is fetched with `CMD_USER_TEMP_RRQ` (9) and the template
/// table with this (7). The asymmetry is not a mistake here — it is what the
/// reference clients do, and firmware in the field has been tested against it.
pub const CMD_DB_RRQ: u16 = 7;

/// Write one fingerprint template.
pub const CMD_USERTEMP_WRQ: u16 = 10;

/// Delete one finger from one user.
pub const CMD_DELETE_USERTEMP: u16 = 19;

/// Read one specific finger, by enrolment number.
///
/// Used only as a fallback: it costs one round trip per finger, so reading a
/// school of 120 staff this way is 1,200 exchanges against a device that
/// answers in its own time.
pub const CMD_GET_USERTEMP: u16 = 88;

/// Table selector for the fingerprint templates, used with [`CMD_DB_RRQ`].
///
/// (`proto::FCT_USER` is 5; the templates are 2.)
pub const FCT_FINGERTMP: i32 = 2;

/// A normally enrolled finger.
pub const FLAG_VALID: u8 = 1;
/// A "duress" finger: scanning it opens the door and silently raises an alarm.
/// Preserved on the round trip because dropping it would quietly disarm a
/// safety feature someone deliberately configured.
pub const FLAG_DURESS: u8 = 3;

/// Templates run from roughly 600 bytes (algorithm 9) to about 2.5 KB
/// (algorithm 12). Anything past this is a misread length, not a fingerprint,
/// and following it would have us allocate from a corrupt size field.
pub const MAX_TEMPLATE_BYTES: usize = 4096;

/// `size:u16, uid:u16, finger:u8, flag:u8`
const RECORD_HEADER: usize = 6;

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// One fingerprint template as the terminal stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceTemplate {
    /// The device-internal user row id — **not** the enrolment number.
    ///
    /// This is the join that catches people out. The template table keys on the
    /// firmware's own slot number, while everything in this application keys on
    /// the enrolment number staff type on the keypad. The two are equal on a
    /// freshly enrolled device and diverge the moment anyone is deleted, so the
    /// user table has to be read alongside the templates to map between them.
    pub uid: u16,
    /// Which finger, 0–9.
    pub finger: u8,
    /// [`FLAG_VALID`] or [`FLAG_DURESS`].
    pub flag: u8,
    /// The vendor blob, exactly as it came off the wire.
    pub template: Vec<u8>,
}

impl DeviceTemplate {
    pub fn is_duress(&self) -> bool {
        self.flag == FLAG_DURESS
    }

    /// Length of this record on the wire, header included.
    pub fn record_len(&self) -> usize {
        RECORD_HEADER + self.template.len()
    }
}

// ---------------------------------------------------------------------------
// Reading: the bulk template table
// ---------------------------------------------------------------------------

/// Decode the fingerprint table returned for [`FCT_FINGERTMP`].
///
/// The layout is a 4-byte total length followed by variable-length records:
///
/// ```text
/// u32  total bytes of records that follow
/// then, repeated:
///   u16  size of this record, including these 6 header bytes
///   u16  device-internal user id
///   u8   finger index, 0-9
///   u8   flag: 1 valid, 3 duress
///   ..   size-6 bytes of template
/// ```
///
/// Unlike the attendance log, records here are self-describing, so there is no
/// need to guess a fixed width — but firmware differs on whether the 4-byte
/// total is present at all. Both forms are tried, and a buffer that does not
/// walk cleanly to its end under either is rejected rather than partially
/// decoded. Half a template is not a fingerprint; it is a user who cannot get
/// through the gate, discovered at 9am by the person it belongs to.
pub fn parse_template_table(buf: &[u8]) -> Result<Vec<DeviceTemplate>> {
    if buf.is_empty() {
        // An enrolled-nobody terminal is legitimate, not an error.
        return Ok(Vec::new());
    }

    // Preferred form: leading total, then records.
    if buf.len() >= 4 {
        let declared = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        if declared <= buf.len() - 4 {
            if let Some(v) = walk(&buf[4..4 + declared]) {
                return Ok(v);
            }
        }
    }

    // Some firmware omits the total and starts straight at the first record.
    if let Some(v) = walk(buf) {
        return Ok(v);
    }

    Err(Error::Protocol(format!(
        "the terminal sent {} bytes of fingerprint data that do not decode as a \
         template table, with or without a leading length. Nothing has been stored — \
         re-run the transfer, and if it repeats, send the Diagnose output on.",
        buf.len()
    )))
}

/// Walk a buffer of records, returning `None` unless it consumes exactly.
///
/// Requiring exact consumption is what makes the two candidate layouts
/// distinguishable: a wrong guess runs off the end or stops short almost
/// immediately, because the first `size` it reads is nonsense.
fn walk(buf: &[u8]) -> Option<Vec<DeviceTemplate>> {
    let mut out = Vec::new();
    let mut at = 0usize;

    while at < buf.len() {
        // Terminals pad the tail of the table with zeros. A run of them is the
        // end of the data, not a malformed record.
        if buf[at..].iter().all(|&b| b == 0) {
            break;
        }
        if buf.len() - at < RECORD_HEADER {
            return None;
        }

        let size = u16::from_le_bytes([buf[at], buf[at + 1]]) as usize;
        // A record must hold a header and at least one byte of template, must
        // fit in what is left, and must not claim an implausible length.
        if size <= RECORD_HEADER
            || size > buf.len() - at
            || size - RECORD_HEADER > MAX_TEMPLATE_BYTES
        {
            return None;
        }

        let finger = buf[at + 4];
        if finger > 9 {
            return None;
        }

        out.push(DeviceTemplate {
            uid: u16::from_le_bytes([buf[at + 2], buf[at + 3]]),
            finger,
            flag: buf[at + 5],
            template: buf[at + RECORD_HEADER..at + size].to_vec(),
        });
        at += size;
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

// ---------------------------------------------------------------------------
// Reading: one finger at a time
// ---------------------------------------------------------------------------

/// Build the body for [`CMD_GET_USERTEMP`]: `<h b>` — enrolment number, finger.
///
/// Note this one addresses the user by **enrolment number**, while the bulk
/// table reports the device uid. The protocol is not consistent about it.
pub fn build_get_template_request(enroll_no: i64, finger: u8) -> Result<Vec<u8>> {
    if finger > 9 {
        return Err(Error::Invalid(format!("finger index {finger} is not in 0-9")));
    }
    if !(i64::from(i16::MIN)..=i64::from(i16::MAX)).contains(&enroll_no) {
        return Err(Error::Invalid(format!(
            "enrolment number {enroll_no} does not fit the 16-bit field this \
             request uses; the terminal cannot be asked about it one finger at a time"
        )));
    }
    let mut b = Vec::with_capacity(3);
    b.extend_from_slice(&(enroll_no as i16).to_le_bytes());
    b.push(finger);
    Ok(b)
}

/// Tidy the reply to a single-finger read.
///
/// The reply is the bare template with no length header, and firmware appends
/// padding the reference clients strip by position: one trailing byte, then a
/// run of six zeros if present. That is guesswork dressed up as a protocol, and
/// it is exactly why [`parse_template_table`] is the path this application
/// prefers — there the length is stated rather than inferred.
///
/// Anything read this way should be written back and read again before it is
/// trusted, which is what the upload routine does.
pub fn trim_single_template_reply(raw: &[u8]) -> &[u8] {
    let mut end = raw.len();
    if end == 0 {
        return raw;
    }
    end -= 1; // the trailing byte the reference client always drops
    if end >= 6 && raw[end - 6..end].iter().all(|&b| b == 0) {
        end -= 6;
    }
    &raw[..end]
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Encode one template for [`CMD_USERTEMP_WRQ`].
///
/// Deliberately the same record shape the device sends back in the bulk table,
/// which is what makes a written template verifiable: upload it, read the table
/// again, and compare. The upload routine treats that read-back as mandatory
/// rather than optional, because a terminal will acknowledge a packet it has
/// stored in a form it cannot later match against a live finger.
pub fn encode_template_record(t: &DeviceTemplate) -> Result<Vec<u8>> {
    if t.finger > 9 {
        return Err(Error::Invalid(format!("finger index {} is not in 0-9", t.finger)));
    }
    if t.template.is_empty() {
        return Err(Error::Invalid(
            "refusing to write an empty fingerprint template".into(),
        ));
    }
    if t.template.len() > MAX_TEMPLATE_BYTES {
        return Err(Error::Invalid(format!(
            "template of {} bytes is larger than any real one ({MAX_TEMPLATE_BYTES} max)",
            t.template.len()
        )));
    }

    let size = t.record_len();
    if size > u16::MAX as usize {
        return Err(Error::Invalid("template record does not fit its length field".into()));
    }

    let mut b = Vec::with_capacity(size);
    b.extend_from_slice(&(size as u16).to_le_bytes());
    b.extend_from_slice(&t.uid.to_le_bytes());
    b.push(t.finger);
    b.push(t.flag);
    b.extend_from_slice(&t.template);
    debug_assert_eq!(b.len(), size);
    Ok(b)
}

/// Build the body for [`CMD_DELETE_USERTEMP`]: `<H b>` — device uid, finger.
pub fn build_delete_template_request(uid: u16, finger: u8) -> Result<Vec<u8>> {
    if finger > 9 {
        return Err(Error::Invalid(format!("finger index {finger} is not in 0-9")));
    }
    let mut b = Vec::with_capacity(3);
    b.extend_from_slice(&uid.to_le_bytes());
    b.push(finger);
    Ok(b)
}

// ---------------------------------------------------------------------------
// Base64
// ---------------------------------------------------------------------------

/// Base64 for template blobs.
///
/// Present because the two transports disagree: the ADMS push channel delivers
/// templates already base64-encoded as text, while a direct read on port 4370
/// delivers raw bytes. Both end up in the same column, so one of them has to be
/// converted, and pulling in a crate for forty lines of table lookup would mean
/// the school PC needs the network to build.
pub mod b64 {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(data: &[u8]) -> String {
        let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            out.push(ALPHABET[(n >> 18) as usize & 63] as char);
            out.push(ALPHABET[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
            out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
        }
        out
    }

    /// Decode, ignoring whitespace. Returns `None` on any character that is not
    /// base64 — a template that fails to decode must not be silently truncated
    /// into a shorter, wrong one.
    pub fn decode(s: &str) -> Option<Vec<u8>> {
        let mut acc: u32 = 0;
        let mut bits = 0u32;
        let mut out = Vec::with_capacity(s.len() / 4 * 3);

        for c in s.bytes() {
            if c.is_ascii_whitespace() {
                continue;
            }
            if c == b'=' {
                break;
            }
            let v = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => return None,
            } as u32;
            acc = (acc << 6) | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
            }
        }
        Some(out)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpl(uid: u16, finger: u8, flag: u8, body: &[u8]) -> DeviceTemplate {
        DeviceTemplate { uid, finger, flag, template: body.to_vec() }
    }

    fn table(records: &[DeviceTemplate], with_prefix: bool) -> Vec<u8> {
        let mut body = Vec::new();
        for r in records {
            body.extend(encode_template_record(r).unwrap());
        }
        if !with_prefix {
            return body;
        }
        let mut out = (body.len() as u32).to_le_bytes().to_vec();
        out.extend(body);
        out
    }

    #[test]
    fn a_template_record_round_trips() {
        let t = tmpl(41, 6, FLAG_VALID, b"not-really-a-fingerprint");
        let encoded = encode_template_record(&t).unwrap();
        assert_eq!(u16::from_le_bytes([encoded[0], encoded[1]]) as usize, t.record_len());
        let back = parse_template_table(&table(&[t.clone()], true)).unwrap();
        assert_eq!(back, vec![t]);
    }

    #[test]
    fn a_table_decodes_with_or_without_the_leading_length() {
        let rs = vec![
            tmpl(1, 0, FLAG_VALID, b"aaaa"),
            tmpl(1, 1, FLAG_VALID, b"bbbbbbbb"),
            tmpl(7, 9, FLAG_DURESS, b"cc"),
        ];
        assert_eq!(parse_template_table(&table(&rs, true)).unwrap(), rs);
        assert_eq!(parse_template_table(&table(&rs, false)).unwrap(), rs);
    }

    #[test]
    fn a_duress_finger_keeps_its_flag() {
        // Losing this quietly turns off an alarm somebody deliberately set up.
        let t = tmpl(3, 2, FLAG_DURESS, b"xyz");
        let back = parse_template_table(&table(&[t], true)).unwrap();
        assert!(back[0].is_duress(), "duress flag was not preserved");
        assert_eq!(back[0].flag, FLAG_DURESS);
    }

    #[test]
    fn trailing_zero_padding_is_not_a_record() {
        let mut buf = table(&[tmpl(1, 0, FLAG_VALID, b"aaaa")], false);
        buf.extend_from_slice(&[0u8; 16]);
        let back = parse_template_table(&buf).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].template, b"aaaa");
    }

    #[test]
    fn an_empty_table_is_not_an_error() {
        // A terminal where nobody has enrolled a finger yet.
        assert!(parse_template_table(&[]).unwrap().is_empty());
    }

    #[test]
    fn junk_is_rejected_rather_than_half_decoded() {
        // The failure that matters: a buffer that starts plausibly and then
        // stops making sense must yield nothing, not the records read so far.
        // Half a template is a member of staff who cannot get in.
        let mut buf = table(&[tmpl(1, 0, FLAG_VALID, b"aaaa")], false);
        buf.extend_from_slice(&[0x11, 0x99]); // a size of 0x9911, way past the end
        assert!(parse_template_table(&buf).is_err());

        assert!(parse_template_table(&[0xff; 32]).is_err());
    }

    #[test]
    fn a_truncated_final_record_is_rejected() {
        let mut buf = table(&[tmpl(1, 0, FLAG_VALID, b"aaaaaaaa")], false);
        buf.truncate(buf.len() - 3);
        assert!(parse_template_table(&buf).is_err());
    }

    #[test]
    fn an_impossible_finger_index_is_rejected() {
        let mut buf = table(&[tmpl(1, 0, FLAG_VALID, b"aaaa")], false);
        buf[4] = 11; // finger index out of range
        assert!(parse_template_table(&buf).is_err());
    }

    #[test]
    fn writing_refuses_input_that_would_corrupt_the_device() {
        assert!(encode_template_record(&tmpl(1, 10, FLAG_VALID, b"a")).is_err(), "finger > 9");
        assert!(encode_template_record(&tmpl(1, 0, FLAG_VALID, b"")).is_err(), "empty template");
        let huge = vec![0u8; MAX_TEMPLATE_BYTES + 1];
        assert!(encode_template_record(&tmpl(1, 0, FLAG_VALID, &huge)).is_err(), "oversized");
    }

    #[test]
    fn a_single_finger_request_is_three_little_endian_bytes() {
        let r = build_get_template_request(41, 6).unwrap();
        assert_eq!(r, vec![41, 0, 6]);
        assert!(build_get_template_request(41, 10).is_err());
        // Enrolment numbers above 32767 cannot be addressed this way at all,
        // and saying so beats sending a silently wrapped number.
        assert!(build_get_template_request(70_000, 0).is_err());
    }

    #[test]
    fn a_delete_request_is_three_little_endian_bytes() {
        assert_eq!(build_delete_template_request(0x0102, 3).unwrap(), vec![0x02, 0x01, 3]);
        assert!(build_delete_template_request(1, 10).is_err());
    }

    #[test]
    fn base64_round_trips_including_padding() {
        for case in [
            &b""[..],
            b"f",
            b"fo",
            b"foo",
            b"foob",
            b"fooba",
            b"foobar",
            &[0u8, 255, 128, 1, 2, 3][..],
        ] {
            let enc = b64::encode(case);
            assert_eq!(b64::decode(&enc).unwrap(), case, "round trip failed for {case:?}");
        }
        assert_eq!(b64::encode(b"ABCD"), "QUJDRA==");
        assert_eq!(b64::decode("QUJDRA==").unwrap(), b"ABCD");
    }

    #[test]
    fn base64_rejects_rubbish_rather_than_truncating() {
        // Returning a shorter, wrong template here would store a fingerprint
        // that can never match, and nothing would report it.
        assert!(b64::decode("QUJD*RA==").is_none());
        assert!(b64::decode("not base64 !!").is_none());
        // Whitespace from a wrapped HTTP body is fine, though.
        assert_eq!(b64::decode("QUJD\r\n  RA==").unwrap(), b"ABCD");
    }

    #[test]
    fn single_reply_trimming_matches_the_reference_client() {
        // one trailing byte, then six zeros
        let mut raw = b"template".to_vec();
        raw.extend_from_slice(&[0u8; 6]);
        raw.push(0x00);
        assert_eq!(trim_single_template_reply(&raw), b"template");

        // one trailing byte only
        let raw2 = b"template\x00".to_vec();
        assert_eq!(trim_single_template_reply(&raw2), b"template");

        assert_eq!(trim_single_template_reply(&[]), &[] as &[u8]);
    }
}
