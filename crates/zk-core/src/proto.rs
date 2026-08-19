//! ZKTeco binary protocol codec (the "standalone"/pull path, TCP port 4370).
//!
//! Everything in this module is a pure function over bytes, which is the whole
//! point: the school's K40 Pro is not reachable from a build machine, so the
//! wire format is verified with unit tests over captured/derived fixtures and
//! only the socket plumbing (`crate::pull`) is left untested until it meets
//! real hardware.
//!
//! Reference behaviour follows the widely used `pyzk` implementation, which is
//! the de-facto description of this undocumented protocol.

use crate::Error;

// ---------------------------------------------------------------------------
// Command codes
// ---------------------------------------------------------------------------

pub const CMD_CONNECT: u16 = 1000;
pub const CMD_EXIT: u16 = 1001;
pub const CMD_ENABLE_DEVICE: u16 = 1002;
pub const CMD_DISABLE_DEVICE: u16 = 1003;
pub const CMD_RESTART: u16 = 1004;
pub const CMD_REFRESH_DATA: u16 = 1013;
pub const CMD_AUTH: u16 = 1102;

pub const CMD_PREPARE_DATA: u16 = 1500;
pub const CMD_DATA: u16 = 1501;
pub const CMD_FREE_DATA: u16 = 1502;
/// Ask the device to prepare a large table in its buffer.
pub const CMD_DATA_WRRQ: u16 = 1503;
/// Read a chunk out of that prepared buffer.
pub const CMD_READ_BUFFER: u16 = 1504;

/// Table selector used with `CMD_DATA_WRRQ`. 5 = the user table.
pub const FCT_USER: i32 = 5;
/// 8 = the attendance log.
pub const FCT_ATTLOG: i32 = 8;

pub const CMD_ACK_OK: u16 = 2000;
pub const CMD_ACK_ERROR: u16 = 2001;
pub const CMD_ACK_DATA: u16 = 2002;
pub const CMD_ACK_UNAUTH: u16 = 2005;

pub const CMD_USER_TEMP_RRQ: u16 = 9;
pub const CMD_SET_USER: u16 = 8;
pub const CMD_DELETE_USER: u16 = 18;
pub const CMD_ATTLOG_RRQ: u16 = 13;
pub const CMD_CLEAR_ATTLOG: u16 = 15;
pub const CMD_GET_FREE_SIZES: u16 = 50;
pub const CMD_DEVICE: u16 = 11;
pub const CMD_GET_TIME: u16 = 201;
pub const CMD_SET_TIME: u16 = 202;

/// Magic prefix that wraps every packet when the transport is TCP.
pub const TCP_MAGIC: [u8; 4] = [0x50, 0x50, 0x82, 0x7d];

const USHRT_MAX: u32 = 65535;

// ---------------------------------------------------------------------------
// Checksum
// ---------------------------------------------------------------------------

/// ZK's 16-bit ones-complement-ish checksum.
///
/// Sums the buffer as little-endian `u16` words (with a trailing odd byte added
/// as-is), folding at `USHRT_MAX` rather than the more usual `0x10000`, then
/// inverts. The odd fold is not a mistake — it is what the devices do.
pub fn checksum(buf: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < buf.len() {
        sum += u16::from_le_bytes([buf[i], buf[i + 1]]) as u32;
        if sum > USHRT_MAX {
            sum -= USHRT_MAX;
        }
        i += 2;
    }
    if i < buf.len() {
        sum += buf[buf.len() - 1] as u32;
    }
    while sum > USHRT_MAX {
        sum -= USHRT_MAX;
    }
    // Invert, then fold back into range the way the devices do.
    let mut signed = !(sum as i32);
    while signed < 0 {
        signed += USHRT_MAX as i32;
    }
    (signed as u32 & 0xffff) as u16
}

// ---------------------------------------------------------------------------
// Packet header
// ---------------------------------------------------------------------------

/// The 8-byte command header that precedes every payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub command: u16,
    pub checksum: u16,
    pub session_id: u16,
    pub reply_id: u16,
}

