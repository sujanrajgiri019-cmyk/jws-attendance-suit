//! ADMS / "push" protocol.
//!
//! In this mode the terminal is the client: it dials out to us over plain HTTP
//! and posts each punch as it happens. That is why it is the default for JWS —
//! there is no polling delay, and the PC does not need to reach the device
//! through any firewall, only the other way round.
//!
//! The endpoints a ZKTeco push device speaks are:
//!
//! * `GET  /iclock/cdata?SN=..&options=all&pushver=..`  handshake, we reply with config
//! * `POST /iclock/cdata?SN=..&table=ATTLOG`            punches, tab separated
//! * `POST /iclock/cdata?SN=..&table=OPERLOG`           admin operations on the device
//! * `GET  /iclock/getrequest?SN=..`                    device asks "anything for me?"
//! * `POST /iclock/devicecmd?SN=..`                     device reports command results
//!
//! Everything here is pure string handling so it can be tested without a socket.

use crate::proto::AttLog;

/// Configuration handed to a terminal during handshake.
#[derive(Debug, Clone)]
pub struct PushConfig {
    /// How often (seconds) the device re-checks for pending commands.
    pub delay_secs: u32,
    /// Seconds to wait after an error before retrying.
    pub error_delay_secs: u32,
    /// Device timezone as an offset in hours. Nepal is +5.75.
    pub timezone_hours: f32,
    /// 1 = send punches the moment they happen.
    pub realtime: bool,
}

impl Default for PushConfig {
    fn default() -> Self {
        // Nepal Standard Time is UTC+5:45, i.e. 5.75 hours. Getting this wrong
        // shifts every imported punch, so it is set explicitly rather than
        // inherited from the host clock.
        PushConfig { delay_secs: 10, error_delay_secs: 30, timezone_hours: 5.75, realtime: true }
    }
}

/// Build the handshake reply for `GET /iclock/cdata?...&options=all`.
///
/// `stamp` is the high-water mark of records we have already accepted; the
/// device resends anything newer. Passing 0 asks for everything it holds.
pub fn handshake_response(serial: &str, stamp: u64, cfg: &PushConfig) -> String {
    // ServerVer and PushProtVer are what tell the terminal it is talking to a
    // real ADMS server rather than something that merely accepts posts. Without
    // them a K40 Pro will happily send its attendance log and then never poll
    // /iclock/getrequest again — so punches arrive, every command queued for the
    // device is ignored forever, and nothing in the exchange looks broken.
    //
    // TransFlag goes out as the numeric bitmask. The named form is accepted by
    // newer firmware only; the digits are understood by all of it, and each one
    // switches on a class of record the device may send us.
    format!(
        "GET OPTION FROM: {sn}\r\n\
         Stamp={stamp}\r\n\
         OpStamp={stamp}\r\n\
         ErrorDelay={err}\r\n\
         Delay={delay}\r\n\
         TransTimes=00:00;14:00\r\n\
         TransInterval=1\r\n\
         TransFlag=1111111111\r\n\
         TimeZone={tz}\r\n\
         Realtime={rt}\r\n\
         Encrypt=0\r\n\
         ServerVer=2.4.1 {stamp}\r\n\
         PushProtVer=2.4.1\r\n\
         PushOptionsFlag=1\r\n\
         ATTLOGStamp=None\r\n\
         OPERLOGStamp=9999\r\n\
         ATTPHOTOStamp=None\r\n",
        sn = serial,
        stamp = stamp,
        err = cfg.error_delay_secs,
        delay = cfg.delay_secs,
        // ADMS expects a whole number of hours here. Nepal is UTC+5:45, and
        // sending "5.75" makes some firmware fail to parse the whole options
        // block — after which it posts attendance but never enables its command
        // channel, which is exactly the half-working state that is hardest to
        // diagnose. The 45 minutes cannot be expressed in this field at all;
        // punch times come from the device's own clock, so nothing is lost by
        // rounding it here.
        tz = cfg.timezone_hours.round() as i32,
        rt = if cfg.realtime { 1 } else { 0 },
    )
}

