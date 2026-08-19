//! Gmail SMTP.
//!
//! Used for three things: telling staff they were marked absent, the daily
//! summary to the Principal, and the password-recovery code.
//!
//! Gmail requires a 16-character *app password*, not the account password.
//! That distinction is the single most common setup mistake, so it is called
//! out in the error text rather than surfaced as "authentication failed".

use lettre::message::{header::ContentType, Mailbox, Message};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{SmtpTransport, Transport};
use rusqlite::Connection;
use zk_core::db;

pub struct MailSettings {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
    pub from_name: String,
}

// Written by hand rather than derived: a derived Debug would print the Gmail
// app password into any log line or panic message that touches this struct.
impl std::fmt::Debug for MailSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MailSettings")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("pass", &if self.pass.is_empty() { "<unset>" } else { "<redacted>" })
            .field("from_name", &self.from_name)
            .finish()
    }
}

pub fn settings_from_db(conn: &Connection) -> Result<MailSettings, String> {
    let get = |k: &str, d: &str| db::get_setting(conn, k).ok().flatten().unwrap_or_else(|| d.into());
    let pass = get("smtp_pass", "");
    if pass.trim().is_empty() {
        return Err("No Gmail app password is saved yet. Open Settings → Email & Alerts and paste \
                    the 16-character app password generated from the Google account."
            .into());
    }
    Ok(MailSettings {
        host: get("smtp_host", "smtp.gmail.com"),
        port: get("smtp_port", "587").parse().unwrap_or(587),
        user: get("smtp_user", ""),
        pass,
        from_name: get("school_name", "Janapremi World School"),
    })
}

fn transport(s: &MailSettings) -> Result<SmtpTransport, String> {
    let creds = Credentials::new(s.user.clone(), s.pass.clone());
    // Port 465 is implicit TLS; 587 upgrades with STARTTLS.
    let builder = if s.port == 465 {
        SmtpTransport::relay(&s.host).map_err(|e| e.to_string())?
    } else {
        SmtpTransport::starttls_relay(&s.host).map_err(|e| e.to_string())?
    };
    Ok(builder.port(s.port).credentials(creds).build())
}

/// Send one message. `to` may be any valid address.
pub fn send(s: &MailSettings, to: &str, subject: &str, body_html: &str) -> Result<(), String> {
    let from: Mailbox = format!("{} <{}>", s.from_name, s.user)
        .parse()
        .map_err(|e| format!("The sender address '{}' is not valid: {e}", s.user))?;
    let to_box: Mailbox =
        to.parse().map_err(|e| format!("'{to}' is not a valid email address: {e}"))?;

    let email = Message::builder()
        .from(from)
        .to(to_box)
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(body_html.to_string())
        .map_err(|e| format!("could not build the message: {e}"))?;

    transport(s)?.send(&email).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("535") || msg.to_lowercase().contains("username and password") {
            "Gmail rejected the sign-in. Make sure you are using a 16-character app password \
             (Google Account → Security → App passwords), not the normal account password."
                .to_string()
        } else if msg.contains("timed out") || msg.contains("dns") {
            format!("Could not reach {}: {msg}. Check the internet connection.", s.host)
        } else {
            msg
        }
    })?;
    Ok(())
}

/// Wrap content in the school's letterhead styling.
pub fn template(school: &str, address: &str, heading: &str, body: &str) -> String {
    format!(
        r#"<!doctype html><html><body style="margin:0;padding:24px;background:#f6f7f9;
 font-family:Segoe UI,Helvetica,Arial,sans-serif;color:#333">
<div style="max-width:560px;margin:0 auto;background:#fff;border:1px solid #e7e9ec;border-radius:12px;overflow:hidden">
  <div style="background:#F16522;padding:18px 24px">
    <div style="color:#fff;font-size:17px;font-weight:700">{school}</div>
    <div style="color:#ffe5d8;font-size:12px;margin-top:2px">{address}</div>
  </div>
  <div style="padding:24px">
    <h2 style="margin:0 0 12px;font-size:17px;color:#333">{heading}</h2>
    {body}
  </div>
  <div style="padding:14px 24px;background:#fafbfc;border-top:1px solid #e7e9ec;
       font-size:11px;color:#868d96">
    Sent automatically by JWS Attendance. Please do not reply to this message.
  </div>
</div></body></html>"#
    )
}