impl Header {
    pub fn parse(buf: &[u8]) -> Result<Self, Error> {
        if buf.len() < 8 {
            return Err(Error::Protocol("packet shorter than 8-byte header".into()));
        }
        Ok(Header {
            command: u16::from_le_bytes([buf[0], buf[1]]),
            checksum: u16::from_le_bytes([buf[2], buf[3]]),
            session_id: u16::from_le_bytes([buf[4], buf[5]]),
            reply_id: u16::from_le_bytes([buf[6], buf[7]]),
        })
    }
}

/// Build a complete command payload (header + data) with a valid checksum.
///
/// `reply_id` is incremented before framing, matching device expectations.
pub fn build_payload(command: u16, session_id: u16, reply_id: u16, data: &[u8]) -> Vec<u8> {
    let next_reply = reply_id.wrapping_add(1);

    let mut buf = Vec::with_capacity(8 + data.len());
    buf.extend_from_slice(&command.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes()); // checksum placeholder
    buf.extend_from_slice(&session_id.to_le_bytes());
    buf.extend_from_slice(&next_reply.to_le_bytes());
    buf.extend_from_slice(data);

    let ck = checksum(&buf);
    buf[2..4].copy_from_slice(&ck.to_le_bytes());
    buf
}

/// Wrap a payload in the TCP magic + length prefix.
pub fn frame_tcp(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&TCP_MAGIC);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Strip the TCP magic + length prefix. Returns (declared_len, payload_slice).
pub fn unframe_tcp(buf: &[u8]) -> Result<(usize, &[u8]), Error> {
    if buf.len() < 8 {
        return Err(Error::Protocol("TCP frame shorter than 8 bytes".into()));
    }
    if buf[0..4] != TCP_MAGIC {
        return Err(Error::Protocol(format!(
            "bad TCP magic {:02x?}, expected {:02x?}",
            &buf[0..4],
            TCP_MAGIC
        )));
    }
    let declared = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
    Ok((declared, &buf[8..]))
}

// ---------------------------------------------------------------------------
// Comm key
// ---------------------------------------------------------------------------

/// Derive the 4-byte authentication token from the device comm key + session id.
///
/// Devices with a non-zero "COMM Key" reject commands until this is sent via
/// `CMD_AUTH`. Your K40 Pro currently has comm key 0, but schools often set one
/// later, so this is implemented rather than assumed away.
pub fn make_comm_key(key: u32, session_id: u16, ticks: u8) -> [u8; 4] {
    // Reverse the low 32 bits of the key.
    let mut k: u32 = 0;
    for i in 0..32 {
        if key & (1 << i) != 0 {
            k = (k << 1) | 1;
        } else {
            k <<= 1;
        }
    }
    k = k.wrapping_add(session_id as u32);

    let b = k.to_le_bytes();
    let x = [
        b[0] ^ b'Z',
        b[1] ^ b'K',
        b[2] ^ b'S',
        b[3] ^ b'O',
    ];

    // Swap the two 16-bit halves.
    let hi = u16::from_le_bytes([x[0], x[1]]);
    let lo = u16::from_le_bytes([x[2], x[3]]);
    let mut y = [0u8; 4];
    y[0..2].copy_from_slice(&lo.to_le_bytes());
    y[2..4].copy_from_slice(&hi.to_le_bytes());

    [y[0] ^ ticks, y[1] ^ ticks, y[2], y[3] ^ ticks]
}

// ---------------------------------------------------------------------------
// Time codec
// ---------------------------------------------------------------------------

/// Decode ZK's packed 4-byte timestamp into (y, m, d, h, min, s).
///
/// The encoding treats every month as 31 days and every year as 12 months, so
/// it is not a real epoch — it is a positional counter.
pub fn decode_time(raw: u32) -> (i32, u32, u32, u32, u32, u32) {
    let mut t = raw;
    let second = t % 60;
    t /= 60;
    let minute = t % 60;
    t /= 60;
    let hour = t % 24;
    t /= 24;
    let day = t % 31 + 1;
    t /= 31;
    let month = t % 12 + 1;
    t /= 12;
    let year = t as i32 + 2000;
    (year, month, day, hour, minute, second)
}

