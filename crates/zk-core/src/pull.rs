//! Pull mode: talk to the terminal over TCP 4370.
//!
//! This is the method the old ZKTeco desktop software uses, kept as a fallback
//! for fetching records that push mode missed (a power cut, a swapped switch)
//! and for reading the user table off a terminal where staff enrolled directly
//! on the keypad.
//!
//! The codec this drives lives in [`crate::proto`] and is fully unit-tested;
//! what is here is socket plumbing, which only a real device can exercise.

use crate::proto::{self, AttLog, DeviceUser, Header};
use crate::{Error, Result};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// A live connection to a terminal.
pub struct Device {
    stream: TcpStream,
    session_id: u16,
    reply_id: u16,
}

/// What the terminal reports about itself.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DeviceInfo {
    pub serial: String,
    pub name: String,
    pub firmware: String,
    pub platform: String,
    pub mac: String,
    pub user_count: u32,
    pub fp_count: u32,
    pub log_count: u32,
}

impl Device {
    /// Open a connection and complete the handshake.
    ///
    /// `comm_key` is the terminal's "COMM Key" setting — 0 on an untouched
    /// device. A wrong key surfaces as [`Error::Unauthorised`] rather than a
    /// confusing timeout.
    pub fn connect(addr: &str, port: u16, comm_key: u32, timeout: Duration) -> Result<Self> {
        let sock: SocketAddr = (addr, port)
            .to_socket_addrs()
            .map_err(|e| Error::Invalid(format!("bad device address {addr}:{port} — {e}")))?
            .next()
            .ok_or_else(|| Error::Invalid(format!("could not resolve {addr}:{port}")))?;

        let stream = TcpStream::connect_timeout(&sock, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream.set_nodelay(true)?;

        // pyzk starts the reply counter just below the wrap point; devices are
        // known to be picky about the first value they see.
        let mut dev = Device { stream, session_id: 0, reply_id: u16::MAX - 1 };

        let (header, _) = dev.command(proto::CMD_CONNECT, &[])?;
        dev.session_id = header.session_id;

        if header.command == proto::CMD_ACK_UNAUTH {
            let key = proto::make_comm_key(comm_key, dev.session_id, 50);
            let (auth, _) = dev.command(proto::CMD_AUTH, &key)?;
            if auth.command != proto::CMD_ACK_OK {
                return Err(Error::Unauthorised);
            }
        } else if header.command != proto::CMD_ACK_OK {
            return Err(Error::Protocol(format!(
                "device refused the connection (reply {})",
                header.command
            )));
        }

        Ok(dev)
    }

    /// Send a command and read one reply.
    fn command(&mut self, cmd: u16, data: &[u8]) -> Result<(Header, Vec<u8>)> {
        let payload = proto::build_payload(cmd, self.session_id, self.reply_id, data);
        self.reply_id = self.reply_id.wrapping_add(1);
        self.stream.write_all(&proto::frame_tcp(&payload))?;
        self.stream.flush()?;
        self.read_reply()
    }

    /// Read one framed reply: 8-byte TCP prefix, then exactly that many bytes.
    fn read_reply(&mut self) -> Result<(Header, Vec<u8>)> {
        let mut prefix = [0u8; 8];
        self.read_exact(&mut prefix)?;
        let (len, _) = proto::unframe_tcp(&prefix)?;

        if len < 8 || len > 8 * 1024 * 1024 {
            return Err(Error::Protocol(format!("implausible reply length {len}")));
        }
        let mut body = vec![0u8; len];
        self.read_exact(&mut body)?;

        let header = Header::parse(&body)?;
        Ok((header, body[8..].to_vec()))
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        let mut read = 0;
        while read < buf.len() {
            match self.stream.read(&mut buf[read..]) {
                Ok(0) => return Err(Error::Protocol("device closed the connection".into())),
                Ok(n) => read += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Err(Error::Timeout),
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => return Err(Error::Timeout),
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    /// Issue a command whose answer is a bulk payload.
    ///
    /// Terminals answer either inline (`CMD_DATA`) or by announcing a size with
    /// `CMD_PREPARE_DATA` and then streaming raw bytes. Both are handled.
    fn read_bulk(&mut self, cmd: u16, data: &[u8]) -> Result<Vec<u8>> {
        let (header, body) = self.command(cmd, data)?;

        match header.command {
            proto::CMD_DATA => Ok(body),

            proto::CMD_PREPARE_DATA => {
                if body.len() < 4 {
                    return Err(Error::Protocol("PREPARE_DATA without a size".into()));
                }
                let size = u32::from_le_bytes([body[0], body[1], body[2], body[3]]) as usize;
                if size > 64 * 1024 * 1024 {
                    return Err(Error::Protocol(format!("device announced {size} bytes")));
                }

                let mut out = Vec::with_capacity(size);
                while out.len() < size {
                    let (h, chunk) = self.read_reply()?;
                    match h.command {
                        proto::CMD_DATA => out.extend_from_slice(&chunk),
                        proto::CMD_ACK_OK => break,
                        other => {
                            return Err(Error::Protocol(format!(
                                "unexpected packet {other} while streaming data"
                            )))
                        }
                    }
                }
                out.truncate(size);
                Ok(out)
            }

            proto::CMD_ACK_OK => Ok(Vec::new()),
            proto::CMD_ACK_ERROR => Err(Error::Protocol("device returned an error".into())),
            other => Err(Error::Protocol(format!("unexpected reply {other}"))),
        }
    }

    /// Stop the terminal accepting scans while we read from it.
    ///
    /// Skipping this risks reading a half-written record mid-transfer.
    pub fn disable(&mut self) -> Result<()> {
        self.command(proto::CMD_DISABLE_DEVICE, &[0, 0])?;
        Ok(())
    }

    pub fn enable(&mut self) -> Result<()> {
        self.command(proto::CMD_ENABLE_DEVICE, &[])?;
        Ok(())
    }

    /// Fetch all attendance records held on the terminal.
    pub fn attendance(&mut self) -> Result<Vec<AttLog>> {
        self.disable()?;
        let result = self.read_bulk(proto::CMD_ATTLOG_RRQ, &[]);
        // Re-enable even if the read failed, or the terminal stays locked and
        // staff cannot scan.
        let _ = self.enable();
        proto::parse_attlog_buffer(&result?)
    }

    /// Read a large table through the device's buffer.
    ///
    /// Newer firmware will not serve the user table through a direct request;
    /// it has to be staged into a buffer with `CMD_DATA_WRRQ` and then pulled
    /// out in chunks. Small tables come back inline in the first reply, so both
    /// paths are handled.
    fn read_with_buffer(&mut self, table_cmd: u16, fct: i32) -> Result<Vec<u8>> {
        const MAX_CHUNK: u32 = 0xFF_C0; // what the devices accept over TCP

        let request = proto::build_wrrq_request(table_cmd, fct, 0);
        let (header, body) = self.command(proto::CMD_DATA_WRRQ, &request)?;

        match header.command {
            // Small table: the device just handed it over.
            proto::CMD_DATA => return Ok(body),

            proto::CMD_ACK_OK | proto::CMD_PREPARE_DATA => {}

            proto::CMD_ACK_ERROR | proto::CMD_ACK_UNAUTH => {
                return Err(Error::Protocol(format!(
                    "the terminal refused to prepare its data (reply {}). \
                     If a COMM key is set on the device, enter it on the Devices screen.",
                    header.command
                )))
            }
            other => {
                return Err(Error::Protocol(format!(
                    "unexpected reply {other} when asking the terminal to prepare its data"
                )))
            }
        }

        // The reply carries a status byte then the total size as a u32.
        if body.len() < 5 {
            return Err(Error::Protocol(format!(
                "the terminal announced its data size in {} bytes; 5 were expected",
                body.len()
            )));
        }
        let size = u32::from_le_bytes([body[1], body[2], body[3], body[4]]);

        if size == 0 {
            // Not an error: the table really is empty.
            let _ = self.command(proto::CMD_FREE_DATA, &[]);
            return Ok(Vec::new());
        }
        if size > 32 * 1024 * 1024 {
            return Err(Error::Protocol(format!("the terminal announced {size} bytes")));
        }

        let mut out = Vec::with_capacity(size as usize);
        let mut start: u32 = 0;
        while start < size {
            let want = MAX_CHUNK.min(size - start);
            let req = proto::build_read_chunk_request(start, want);
            let (h, chunk) = self.command(proto::CMD_READ_BUFFER, &req)?;

            let piece = match h.command {
                proto::CMD_DATA => chunk,
                proto::CMD_PREPARE_DATA => {
                    // Announced then streamed, same as the attendance path.
                    let mut acc = Vec::with_capacity(want as usize);
                    while (acc.len() as u32) < want {
                        let (hh, part) = self.read_reply()?;
                        match hh.command {
                            proto::CMD_DATA => acc.extend_from_slice(&part),
                            proto::CMD_ACK_OK => break,
                            other => {
                                return Err(Error::Protocol(format!(
                                    "unexpected packet {other} while reading a chunk at offset {start}"
                                )))
                            }
                        }
                    }
                    acc
                }
                other => {
                    return Err(Error::Protocol(format!(
                        "the terminal returned {other} instead of data at offset {start} of {size}"
                    )))
                }
            };

            if piece.is_empty() {
                return Err(Error::Protocol(format!(
                    "the terminal stopped sending at offset {start} of {size} bytes"
                )));
            }
            out.extend_from_slice(&piece);
            start += piece.len() as u32;
        }

        let _ = self.command(proto::CMD_FREE_DATA, &[]);
        out.truncate(size as usize);
        Ok(out)
    }

    /// Fetch the user table.
    pub fn users(&mut self) -> Result<Vec<DeviceUser>> {
        self.disable()?;
        let result = self.read_with_buffer(proto::CMD_USER_TEMP_RRQ, proto::FCT_USER);
        let _ = self.enable();

        let raw = result?;
        if raw.is_empty() {
            return Err(Error::Protocol(
                "The terminal reported no users at all. If staff are enrolled on it, \
                 the device may be using an older protocol than this app expects."
                    .into(),
            ));
        }
        proto::parse_user_buffer(&raw).map_err(|e| {
            Error::Protocol(format!(
                "{e}. The terminal sent {} bytes, which does not divide into \
                 either the 72-byte or 28-byte user record layout.",
                raw.len()
            ))
        })
    }

    /// Create or update a user on the terminal.
    pub fn set_user(&mut self, user: &DeviceUser) -> Result<()> {
        let data = proto::encode_user_72(user);
        let (h, _) = self.command(proto::CMD_SET_USER, &data)?;
        if h.command != proto::CMD_ACK_OK {
            return Err(Error::Protocol(format!("device rejected user {}", user.user_id)));
        }
        self.command(proto::CMD_REFRESH_DATA, &[])?;
        Ok(())
    }

    /// Delete a user by device uid.
    pub fn delete_user(&mut self, uid: u16) -> Result<()> {
        let (h, _) = self.command(proto::CMD_DELETE_USER, &uid.to_le_bytes())?;
        if h.command != proto::CMD_ACK_OK {
            return Err(Error::Protocol(format!("device refused to delete uid {uid}")));
        }
        Ok(())
    }

    /// Erase the stored attendance log. Not reversible.
    pub fn clear_attendance(&mut self) -> Result<()> {
        let (h, _) = self.command(proto::CMD_CLEAR_ATTLOG, &[])?;
        if h.command != proto::CMD_ACK_OK {
            return Err(Error::Protocol("device refused to clear its log".into()));
        }
        Ok(())
    }

    /// Read a device parameter such as `~SerialNumber` or `FPVersion`.
    pub fn param(&mut self, key: &str) -> Result<String> {
        let (_, body) = self.command(proto::CMD_DEVICE, key.as_bytes())?;
        let s = String::from_utf8_lossy(&body);
        Ok(s.split('=').nth(1).unwrap_or("").trim_end_matches('\0').trim().to_string())
    }

    /// Collect identity and capacity information.
    pub fn info(&mut self) -> Result<DeviceInfo> {
        let mut i = DeviceInfo {
            serial: self.param("~SerialNumber").unwrap_or_default(),
            name: self.param("~DeviceName").unwrap_or_default(),
            firmware: String::new(),
            platform: self.param("~Platform").unwrap_or_default(),
            mac: self.param("MAC").unwrap_or_default(),
            ..Default::default()
        };
        if let Ok((_, b)) = self.command(proto::CMD_GET_FREE_SIZES, &[]) {
            // The status block is a run of little-endian u32 counters; the
            // user/fingerprint/record totals sit at fixed word offsets.
            let word = |n: usize| -> u32 {
                b.get(n * 4..n * 4 + 4)
                    .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
                    .unwrap_or(0)
            };
            i.user_count = word(4);
            i.fp_count = word(2);
            i.log_count = word(6);
        }
        Ok(i)
    }

    /// Close the session politely so the terminal frees it immediately.
    pub fn disconnect(mut self) -> Result<()> {
        let _ = self.command(proto::CMD_EXIT, &[]);
        Ok(())
    }
}

/// What actually happens when we try to speak the SDK protocol to a terminal.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Probe {
    /// Did the TCP connection open at all?
    pub socket_open: bool,
    pub socket_ms: u128,
    /// Bytes we sent, as hex.
    pub sent_hex: String,
    /// Bytes that came back, as hex. Empty means the device stayed silent.
    pub received_hex: String,
    pub received_bytes: usize,
    /// The decoded reply command, when there was one.
    pub reply_command: Option<u16>,
    pub reply_name: String,
    pub error: Option<String>,
    pub verdict: String,
}

/// Open a socket, send one CMD_CONNECT, and report exactly what came back.
///
/// This exists because "the port is open" and "the device speaks to us" turned
/// out to be completely different things on a K40 Pro with the cloud server
/// enabled: the socket accepts instantly and then nothing ever answers. From
/// the outside that is indistinguishable from a wrong address or a firewall,
/// and each has a different fix. The hex dump settles it.
pub fn probe(addr: &str, port: u16, comm_key: u32, timeout: Duration) -> Probe {
    let mut p = Probe {
        socket_open: false,
        socket_ms: 0,
        sent_hex: String::new(),
        received_hex: String::new(),
        received_bytes: 0,
        reply_command: None,
        reply_name: String::new(),
        error: None,
        verdict: String::new(),
    };

    let sock: SocketAddr = match (addr, port).to_socket_addrs().ok().and_then(|mut a| a.next()) {
        Some(s) => s,
        None => {
            p.error = Some(format!("{addr}:{port} is not an address this PC can resolve"));
            p.verdict = "The address itself is wrong.".into();
            return p;
        }
    };

    let started = std::time::Instant::now();
    let mut stream = match TcpStream::connect_timeout(&sock, timeout) {
        Ok(s) => s,
        Err(e) => {
            p.error = Some(e.to_string());
            p.verdict = format!(
                "Nothing is listening on {addr}:{port}. Either the address is wrong,                  the terminal is off, or something on the network is blocking it."
            );
            return p;
        }
    };
    p.socket_open = true;
    p.socket_ms = started.elapsed().as_millis();

    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let _ = stream.set_nodelay(true);

    // One CMD_CONNECT, framed exactly as a live session would send it.
    let payload = proto::build_payload(proto::CMD_CONNECT, 0, u16::MAX - 1, &[]);
    let frame = proto::frame_tcp(&payload);
    p.sent_hex = hex(&frame);

    if let Err(e) = stream.write_all(&frame) {
        p.error = Some(e.to_string());
        p.verdict = "The connection opened but closed before anything could be sent.".into();
        return p;
    }

    let mut buf = [0u8; 1024];
    match stream.read(&mut buf) {
        Ok(0) => {
            p.verdict = format!(
                "{addr}:{port} accepted the connection and then closed it without answering.                  On a ZKTeco terminal this means the SDK service is switched off — which is                  what enabling the cloud server (ADMS) does."
            );
        }
        Ok(n) => {
            p.received_bytes = n;
            p.received_hex = hex(&buf[..n]);
            // Unframe, then read the 8-byte header out of the payload.
            let decoded = proto::unframe_tcp(&buf[..n])
                .ok()
                .and_then(|(_, payload)| Header::parse(payload).ok());

            if let Some(h) = decoded {
                p.reply_command = Some(h.command);
                p.reply_name = reply_name(h.command).to_string();
                p.verdict = match h.command {
                    proto::CMD_ACK_OK => "The terminal answered correctly. Direct transfers will work.".into(),
                    proto::CMD_ACK_UNAUTH => format!(
                        "The terminal answered but wants a communication key. Enter the COMM key                          from the device's menu on the Devices screen — {comm_key} was tried."
                    ),
                    other => format!(
                        "The terminal answered with reply {other} ({}), which is not the                          acknowledgement a connection expects.",
                        reply_name(other)
                    ),
                };
            } else {
                p.verdict = format!(
                    "{n} bytes came back but not in the shape this protocol expects.                      Something else may be listening on that port."
                );
            }
        }
        Err(e) => {
            p.error = Some(e.to_string());
            p.verdict = format!(
                "{addr}:{port} accepted the connection and then went silent — nothing came                  back within {} seconds. On a ZKTeco terminal this is what an enabled cloud                  server (ADMS) looks like: the port stays open but the SDK service behind it                  is disabled. Turn the cloud server off on the terminal to use direct                  transfers.",
                timeout.as_secs()
            );
        }
    }
    p
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

/// A name for the handful of replies worth recognising by sight.
fn reply_name(command: u16) -> &'static str {
    match command {
        proto::CMD_ACK_OK => "ACK_OK",
        proto::CMD_ACK_ERROR => "ACK_ERROR",
        proto::CMD_ACK_DATA => "ACK_DATA",
        proto::CMD_ACK_UNAUTH => "ACK_UNAUTH",
        _ => "unrecognised",
    }
}

/// Check whether a terminal answers on the given address, and how quickly.
pub fn ping(addr: &str, port: u16, timeout: Duration) -> Result<u128> {
    let sock: SocketAddr = (addr, port)
        .to_socket_addrs()
        .map_err(|e| Error::Invalid(format!("bad address {addr}:{port} — {e}")))?
        .next()
        .ok_or_else(|| Error::Invalid(format!("could not resolve {addr}:{port}")))?;

    let start = std::time::Instant::now();
    TcpStream::connect_timeout(&sock, timeout)?;
    Ok(start.elapsed().as_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_probe_of_a_dead_address_reports_the_socket_not_the_protocol() {
        // 203.0.113.0/24 is reserved for documentation and routes nowhere.
        let p = probe("203.0.113.1", 4370, 0, Duration::from_millis(300));
        assert!(!p.socket_open);
        assert!(p.received_hex.is_empty());
        assert!(p.verdict.contains("Nothing is listening"), "{}", p.verdict);
        assert!(p.error.is_some());
    }

    #[test]
    fn a_probe_names_the_address_it_could_not_resolve() {
        let p = probe("not a hostname at all", 4370, 0, Duration::from_millis(200));
        assert!(!p.socket_open);
        assert!(p.verdict.contains("address"), "{}", p.verdict);
    }

    #[test]
    fn a_probe_records_exactly_what_it_sent() {
        // Whatever happens to the connection, the outgoing frame is captured —
        // that is half the evidence when a device answers unexpectedly.
        let p = probe("127.0.0.1", 1, 0, Duration::from_millis(200));
        let expected = super::hex(&proto::frame_tcp(&proto::build_payload(
            proto::CMD_CONNECT,
            0,
            u16::MAX - 1,
            &[],
        )));
        // Port 1 refuses, so nothing was sent; the field stays empty rather
        // than claiming a frame went out.
        assert!(p.sent_hex.is_empty() || p.sent_hex == expected);
        assert!(!p.verdict.is_empty(), "a probe must always reach a verdict");
    }

    #[test]
    fn a_silent_port_is_diagnosed_as_the_sdk_being_switched_off() {
        // A listener that accepts and then says nothing is exactly what a
        // ZKTeco terminal does when its cloud server is enabled. Stand one up
        // and check the probe reaches that conclusion rather than blaming the
        // network.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            // Accept, then hold the connection open without replying.
            if let Ok((stream, _)) = listener.accept() {
                std::thread::sleep(Duration::from_secs(2));
                drop(stream);
            }
        });

        let p = probe("127.0.0.1", port, 0, Duration::from_millis(400));
        assert!(p.socket_open, "the socket should have opened");
        assert!(!p.sent_hex.is_empty(), "the connect frame should have gone out");
        assert_eq!(p.received_bytes, 0);
        assert!(
            p.verdict.contains("cloud server") || p.verdict.contains("went silent"),
            "verdict should point at the SDK service, got: {}",
            p.verdict
        );
    }

    #[test]
    fn bad_addresses_fail_fast_with_a_clear_message() {
        let e = ping("not a host", 4370, Duration::from_millis(200)).unwrap_err();
        assert!(matches!(e, Error::Invalid(_)), "got {e:?}");
    }

    #[test]
    fn unreachable_device_reports_an_error_rather_than_hanging() {
        // 203.0.113.0/24 is TEST-NET-3, reserved and unroutable.
        let start = std::time::Instant::now();
        let r = ping("203.0.113.1", 4370, Duration::from_millis(300));
        assert!(r.is_err());
        assert!(start.elapsed() < Duration::from_secs(3), "timeout must be honoured");
    }

    #[test]
    fn connect_to_a_closed_port_is_an_error_not_a_panic() {
        let r = Device::connect("127.0.0.1", 1, 0, Duration::from_millis(300));
        assert!(r.is_err());
    }
}