/// One fingerprint template as the terminal reports it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PushFinger {
    pub pin: String,
    /// Which finger, 0-9.
    pub index: u8,
    pub size: i64,
    pub valid: bool,
    /// The device's own template blob, base64 as it arrived.
    pub template: String,
}

/// Parse a `table=FINGERTMP` body.
///
/// ```text
/// FP PIN=41\tFID=0\tSize=1024\tValid=1\tTMP=TUp3AAAA...
/// ```
///
/// The template is kept exactly as the terminal sent it. It is a vendor blob,
/// not something to interpret — re-encoding it would risk making it unusable
/// when it is uploaded back to a replacement device, which is the entire point
/// of storing it.
pub fn parse_fingerprint_body(body: &str) -> Vec<PushFinger> {
    let mut out = Vec::new();

    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let rest = line.strip_prefix("FP").unwrap_or(line).trim_start();

        let mut f = PushFinger { valid: true, ..Default::default() };
        for field in rest.split('\t') {
            let Some((k, v)) = field.split_once('=') else { continue };
            match k.trim() {
                "PIN" | "PIN2" => f.pin = v.trim().to_string(),
                "FID" => f.index = v.trim().parse().unwrap_or(0),
                "Size" => f.size = v.trim().parse().unwrap_or(0),
                "Valid" => f.valid = v.trim() != "0",
                // The template itself may contain '=' padding, so take the
                // whole remainder rather than splitting again.
                "TMP" => f.template = v.trim().to_string(),
                _ => {}
            }
        }

        if f.pin.trim().parse::<i64>().is_err() || f.template.is_empty() || f.index > 9 {
            continue;
        }
        out.push(f);
    }
    out
}

/// One user record as the terminal reports it over ADMS.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PushUser {
    pub pin: String,
    pub name: String,
    pub privilege: u8,
    pub password: String,
    pub card: String,
    pub group: u8,
}

/// Parse a `table=OPERLOG`-style USERINFO body.
///
/// Each line looks like:
///
/// ```text
/// USER PIN=41\tName=Sarita Maharjan\tPri=0\tPasswd=\tCard=0\tGrp=1\tTZ=0000000000000000
/// ```
///
/// Fields are `key=value` pairs in no guaranteed order, and firmware versions
/// differ in which ones they include — so this reads them by name rather than
/// by position, and skips a line only when it has no PIN to join on.
pub fn parse_userinfo_body(body: &str) -> Vec<PushUser> {
    let mut out = Vec::new();

    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Drop the leading record type ("USER ", "OPLOG ", ...) if present.
        let rest = line.strip_prefix("USER").unwrap_or(line).trim_start();
        if rest.is_empty() {
            continue;
        }

        let mut u = PushUser { privilege: 0, group: 1, ..Default::default() };
        let mut saw_pin = false;

        for field in rest.split('\t') {
            let Some((k, v)) = field.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "PIN" | "PIN2" => {
                    // Some firmware sends both PIN (index) and PIN2 (the number
                    // typed on the keypad). PIN2 is the one everything joins on,
                    // so it wins when both are present.
                    if k.trim() == "PIN2" || !saw_pin {
                        u.pin = v.to_string();
                        saw_pin = true;
                    }
                }
                "Name" => u.name = v.to_string(),
                "Pri" => u.privilege = v.parse().unwrap_or(0),
                "Passwd" => u.password = v.to_string(),
                "Card" => u.card = v.to_string(),
                "Grp" => u.group = v.parse().unwrap_or(1),
                _ => {}
            }
        }

        // A record with no enrolment number cannot be joined to anything.
        if u.pin.trim().is_empty() || u.pin.trim().parse::<i64>().is_err() {
            continue;
        }
        out.push(u);
    }
    out
}