/// Encode (y, m, d, h, min, s) back into ZK's packed 4-byte form.
pub fn encode_time(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> u32 {
    let y = (year % 100) as u32;
    let days = y * 12 * 31 + (month - 1) * 31 + (day - 1);
    days * 24 * 60 * 60 + (hour * 60 + minute) * 60 + second
}

// ---------------------------------------------------------------------------
// Record parsing
// ---------------------------------------------------------------------------

/// One raw punch as stored on the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttLog {
    /// Device-internal row id (not stable across clears).
    pub uid: u32,
    /// The enrolment number as a string — this is what we join on.
    pub user_id: String,
    /// Verification method: 1 = fingerprint, 4 = card, 15 = face, etc.
    pub verify: u8,
    /// 0 = check-in, 1 = check-out, 2/3 = break, 4/5 = overtime.
    pub punch: u8,
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl AttLog {
    /// `YYYY-MM-DD HH:MM:SS`, the form stored in SQLite.
    pub fn timestamp(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    pub fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    fn plausible(&self) -> bool {
        self.year >= 2000
            && self.year <= 2100
            && (1..=12).contains(&self.month)
            && (1..=31).contains(&self.day)
            && self.hour < 24
            && self.minute < 60
            && self.second < 60
            && !self.user_id.is_empty()
    }
}

fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

/// Parse a 40-byte attendance record (the modern layout used by ZLM60 devices
/// such as the K40 Pro).
fn parse_attlog_40(r: &[u8]) -> AttLog {
    let uid = u16::from_le_bytes([r[0], r[1]]) as u32;
    let user_id = cstr(&r[2..26]);
    let verify = r[26];
    let ts = u32::from_le_bytes([r[27], r[28], r[29], r[30]]);
    let punch = r[31];
    let (year, month, day, hour, minute, second) = decode_time(ts);
    AttLog { uid, user_id, verify, punch, year, month, day, hour, minute, second }
}

/// Parse a 16-byte attendance record (compact layout, numeric user ids only).
fn parse_attlog_16(r: &[u8]) -> AttLog {
    let user_id = u32::from_le_bytes([r[0], r[1], r[2], r[3]]);
    let ts = u32::from_le_bytes([r[4], r[5], r[6], r[7]]);
    let verify = r[8];
    let punch = r[9];
    let (year, month, day, hour, minute, second) = decode_time(ts);
    AttLog {
        uid: user_id,
        user_id: user_id.to_string(),
        verify,
        punch,
        year,
        month,
        day,
        hour,
        minute,
        second,
    }
}

/// Parse an 8-byte attendance record (oldest layout).
fn parse_attlog_8(r: &[u8]) -> AttLog {
    let uid = u16::from_le_bytes([r[0], r[1]]) as u32;
    let verify = r[2];
    let ts = u32::from_le_bytes([r[3], r[4], r[5], r[6]]);
    let punch = r[7];
    let (year, month, day, hour, minute, second) = decode_time(ts);
    AttLog {
        uid,
        user_id: uid.to_string(),
        verify,
        punch,
        year,
        month,
        day,
        hour,
        minute,
        second,
    }
}

/// Decide which record width a buffer uses and decode all of it.
///
/// Terminals do not announce their record size, so this tries each candidate
/// width that divides the buffer evenly and keeps the one that yields sane
/// dates. Guessing wrong silently would import a year of garbage, so the
/// plausibility check is the actual safety mechanism here, not a nicety.
pub fn parse_attlog_buffer(buf: &[u8]) -> Result<Vec<AttLog>, Error> {
    if buf.is_empty() {
        return Ok(Vec::new());
    }

    for &(size, f) in &[
        (40usize, parse_attlog_40 as fn(&[u8]) -> AttLog),
        (16, parse_attlog_16),
        (8, parse_attlog_8),
    ] {
        if buf.len() % size != 0 {
            continue;
        }
        let out: Vec<AttLog> = buf.chunks_exact(size).map(f).collect();
        let total = out.len();
        let good = out.iter().filter(|a| a.plausible()).count();
        // Require a strong majority to be sane before trusting this width.
        if total > 0 && good * 10 >= total * 9 {
            return Ok(out.into_iter().filter(|a| a.plausible()).collect());
        }
    }

    Err(Error::Protocol(format!(
        "could not determine attendance record width for {} bytes",
        buf.len()
    )))
}

/// A user as stored on the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceUser {
    pub uid: u16,
    pub user_id: String,
    pub name: String,
    /// 0 = user, 2 = enroller, 6 = manager, 14 = super admin.
    pub privilege: u8,
    pub password: String,
    pub card: u32,
    pub group_id: String,
}

fn parse_user_72(r: &[u8]) -> DeviceUser {
    DeviceUser {
        uid: u16::from_le_bytes([r[0], r[1]]),
        privilege: r[2],
        password: cstr(&r[3..11]),
        name: cstr(&r[11..35]),
        card: u32::from_le_bytes([r[35], r[36], r[37], r[38]]),
        // r[39] is padding
        group_id: cstr(&r[40..47]),
        // r[47] is padding
        user_id: cstr(&r[48..72]),
    }
}

fn parse_user_28(r: &[u8]) -> DeviceUser {
    let user_id = u32::from_le_bytes([r[24], r[25], r[26], r[27]]);
    DeviceUser {
        uid: u16::from_le_bytes([r[0], r[1]]),
        privilege: r[2],
        password: cstr(&r[3..8]),
        name: cstr(&r[8..16]),
        card: u32::from_le_bytes([r[16], r[17], r[18], r[19]]),
        // r[20] padding
        group_id: r[21].to_string(),
        user_id: user_id.to_string(),
    }
}

/// Decode a user table buffer, auto-detecting the 72- or 28-byte layout.
pub fn parse_user_buffer(buf: &[u8]) -> Result<Vec<DeviceUser>, Error> {
    if buf.is_empty() {
        return Ok(Vec::new());
    }
    for &(size, f) in &[
        (72usize, parse_user_72 as fn(&[u8]) -> DeviceUser),
        (28, parse_user_28),
    ] {
        if buf.len() % size != 0 {
            continue;
        }
        let out: Vec<DeviceUser> = buf.chunks_exact(size).map(f).collect();
        let good = out.iter().filter(|u| !u.user_id.is_empty()).count();
        if !out.is_empty() && good * 10 >= out.len() * 9 {
            return Ok(out.into_iter().filter(|u| !u.user_id.is_empty()).collect());
        }
    }
    Err(Error::Protocol(format!(
        "could not determine user record width for {} bytes",
        buf.len()
    )))
}

/// Build the request body for `CMD_DATA_WRRQ`.
///
/// Layout is `<b h i i>`: a leading 1, the table command, the table selector,
/// and an extra word — 11 bytes, no padding. Newer firmware (yours reports
/// Push Service 2.0.33S) only serves the user table this way; the older direct
/// `CMD_USER_TEMP_RRQ` request returns nothing at all.
pub fn build_wrrq_request(command: u16, fct: i32, ext: i32) -> Vec<u8> {
    let mut b = Vec::with_capacity(11);
    b.push(1u8);
    b.extend_from_slice(&(command as i16).to_le_bytes());
    b.extend_from_slice(&fct.to_le_bytes());
    b.extend_from_slice(&ext.to_le_bytes());
    debug_assert_eq!(b.len(), 11);
    b
}

/// Build the request body for `CMD_READ_BUFFER`: `<i i>` start and length.
pub fn build_read_chunk_request(start: u32, size: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(8);
    b.extend_from_slice(&start.to_le_bytes());
    b.extend_from_slice(&size.to_le_bytes());
    b
}

/// Encode a user for `CMD_SET_USER` in the 72-byte layout.
pub fn encode_user_72(u: &DeviceUser) -> Vec<u8> {
    fn fixed(s: &str, n: usize) -> Vec<u8> {
        let mut v = s.as_bytes().to_vec();
        v.truncate(n);
        v.resize(n, 0);
        v
    }
    let mut b = Vec::with_capacity(72);
    b.extend_from_slice(&u.uid.to_le_bytes());
    b.push(u.privilege);
    b.extend_from_slice(&fixed(&u.password, 8));
    b.extend_from_slice(&fixed(&u.name, 24));
    b.extend_from_slice(&u.card.to_le_bytes());
    b.push(0);
    b.extend_from_slice(&fixed(&u.group_id, 7));
    b.push(0);
    b.extend_from_slice(&fixed(&u.user_id, 24));
    debug_assert_eq!(b.len(), 72);
    b
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_is_stable_and_folds() {
        // A packet whose checksum field is zeroed must produce a value that,
        // once written back, makes the whole buffer verify consistently.
        let payload = build_payload(CMD_CONNECT, 0, 65534, &[]);
        let h = Header::parse(&payload).unwrap();
        assert_eq!(h.command, CMD_CONNECT);
        assert_eq!(h.session_id, 0);
        assert_eq!(h.reply_id, 65535, "reply id must be incremented before send");

        // Recomputing over the buffer with the checksum zeroed reproduces it.
        let mut z = payload.clone();
        z[2..4].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(checksum(&z), h.checksum);
    }

    #[test]
    fn checksum_handles_odd_length() {
        assert_eq!(checksum(&[]), checksum(&[]));
        let a = checksum(&[0x01, 0x02, 0x03]);
        let b = checksum(&[0x01, 0x02, 0x03]);
        assert_eq!(a, b);
        // Differing trailing byte must change the result.
        assert_ne!(checksum(&[0x01, 0x02, 0x03]), checksum(&[0x01, 0x02, 0x04]));
    }

    #[test]
    fn reply_id_wraps_without_panicking() {
        let p = build_payload(CMD_EXIT, 7, u16::MAX, &[]);
        assert_eq!(Header::parse(&p).unwrap().reply_id, 0);
    }

    #[test]
    fn tcp_frame_roundtrip() {
        let payload = build_payload(CMD_ATTLOG_RRQ, 42, 3, b"hello");
        let framed = frame_tcp(&payload);
        assert_eq!(&framed[0..4], &TCP_MAGIC);
        let (len, body) = unframe_tcp(&framed).unwrap();
        assert_eq!(len, payload.len());
        assert_eq!(body, &payload[..]);
    }

    #[test]
    fn tcp_frame_rejects_bad_magic() {
        let bad = [0xde, 0xad, 0xbe, 0xef, 0, 0, 0, 0];
        assert!(unframe_tcp(&bad).is_err());
    }

    #[test]
    fn time_roundtrips() {
        for &(y, m, d, h, mi, s) in &[
            (2026, 8, 18, 9, 1, 23),
            (2026, 1, 1, 0, 0, 0),
            (2025, 12, 31, 23, 59, 59),
            (2082, 4, 15, 16, 30, 0),
        ] {
            let enc = encode_time(y, m, d, h, mi, s);
            assert_eq!(decode_time(enc), (y, m, d, h, mi, s), "roundtrip {y}-{m}-{d}");
        }
    }

    #[test]
    fn decode_time_matches_known_encoding() {
        // Independently computed: 2026-08-18 09:01:23
        // days = 26*12*31 + 7*31 + 17 = 9672 + 217 + 17 = 9906
        // secs = 9906*86400 + (9*60+1)*60 + 23 = 855878400 + 32460 + 23
        let raw = 9906u32 * 86400 + (9 * 60 + 1) * 60 + 23;
        assert_eq!(decode_time(raw), (2026, 8, 18, 9, 1, 23));
    }

    fn attlog_40_fixture(user: &str, ts: u32, punch: u8) -> Vec<u8> {
        let mut r = vec![0u8; 40];
        r[0..2].copy_from_slice(&7u16.to_le_bytes());
        let ub = user.as_bytes();
        r[2..2 + ub.len()].copy_from_slice(ub);
        r[26] = 1; // fingerprint
        r[27..31].copy_from_slice(&ts.to_le_bytes());
        r[31] = punch;
        r
    }

    #[test]
    fn parses_40_byte_attlog() {
        let ts = encode_time(2026, 8, 18, 9, 1, 23);
        let mut buf = attlog_40_fixture("41", ts, 0);
        buf.extend(attlog_40_fixture("12", encode_time(2026, 8, 18, 16, 5, 0), 1));

        let logs = parse_attlog_buffer(&buf).unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].user_id, "41");
        assert_eq!(logs[0].timestamp(), "2026-08-18 09:01:23");
        assert_eq!(logs[0].punch, 0);
        assert_eq!(logs[0].verify, 1);
        assert_eq!(logs[1].user_id, "12");
        assert_eq!(logs[1].timestamp(), "2026-08-18 16:05:00");
        assert_eq!(logs[1].punch, 1);
    }

    #[test]
    fn parses_16_byte_attlog() {
        let mut r = vec![0u8; 16];
        r[0..4].copy_from_slice(&41u32.to_le_bytes());
        r[4..8].copy_from_slice(&encode_time(2026, 8, 18, 8, 45, 10).to_le_bytes());
        r[8] = 1;
        r[9] = 0;
        let logs = parse_attlog_buffer(&r).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].user_id, "41");
        assert_eq!(logs[0].timestamp(), "2026-08-18 08:45:10");
    }

    #[test]
    fn empty_attlog_is_not_an_error() {
        assert!(parse_attlog_buffer(&[]).unwrap().is_empty());
    }

    #[test]
    fn rejects_undecodable_attlog_rather_than_inventing_dates() {
        // 40 bytes of 0xFF divides evenly by 40 but decodes to year 6000+.
        let junk = vec![0xffu8; 40];
        assert!(parse_attlog_buffer(&junk).is_err());
    }

    #[test]
    fn attlog_width_detection_prefers_correct_layout() {
        // 80 bytes divides by both 40 and 16(x5) and 8 — only the 40-byte
        // reading yields sane dates, so that must win.
        let ts = encode_time(2026, 8, 18, 9, 0, 0);
        let mut buf = attlog_40_fixture("1", ts, 0);
        buf.extend(attlog_40_fixture("2", ts, 0));
        assert_eq!(buf.len(), 80);
        let logs = parse_attlog_buffer(&buf).unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].user_id, "1");
    }

    #[test]
    fn user_72_roundtrips() {
        let u = DeviceUser {
            uid: 41,
            user_id: "41".into(),
            name: "Sarita Maharjan".into(),
            privilege: 0,
            password: "".into(),
            card: 1234567,
            group_id: "1".into(),
        };
        let bytes = encode_user_72(&u);
        assert_eq!(bytes.len(), 72);
        let back = parse_user_buffer(&bytes).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0], u);
    }

    #[test]
    fn user_name_is_truncated_not_corrupted() {
        let u = DeviceUser {
            uid: 1,
            user_id: "1".into(),
            name: "A".repeat(40), // longer than the 24-byte field
            privilege: 0,
            password: String::new(),
            card: 0,
            group_id: String::new(),
        };
        let bytes = encode_user_72(&u);
        assert_eq!(bytes.len(), 72, "over-long name must not overflow the record");
        let back = parse_user_buffer(&bytes).unwrap();
        assert_eq!(back[0].name.len(), 24);
    }

    #[test]
    fn comm_key_is_deterministic_and_session_dependent() {
        let a = make_comm_key(0, 1234, 50);
        let b = make_comm_key(0, 1234, 50);
        assert_eq!(a, b);
        assert_ne!(make_comm_key(0, 1234, 50), make_comm_key(0, 1235, 50));
        assert_ne!(make_comm_key(0, 1234, 50), make_comm_key(1, 1234, 50));
    }

    #[test]
    fn wrrq_request_has_the_exact_layout_the_device_expects() {
        // <b h i i> with no padding: 1 + 2 + 4 + 4 = 11 bytes.
        let r = build_wrrq_request(CMD_USER_TEMP_RRQ, FCT_USER, 0);
        assert_eq!(r.len(), 11, "a padded struct here makes the device return nothing");
        assert_eq!(r[0], 1);
        assert_eq!(i16::from_le_bytes([r[1], r[2]]), CMD_USER_TEMP_RRQ as i16);
        assert_eq!(i32::from_le_bytes([r[3], r[4], r[5], r[6]]), FCT_USER);
        assert_eq!(i32::from_le_bytes([r[7], r[8], r[9], r[10]]), 0);
    }

    #[test]
    fn read_chunk_request_is_two_little_endian_words() {
        let r = build_read_chunk_request(0, 0xFFC0);
        assert_eq!(r.len(), 8);
        assert_eq!(u32::from_le_bytes([r[0], r[1], r[2], r[3]]), 0);
        assert_eq!(u32::from_le_bytes([r[4], r[5], r[6], r[7]]), 0xFFC0);

        let r2 = build_read_chunk_request(0xFFC0, 512);
        assert_eq!(u32::from_le_bytes([r2[0], r2[1], r2[2], r2[3]]), 0xFFC0);
        assert_eq!(u32::from_le_bytes([r2[4], r2[5], r2[6], r2[7]]), 512);
    }

    #[test]
    fn header_parse_rejects_short_buffer() {
        assert!(Header::parse(&[0u8; 4]).is_err());
    }
}