/// The message a member of staff gets when they were marked absent.
pub fn absence_notice(school: &str, address: &str, name: &str, date: &str, date_bs: &str) -> String {
    let bs = if date_bs.is_empty() {
        String::new()
    } else {
        format!(" ({date_bs})")
    };
    template(
        school,
        address,
        "Attendance not recorded",
        &format!(
            r#"<p style="margin:0 0 12px;font-size:14px">Dear {name},</p>
<p style="margin:0 0 12px;font-size:14px;line-height:1.6">
  Our records show no attendance was registered for you on
  <b>{date}</b>{bs}.
</p>
<p style="margin:0 0 12px;font-size:14px;line-height:1.6">
  If you were present and this is a mistake — a missed scan, or a problem with the
  terminal — please inform the school office so the record can be corrected.
</p>
<p style="margin:0;font-size:14px">Thank you.</p>"#
        ),
    )
}

/// The password-recovery message.
pub fn reset_code_mail(school: &str, address: &str, code: &str) -> String {
    template(
        school,
        address,
        "Your password reset code",
        &format!(
            r#"<p style="margin:0 0 16px;font-size:14px;line-height:1.6">
  Someone asked to reset the JWS Attendance administrator password on the school computer.
  Enter this code in the application to continue:
</p>
<div style="text-align:center;margin:20px 0">
  <span style="display:inline-block;font-size:30px;font-weight:700;letter-spacing:9px;
    color:#F16522;background:#fff4ee;border:1px solid #ffe5d8;border-radius:10px;
    padding:14px 22px">{code}</span>
</div>
<p style="margin:0;font-size:13px;color:#5a5f66;line-height:1.6">
  The code stops working after 15 minutes. If you did not request this, no action is
  needed — but do tell whoever manages the school computer.
</p>"#
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absence_notice_contains_the_essentials() {
        let m = absence_notice(
            "Janapremi World School",
            "Madhyapur Thimi-3",
            "Sarita Maharjan",
            "2026-08-19",
            "3 Bhadra 2083",
        );
        assert!(m.contains("Sarita Maharjan"));
        assert!(m.contains("2026-08-19"));
        assert!(m.contains("3 Bhadra 2083"));
        assert!(m.contains("Janapremi World School"));
        assert!(m.contains("#F16522"), "school colour should carry through");
    }

    #[test]
    fn absence_notice_omits_empty_bs_date_cleanly() {
        let m = absence_notice("S", "A", "N", "2026-08-19", "");
        assert!(!m.contains("()"), "must not leave empty brackets");
    }

    #[test]
    fn reset_mail_shows_the_code_and_the_expiry() {
        let m = reset_code_mail("S", "A", "483920");
        assert!(m.contains("483920"));
        assert!(m.contains("15 minutes"));
    }

    #[test]
    fn missing_app_password_is_reported_before_any_connection_attempt() {
        let conn = zk_core::db::open_memory().unwrap();
        let err = settings_from_db(&conn).unwrap_err();
        assert!(err.contains("app password"), "got: {err}");
    }

    #[test]
    fn debug_output_never_contains_the_password() {
        // This struct ends up in log lines and panic messages; the Gmail app
        // password must not travel with it.
        let s = MailSettings {
            host: "smtp.gmail.com".into(),
            port: 587,
            user: "jws.staffattendance@gmail.com".into(),
            pass: "abcdefghijklmnop".into(),
            from_name: "Janapremi World School".into(),
        };
        let text = format!("{s:?}");
        assert!(!text.contains("abcdefghijklmnop"), "password leaked: {text}");
        assert!(text.contains("<redacted>"));
        assert!(text.contains("smtp.gmail.com"), "the useful fields are still there");
    }

    #[test]
    fn settings_load_once_a_password_is_present() {
        let conn = zk_core::db::open_memory().unwrap();
        zk_core::db::set_setting(&conn, "smtp_pass", "abcd efgh ijkl mnop").unwrap();
        let s = settings_from_db(&conn).unwrap();
        assert_eq!(s.host, "smtp.gmail.com");
        assert_eq!(s.port, 587);
        assert_eq!(s.from_name, "Janapremi World School");
    }
}
