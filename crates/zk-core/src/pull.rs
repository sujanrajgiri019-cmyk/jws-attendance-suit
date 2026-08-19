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

    /// Fetch the user table.
    pub fn users(&mut self) -> Result<Vec<DeviceUser>> {
        self.disable()?;
        let result = self.read_bulk(proto::CMD_USER_TEMP_RRQ, &[0x05, 0x00, 0x00, 0x00, 0x00]);
        let _ = self.enable();
        proto::parse_user_buffer(&result?)
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
