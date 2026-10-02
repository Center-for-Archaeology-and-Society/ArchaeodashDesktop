//! Email adapters for verification and reset messages (Section 11.1:
//! "Email sender supports SMTP and local sendmail adapters plus an explicit
//! development sink. Never log tokens or full email content in production").
//!
//! Security rules encoded here:
//! - Tokens appear only in the message body produced by the caller; the
//!   [`EmailSender`] trait returns outcome metadata, never content.
//! - [`redact_email`] masks the local part for any caller-side logging.
//! - The dev sink is explicit: constructing it is a deliberate development
//!   choice, and it stores messages in memory for inspection instead of
//!   delivering anything.
//! - The sendmail adapter never passes recipient addresses through a shell;
//!   the message is written to the command's stdin.

use std::sync::{Arc, Mutex};
use thiserror::Error;

/// A plain-text account email (verification link, reset link, notice).
/// Links carry one-time presentation tokens; nothing here is logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailMessage {
    pub to: String,
    pub subject: String,
    pub text_body: String,
}

#[derive(Debug, Error)]
pub enum EmailError {
    /// Delivery failed. The source detail names the transport mechanism
    /// (not message content) so operators can diagnose without exposing
    /// tokens or addresses in application logs.
    #[error("email delivery failed via {mechanism}")]
    SendFailed {
        mechanism: &'static str,
        #[source]
        detail: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
    #[error("email address is not valid for sending")]
    InvalidAddress,
}

/// Outcome reported to callers for audit logging (metadata only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendOutcome {
    /// Handed to the transport (SMTP accepted, sendmail exited 0).
    Sent,
    /// Dev sink stored the message instead of sending.
    Stored,
}