/// Parse the tab-separated ATTLOG body a device posts to `/iclock/cdata`.
///
/// Layout is `user_id \t timestamp \t status \t verify \t workcode ...`, one
/// record per line. Devices vary in how many trailing columns they send and
/// some pad with empty fields, so only the first four are relied upon.
///
/// Malformed lines are skipped rather than failing the whole batch: losing one
/// punch to a truncated line is far better than rejecting a POST and having the
/// terminal retry the entire buffer forever.
pub fn parse_attlog_body(body: &str) -> Vec<AttLog> {
    let mut out = Vec::new();

    for line in body.lines() {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 2 {
            continue;
        }

        let user_id = f[0].trim();
        if user_id.is_empty() {
            continue;
        }

        let Some(dt) = parse_datetime(f[1].trim()) else {
            continue;
        };

        // Column 2 is the punch state (0 in / 1 out), column 3 the verify mode.
        let punch = f.get(2).and_then(|s| s.trim().parse::<u8>().ok()).unwrap_or(0);
        let verify = f.get(3).and_then(|s| s.trim().parse::<u8>().ok()).unwrap_or(1);

        out.push(AttLog {
            uid: 0,
            user_id: user_id.to_string(),
            verify,
            punch,
            year: dt.0,
            month: dt.1,
            day: dt.2,
            hour: dt.3,
            minute: dt.4,
            second: dt.5,
        });
    }

    out
}

/// Accept `YYYY-MM-DD HH:MM:SS`, and tolerate a `T` separator or a missing
/// seconds field, both of which appear in the wild.
fn parse_datetime(s: &str) -> Option<(i32, u32, u32, u32, u32, u32)> {
    let s = s.replace('T', " ");
    let (d, t) = s.split_once(' ')?;

    let mut dp = d.split('-');
    let year: i32 = dp.next()?.parse().ok()?;
    let month: u32 = dp.next()?.parse().ok()?;
    let day: u32 = dp.next()?.parse().ok()?;

    let mut tp = t.split(':');
    let hour: u32 = tp.next()?.parse().ok()?;
    let minute: u32 = tp.next()?.parse().ok()?;
    let second: u32 = tp.next().and_then(|v| v.parse().ok()).unwrap_or(0);

    if !(2000..=2100).contains(&year)
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    Some((year, month, day, hour, minute, second))
}

/// Extract a query parameter from a raw request target such as
/// `/iclock/cdata?SN=GED7253800740&table=ATTLOG`.
pub fn query_param(target: &str, key: &str) -> Option<String> {
    let q = target.split_once('?')?.1;
    for pair in q.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k.eq_ignore_ascii_case(key) {
            return Some(url_decode(v));
        }
    }
    None
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A command queued for a terminal to collect on its next `getrequest` poll.
#[derive(Debug, Clone)]
pub enum DeviceCommand {
    /// Create or update a user on the device.
    SetUser { pin: String, name: String, privilege: u8, password: String, card: String },
    /// Remove a user and their templates.
    DeleteUser { pin: String },
    /// Ask the device to re-send all its data.
    CheckData,
    /// Reboot.
    Reboot,
    /// Clear the stored attendance log.
    ClearLog,
    /// Ask the device to re-send its attendance log for a date range.
    ///
    /// This is the push-mode equivalent of dialling the terminal on port 4370
    /// and pulling the records: instead of this PC opening a connection to the
    /// device, the device is told what to send the next time it checks in. On a
    /// terminal configured for ADMS that is usually the *only* way in, because
    /// enabling the cloud server commonly closes the direct TCP port.
    QueryAttlog { from: String, to: String },
    /// Ask the device to re-send its user table.
    QueryUserInfo,
    /// Ask the device to re-send every fingerprint template.
    QueryFingerprints,
}