/// Delivery abstraction over the three Section 11.1 adapters. Object-safe
/// via boxed futures so configuration picks an adapter at runtime
/// (`Arc<dyn EmailSender>`).
pub trait EmailSender: Send + Sync {
    fn send(
        &self,
        message: EmailMessage,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<SendOutcome, EmailError>> + Send + '_>,
    >;
}

/// Masks the local part of an email for logs: `user@example.com` becomes
/// `u***@example.com`. Empty or malformed inputs redact completely.
pub fn redact_email(email: &str) -> String {
    match email.split_once('@') {
        Some((local, domain)) if !local.is_empty() && !domain.is_empty() => {
            let mut masked = String::with_capacity(email.len());
            masked.push_str(&local[..1]);
            masked.push_str("***@");
            masked.push_str(domain);
            masked
        }
        _ => "***".to_string(),
    }
}

/// Builds an absolute action link (verify/reset) from a configured base URL.
/// Rejects base URLs with paths or queries so tokens cannot end up in
/// unexpected origins; the API layer owns the path.
pub fn build_action_link(
    base_url: &str,
    path: &str,
    token_presentation: &str,
) -> Result<String, EmailError> {
    let base = url::Url::parse(base_url).map_err(|_| EmailError::SendFailed {
        mechanism: "link-build",
        detail: None,
    })?;
    if !matches!(base.scheme(), "http" | "https") || !base.path().is_empty() && base.path() != "/" {
        return Err(EmailError::SendFailed {
            mechanism: "link-build",
            detail: None,
        });
    }
    let joined = base
        .join(path.trim_start_matches('/'))
        .map_err(|_| EmailError::SendFailed {
            mechanism: "link-build",
            detail: None,
        })?;
    Ok(format!("{joined}?token={token_presentation}"))
}

/// Development sink: stores messages for inspection; never delivers.
/// Construction is explicit so production configurations cannot pick it up
/// by accident.
#[derive(Clone, Default)]
pub struct DevSinkEmailSender {
    stored: Arc<Mutex<Vec<EmailMessage>>>,
}

impl DevSinkEmailSender {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot of stored messages (dev/test inspection only).
    pub fn stored(&self) -> Vec<EmailMessage> {
        self.stored
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl EmailSender for DevSinkEmailSender {
    fn send(
        &self,
        message: EmailMessage,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<SendOutcome, EmailError>> + Send + '_>,
    > {
        Box::pin(async move {
            if message.to.is_empty() {
                return Err(EmailError::InvalidAddress);
            }
            self.stored
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(message);
            Ok(SendOutcome::Stored)
        })
    }
}

/// Local sendmail adapter: pipes an RFC 5322 message to the command's stdin.
/// Default command is `sendmail -t -i`; no shell, no recipient argv exposure.
#[derive(Debug, Clone)]
pub struct SendmailEmailSender {
    command: String,
    args: Vec<String>,
}

impl SendmailEmailSender {
    pub fn new() -> Self {
        Self {
            command: "sendmail".to_string(),
            args: vec!["-t".into(), "-i".into()],
        }
    }

    pub fn with_command(command: String, args: Vec<String>) -> Self {
        Self { command, args }
    }

    fn render_rfc5322(message: &EmailMessage) -> String {
        // Header values are caller-controlled literals (subjects, addresses);
        // strip CR/LF to prevent header injection.
        let clean = |s: &str| s.replace(['\r', '\n'], " ");
        format!(
            "To: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=utf-8\r\nMIME-Version: 1.0\r\n\r\n{}\r\n",
            clean(&message.to),
            clean(&message.subject),
            message.text_body
        )
    }
}

impl Default for SendmailEmailSender {
    fn default() -> Self {
        Self::new()
    }
}

impl EmailSender for SendmailEmailSender {
    fn send(
        &self,
        message: EmailMessage,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<SendOutcome, EmailError>> + Send + '_>,
    > {
        Box::pin(async move {
            if message.to.is_empty() {
                return Err(EmailError::InvalidAddress);
            }
            let rendered = Self::render_rfc5322(&message);
            let mut child = tokio::process::Command::new(&self.command)
                .args(&self.args)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| EmailError::SendFailed {
                    mechanism: "sendmail",
                    detail: Some(Box::new(e)),
                })?;
            let Some(mut stdin) = child.stdin.take() else {
                return Err(EmailError::SendFailed {
                    mechanism: "sendmail",
                    detail: None,
                });
            };
            tokio::io::AsyncWriteExt::write_all(&mut stdin, rendered.as_bytes())
                .await
                .map_err(|e| EmailError::SendFailed {
                    mechanism: "sendmail",
                    detail: Some(Box::new(e)),
                })?;
            drop(stdin);
            let status = child.wait().await.map_err(|e| EmailError::SendFailed {
                mechanism: "sendmail",
                detail: Some(Box::new(e)),
            })?;
            if status.success() {
                Ok(SendOutcome::Sent)
            } else {
                Err(EmailError::SendFailed {
                    mechanism: "sendmail",
                    detail: Some(Box::new(std::io::Error::other(format!(
                        "exit status: {status}"
                    )))),
                })
            }
        })
    }
}

/// SMTP adapter over `lettre` with rustls TLS. Configuration comes from
/// deployment env; credentials never appear in logs or errors.
#[derive(Debug, Clone)]
pub struct SmtpEmailSender {
    host: String,
    port: u16,
    username: String,
    password: String,
    from: String,
}

impl SmtpEmailSender {
    pub fn new(host: String, port: u16, username: String, password: String, from: String) -> Self {
        Self {
            host,
            port,
            username,
            password,
            from,
        }
    }
}

impl EmailSender for SmtpEmailSender {
    fn send(
        &self,
        message: EmailMessage,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<SendOutcome, EmailError>> + Send + '_>,
    > {
        Box::pin(async move {
            use lettre::message::Mailbox;
            use lettre::transport::smtp::authentication::Credentials;
            use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

            let from: Mailbox = self.from.parse().map_err(|_| EmailError::SendFailed {
                mechanism: "smtp",
                detail: None,
            })?;
            let to: Mailbox = message.to.parse().map_err(|_| EmailError::InvalidAddress)?;
            let email = Message::builder()
                .from(from)
                .to(to)
                .subject(&message.subject)
                .body(message.text_body.clone())
                .map_err(|_| EmailError::SendFailed {
                    mechanism: "smtp",
                    detail: None,
                })?;
            let mailer = AsyncSmtpTransport::<Tokio1Executor>::relay(&self.host)
                .map_err(|e| EmailError::SendFailed {
                    mechanism: "smtp",
                    detail: Some(Box::new(e)),
                })?
                .port(self.port)
                .credentials(Credentials::new(
                    self.username.clone(),
                    self.password.clone(),
                ))
                .build();
            mailer
                .send(email)
                .await
                .map(|_| SendOutcome::Sent)
                .map_err(|e| EmailError::SendFailed {
                    mechanism: "smtp",
                    detail: Some(Box::new(e)),
                })
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;

    #[tokio::test]
    async fn dev_sink_stores_and_reports_metadata_only() {
        let sink = DevSinkEmailSender::new();
        let outcome = sink
            .send(EmailMessage {
                to: "user@example.com".into(),
                subject: "Verify your account".into(),
                text_body: "https://host/auth/verify?token=secret-token".into(),
            })
            .await
            .expect("dev sink send");
        assert_eq!(outcome, SendOutcome::Stored);
        let stored = sink.stored();
        assert_eq!(stored.len(), 1);
        assert!(stored[0].text_body.contains("secret-token"));
    }

    #[tokio::test]
    async fn dev_sink_rejects_empty_address() {
        let sink = DevSinkEmailSender::new();
        let err = sink
            .send(EmailMessage {
                to: String::new(),
                subject: "s".into(),
                text_body: "b".into(),
            })
            .await
            .expect_err("empty address must fail");
        assert!(matches!(err, EmailError::InvalidAddress));
        assert!(sink.stored().is_empty());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn sendmail_pipes_message_without_shell() {
        // `cat` stands in for a sendmail binary: it reads stdin and exits 0.
        let dir = std::env::temp_dir().join(format!("auth-email-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmpdir");
        let out_path = dir.join("captured.txt");
        let sender = SendmailEmailSender::with_command(
            "sh".into(),
            vec!["-c".into(), format!("cat > {}", out_path.display())],
        );
        let outcome = sender
            .send(EmailMessage {
                to: "user@example.com".into(),
                subject: "Reset".into(),
                text_body: "link-with-token".into(),
            })
            .await
            .expect("sendmail send");
        assert_eq!(outcome, SendOutcome::Sent);
        let captured = std::fs::read_to_string(&out_path).expect("captured");
        assert!(captured.contains("To: user@example.com"));
        assert!(captured.contains("link-with-token"));
        assert!(captured.starts_with("To: "));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn sendmail_fails_closed_on_nonzero_exit() {
        let sender = SendmailEmailSender::with_command("false".into(), vec![]);
        let err = sender
            .send(EmailMessage {
                to: "user@example.com".into(),
                subject: "s".into(),
                text_body: "b".into(),
            })
            .await
            .expect_err("nonzero exit must fail");
        assert!(matches!(
            err,
            EmailError::SendFailed {
                mechanism: "sendmail",
                ..
            }
        ));
    }

    #[test]
    fn header_injection_is_stripped() {
        let rendered = SendmailEmailSender::render_rfc5322(&EmailMessage {
            to: "a@b.co".into(),
            subject: "s\r\nBcc: attacker@evil.co".into(),
            text_body: "body".into(),
        });
        // The injected CRLF becomes a space, so "Bcc: ..." stays literal text
        let header_lines: Vec<&str> = rendered
            .split("\r\n")
            .take_while(|line| !line.is_empty())
            .collect();
        assert_eq!(
            header_lines,
            vec![
                "To: a@b.co",
                "Subject: s  Bcc: attacker@evil.co",
                "Content-Type: text/plain; charset=utf-8",
                "MIME-Version: 1.0",
            ],
            "exactly four headers; injected Bcc is subject text: {rendered:?}"
        );
    }

    #[test]
    fn redaction_masks_local_part_only() {
        assert_eq!(redact_email("user@example.com"), "u***@example.com");
        assert_eq!(redact_email("a@b.co"), "a***@b.co");
        assert_eq!(redact_email("@nodomain"), "***");
        assert_eq!(redact_email("nolocal@"), "***");
        assert_eq!(redact_email(""), "***");
    }
}