impl DeviceCommand {
    /// Render as the single line the device expects, given a command id.
    pub fn encode(&self, id: u64) -> String {
        match self {
            DeviceCommand::SetUser { pin, name, privilege, password, card } => format!(
                "C:{id}:DATA UPDATE USERINFO PIN={pin}\tName={name}\tPri={privilege}\tPasswd={password}\tCard={card}\tGrp=1\tTZ=0000000000000000"
            ),
            DeviceCommand::DeleteUser { pin } => {
                format!("C:{id}:DATA DELETE USERINFO PIN={pin}")
            }
            DeviceCommand::CheckData => format!("C:{id}:CHECK"),
            DeviceCommand::Reboot => format!("C:{id}:REBOOT"),
            DeviceCommand::ClearLog => format!("C:{id}:CLEAR LOG"),
            // Dates go as 'YYYY-MM-DD HH:MM:SS'; the terminal ignores a range
            // it cannot satisfy and sends what it has.
            DeviceCommand::QueryAttlog { from, to } => format!(
                "C:{id}:DATA QUERY ATTLOG StartTime={from} 00:00:00\tEndTime={to} 23:59:59"
            ),
            // An empty PIN means every user.
            DeviceCommand::QueryUserInfo => format!("C:{id}:DATA QUERY USERINFO PIN="),
            DeviceCommand::QueryFingerprints => {
                format!("C:{id}:DATA QUERY FINGERTMP PIN=")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_log_query_names_the_range_the_device_expects() {
        let c = DeviceCommand::QueryAttlog {
            from: "2026-08-01".into(),
            to: "2026-08-19".into(),
        };
        let line = c.encode(7);
        assert_eq!(
            line,
            "C:7:DATA QUERY ATTLOG StartTime=2026-08-01 00:00:00\tEndTime=2026-08-19 23:59:59"
        );
        // The id has to match the row the device reports back against, or the
        // command is never marked as done and is handed out again forever.
        assert!(line.starts_with("C:7:"));
    }

    #[test]
    fn a_user_query_asks_for_everyone() {
        assert_eq!(DeviceCommand::QueryUserInfo.encode(3), "C:3:DATA QUERY USERINFO PIN=");
    }

    #[test]
    fn every_command_is_one_line_with_its_id_first() {
        // The device reads one command per line. A newline inside any of these
        // would split it into two commands, the second of them nonsense.
        let all = [
            DeviceCommand::CheckData,
            DeviceCommand::Reboot,
            DeviceCommand::ClearLog,
            DeviceCommand::QueryUserInfo,
            DeviceCommand::QueryFingerprints,
            DeviceCommand::QueryAttlog { from: "2026-01-01".into(), to: "2026-01-31".into() },
            DeviceCommand::DeleteUser { pin: "41".into() },
        ];
        for c in all {
            let line = c.encode(11);
            assert!(!line.contains('\n'), "command spans two lines: {line}");
            assert!(line.starts_with("C:11:"), "id must lead: {line}");
        }
    }




    #[test]
    fn the_timezone_is_a_whole_number() {
        // "TimeZone=5.75" is not a value ADMS understands, and firmware that
        // cannot parse the options block stops enabling its command channel.
        let r = handshake_response("SN", 1, &PushConfig::default());
        assert!(r.contains("TimeZone=6\r\n"), "timezone must be an integer:\n{r}");
        assert!(!r.contains("5.75"), "a fractional offset reached the device");
    }

    #[test]
    fn the_handshake_advertises_a_real_adms_server() {
        // Without ServerVer and PushProtVer a K40 Pro posts its attendance log
        // and then never polls for commands. Nothing errors; the command queue
        // simply fills up forever. These four lines are what prevent that, so
        // they are pinned.
        let r = handshake_response("GED7253800740", 42, &PushConfig::default());
        assert!(r.contains("ServerVer="), "missing ServerVer:\n{r}");
        assert!(r.contains("PushProtVer="), "missing PushProtVer:\n{r}");
        assert!(r.contains("PushOptionsFlag=1"), "missing PushOptionsFlag:\n{r}");
        assert!(r.starts_with("GET OPTION FROM: GED7253800740"));
    }

    #[test]
    fn the_handshake_sends_the_numeric_transflag() {
        // Older firmware does not understand the named list form and quietly
        // sends nothing at all.
        let r = handshake_response("SN", 1, &PushConfig::default());
        assert!(r.contains("TransFlag=1111111111"), "{r}");
        assert!(!r.contains("TransFlag=TransData"), "named form is not universally understood");
    }

    #[test]
    fn every_handshake_line_ends_the_way_the_device_expects() {
        let r = handshake_response("SN", 7, &PushConfig::default());
        for line in r.split("\r\n").filter(|l| !l.is_empty()) {
            assert!(!line.contains('\n'), "bare newline inside a line: {line:?}");
        }
        assert!(r.ends_with("\r\n"), "the last line must be terminated too");
    }

    #[test]
    fn parses_a_fingerprint_post() {
        let body = "FP PIN=41\tFID=0\tSize=1024\tValid=1\tTMP=QUJDRA==\n\
                    FP PIN=41\tFID=1\tSize=980\tValid=1\tTMP=RUZHSA==\n";
        let fps = parse_fingerprint_body(body);
        assert_eq!(fps.len(), 2);
        assert_eq!(fps[0].pin, "41");
        assert_eq!(fps[0].index, 0);
        assert_eq!(fps[0].size, 1024);
        assert!(fps[0].valid);
        // Base64 padding must survive: a truncated template is a useless one.
        assert_eq!(fps[0].template, "QUJDRA==");
        assert_eq!(fps[1].index, 1);
    }

    #[test]
    fn a_template_containing_equals_signs_is_kept_whole() {
        // Splitting on every '=' would cut base64 padding off the end and leave
        // a blob the terminal rejects when it is uploaded back.
        let body = "FP PIN=7\tFID=2\tTMP=YWJjZGVmZ2hpams=";
        assert_eq!(parse_fingerprint_body(body)[0].template, "YWJjZGVmZ2hpams=");
    }

    #[test]
    fn fingerprint_records_without_a_template_or_a_pin_are_skipped() {
        let body = "FP PIN=41\tFID=0\tSize=0\tTMP=\n\
                    FP PIN=\tFID=0\tTMP=QUJD\n\
                    FP PIN=abc\tFID=0\tTMP=QUJD\n\
                    FP PIN=8\tFID=11\tTMP=QUJD\n\
                    FP PIN=9\tFID=3\tTMP=QUJD\n";
        let fps = parse_fingerprint_body(body);
        assert_eq!(fps.len(), 1, "only the usable template should survive");
        assert_eq!(fps[0].pin, "9");
        assert_eq!(fps[0].index, 3);
    }

    #[test]
    fn parses_a_userinfo_post() {
        let body = "USER PIN=41\tName=Sarita Maharjan\tPri=0\tPasswd=\tCard=0\tGrp=1\tTZ=0\n\
                    USER PIN=12\tName=Ramesh Karki\tPri=14\tPasswd=1234\tCard=99\tGrp=2\n";
        let users = parse_userinfo_body(body);
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].pin, "41");
        assert_eq!(users[0].name, "Sarita Maharjan");
        assert_eq!(users[0].privilege, 0);
        assert_eq!(users[1].privilege, 14, "the super admin flag must survive");
        assert_eq!(users[1].card, "99");
        assert_eq!(users[1].group, 2);
    }

    #[test]
    fn userinfo_fields_are_read_by_name_not_position() {
        // Firmware differs in field order and in which fields it sends at all.
        let body = "USER Name=Out Of Order\tGrp=1\tPIN=7\tCard=5";
        let u = &parse_userinfo_body(body)[0];
        assert_eq!(u.pin, "7");
        assert_eq!(u.name, "Out Of Order");
        assert_eq!(u.card, "5");
        assert_eq!(u.password, "", "a missing field is empty, not a parse failure");
    }

    #[test]
    fn pin2_wins_over_the_internal_index() {
        // PIN is the device's own row number; PIN2 is what is typed on the
        // keypad and what every punch is joined on. Reading the wrong one
        // silently attaches a month of attendance to the wrong person.
        let body = "USER PIN=3\tPIN2=118\tName=Nabin Rai";
        assert_eq!(parse_userinfo_body(body)[0].pin, "118");
    }

    #[test]
    fn userinfo_lines_without_a_usable_pin_are_skipped() {
        let body = "USER Name=No Number\tCard=1\n\
                    USER PIN=\tName=Blank\n\
                    USER PIN=abc\tName=Not A Number\n\
                    USER PIN=9\tName=Good\n\
                    \n";
        let users = parse_userinfo_body(body);
        assert_eq!(users.len(), 1, "only the joinable record should survive");
        assert_eq!(users[0].pin, "9");
    }

    #[test]
    fn a_userinfo_body_that_is_rubbish_returns_nothing_rather_than_panicking() {
        assert!(parse_userinfo_body("").is_empty());
        assert!(parse_userinfo_body("\n\n\n").is_empty());
        assert!(parse_userinfo_body("total nonsense with no equals signs").is_empty());
    }

    #[test]
    fn parses_a_normal_attlog_post() {
        let body = "41\t2026-08-18 09:01:23\t0\t1\t0\t0\t0\t\t0\n\
                    12\t2026-08-18 16:05:00\t1\t1\t0\t0\t0\t\t0\n";
        let logs = parse_attlog_body(body);
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].user_id, "41");
        assert_eq!(logs[0].timestamp(), "2026-08-18 09:01:23");
        assert_eq!(logs[0].punch, 0);
        assert_eq!(logs[1].punch, 1);
        assert_eq!(logs[1].timestamp(), "2026-08-18 16:05:00");
    }

    #[test]
    fn skips_bad_lines_but_keeps_good_ones() {
        // A truncated line in the middle must not cost us the surrounding
        // punches — the device would otherwise resend the whole batch forever.
        let body = "41\t2026-08-18 09:01:23\t0\t1\n\
                    garbage-with-no-tabs\n\
                    \t2026-08-18 09:02:00\t0\t1\n\
                    99\tnot-a-date\t0\t1\n\
                    12\t2026-08-18 16:05:00\t1\t1\n";
        let logs = parse_attlog_body(body);
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].user_id, "41");
        assert_eq!(logs[1].user_id, "12");
    }

    #[test]
    fn tolerates_missing_seconds_and_t_separator() {
        let logs = parse_attlog_body("7\t2026-08-18T09:05\t0\t1\n");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].timestamp(), "2026-08-18 09:05:00");
    }

    #[test]
    fn defaults_punch_and_verify_when_columns_absent() {
        let logs = parse_attlog_body("7\t2026-08-18 09:05:00\n");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].punch, 0);
        assert_eq!(logs[0].verify, 1);
    }

    #[test]
    fn rejects_out_of_range_dates() {
        assert!(parse_attlog_body("7\t1970-01-01 09:00:00\t0\t1\n").is_empty());
        assert!(parse_attlog_body("7\t2026-13-01 09:00:00\t0\t1\n").is_empty());
        assert!(parse_attlog_body("7\t2026-08-18 25:00:00\t0\t1\n").is_empty());
    }

    #[test]
    fn empty_body_yields_nothing() {
        assert!(parse_attlog_body("").is_empty());
        assert!(parse_attlog_body("\n\n  \n").is_empty());
    }

    #[test]
    fn handshake_contains_required_keys() {
        let r = handshake_response("GED7253800740", 0, &PushConfig::default());
        assert!(r.starts_with("GET OPTION FROM: GED7253800740"));
        for key in ["Stamp=", "Delay=", "TransFlag=", "TimeZone=6", "Realtime=1"] {
            assert!(r.contains(key), "handshake missing {key}");
        }
    }

    #[test]
    fn query_params_are_extracted_and_decoded() {
        let t = "/iclock/cdata?SN=GED7253800740&table=ATTLOG&Stamp=9999";
        assert_eq!(query_param(t, "SN").unwrap(), "GED7253800740");
        assert_eq!(query_param(t, "table").unwrap(), "ATTLOG");
        assert_eq!(query_param(t, "sn").unwrap(), "GED7253800740", "keys are case-insensitive");
        assert!(query_param(t, "missing").is_none());
        assert!(query_param("/iclock/cdata", "SN").is_none());
    }

    #[test]
    fn url_decoding_handles_escapes() {
        assert_eq!(query_param("/x?n=Sarita%20Maharjan", "n").unwrap(), "Sarita Maharjan");
        assert_eq!(query_param("/x?n=a+b", "n").unwrap(), "a b");
    }

    #[test]
    fn set_user_command_is_well_formed() {
        let c = DeviceCommand::SetUser {
            pin: "41".into(),
            name: "Sarita Maharjan".into(),
            privilege: 0,
            password: String::new(),
            card: "0".into(),
        };
        let line = c.encode(5);
        assert!(line.starts_with("C:5:DATA UPDATE USERINFO PIN=41\t"));
        assert!(line.contains("Name=Sarita Maharjan"));
    }

    #[test]
    fn delete_command_is_well_formed() {
        assert_eq!(
            DeviceCommand::DeleteUser { pin: "41".into() }.encode(9),
            "C:9:DATA DELETE USERINFO PIN=41"
        );
    }
}
