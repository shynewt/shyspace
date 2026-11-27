//! An invitee's only way into the bucket: the two presigned links in their
//! invite code, used over plain HTTPS.

use anyhow::{Context, Result, ensure};
use reqwest::StatusCode;
use tracing::warn;

use crate::membership::invite::{Invite, JoinMailbox};

/// The largest answer we accept. It holds a Welcome and the key history.
const MAX_ANSWER_SIZE: usize = 32 * 1024 * 1024;
const MAX_ERROR_BODY: usize = 16 * 1024;
/// A 403 on a link that worked when the invite was made: almost always its
/// signing key was deleted after the admin changed the storage key.
const INVITE_REFUSED: &str = "This invite no longer works, probably because the workspace's storage key changed. Ask the admin for a new one.";
/// A 403 for a link past its lifetime, which can come a moment before the
/// invite's own expiry, as the links are signed first.
const INVITE_EXPIRED: &str = "This invite has expired. Ask the admin for a new one.";

pub struct PresignedMailbox {
    http: reqwest::Client,
    request_url: String,
    answer_url: String,
}

impl PresignedMailbox {
    pub fn for_link(code: &crate::membership::link::LinkCode) -> Self {
        Self {
            http: reqwest::Client::new(),
            request_url: code.request_url.clone(),
            answer_url: code.answer_url.clone(),
        }
    }

    pub fn for_invite(invite: &Invite) -> Self {
        Self {
            http: reqwest::Client::new(),
            request_url: invite.request_url.clone(),
            answer_url: invite.answer_url.clone(),
        }
    }
}

/// Reads a body without ever holding more than `MAX_ANSWER_SIZE` bytes.
async fn read_capped(mut response: reqwest::Response) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            body.len() + chunk.len() <= MAX_ANSWER_SIZE,
            "the admin's answer is too large"
        );
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Why the provider refused a link, as the joiner should read it: what to
/// do for a 403, the provider's reason after `what` otherwise.
async fn refused(response: reqwest::Response, what: &str) -> anyhow::Error {
    let status = response.status();
    let reason = refusal(response).await;
    if status != StatusCode::FORBIDDEN {
        return anyhow::anyhow!("{what}: {reason}");
    }
    warn!("the storage refused an invite link: {reason}");
    if reason.to_lowercase().contains("expired") {
        return anyhow::anyhow!(INVITE_EXPIRED);
    }
    anyhow::anyhow!(INVITE_REFUSED)
}

/// The provider's reason for refusing, like "403 Forbidden: Request has
/// expired" or "403 Forbidden: not entitled" (the bucket key can't write).
async fn refusal(response: reqwest::Response) -> String {
    let status = response.status();
    let body = read_up_to(response, MAX_ERROR_BODY).await.unwrap_or_default();
    let body = String::from_utf8_lossy(&body);
    match xml_tag(&body, "Message").or_else(|| xml_tag(&body, "Code")) {
        Some(reason) => format!("{status}: {}", printable(reason)),
        None => status.to_string(),
    }
}

/// The most of a provider's reason kept, for logs and errors.
const MAX_REASON_CHARS: usize = 200;

/// `text` from a server nobody vouches for, cut short and with control
/// characters (newlines included) and invisible direction or formatting
/// marks escaped, so it can't forge or disguise log lines.
fn printable(text: &str) -> String {
    let mut printable = String::new();
    for (index, character) in text.chars().enumerate() {
        if index == MAX_REASON_CHARS {
            printable.push('…');
            break;
        }
        match character.is_control() || is_invisible_mark(character) {
            true => printable.extend(character.escape_unicode()),
            false => printable.push(character),
        }
    }
    printable
}

/// Zero-width and direction marks, which change how text around them reads.
fn is_invisible_mark(character: char) -> bool {
    matches!(character, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}')
}

/// Reads at most `limit` bytes of a body: the address came from an invite
/// code, so the server on the other end isn't trusted.
async fn read_up_to(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        let room = limit.saturating_sub(body.len());
        body.extend_from_slice(&chunk[..chunk.len().min(room)]);
        if body.len() >= limit {
            break;
        }
    }
    Ok(body)
}

/// The text of the first `<tag>` in an S3 error document.
fn xml_tag<'a>(body: &'a str, tag: &str) -> Option<&'a str> {
    let start = body.find(&format!("<{tag}>"))? + tag.len() + 2;
    let end = start + body[start..].find(&format!("</{tag}>"))?;
    Some(body[start..end].trim()).filter(|text| !text.is_empty())
}

impl JoinMailbox for PresignedMailbox {
    async fn post_request(&self, request: Vec<u8>) -> Result<()> {
        let response = self
            .http
            .put(&self.request_url)
            .body(request)
            .send()
            .await
            // The link is a secret, and the error would quote it.
            .map_err(reqwest::Error::without_url)
            .context("can't reach the workspace's storage")?;
        if !response.status().is_success() {
            return Err(refused(response, "the workspace's storage refused the join request").await);
        }
        Ok(())
    }

    async fn fetch_answer(&self) -> Result<Option<Vec<u8>>> {
        let response = self
            .http
            .get(&self.answer_url)
            .send()
            .await
            // The link is a secret, and the error would quote it.
            .map_err(reqwest::Error::without_url)
            .context("can't reach the workspace's storage")?;
        match response.status() {
            StatusCode::NOT_FOUND => Ok(None),
            status if status.is_success() => read_capped(response).await.map(Some),
            _ => Err(refused(response, "the workspace's storage refused to send the answer").await),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DELETED_KEY: &str =
        "<Code>InvalidAccessKeyId</Code><Message>The access key does not exist.</Message>";

    /// A server answering every request with `status` and an S3 error with
    /// `error` inside, as a provider does; returns its address.
    async fn answering(status: &'static str, error: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 8192];
                let _ = socket.read(&mut request).await;
                let body = format!("<?xml version=\"1.0\"?><Error>{error}</Error>");
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        format!("http://{address}")
    }

    fn mailbox_at(server: &str) -> PresignedMailbox {
        PresignedMailbox {
            http: reqwest::Client::new(),
            request_url: format!("{server}/request"),
            answer_url: format!("{server}/answer"),
        }
    }

    #[tokio::test]
    async fn links_refused_after_a_key_change_say_to_ask_for_a_new_invite() {
        let mailbox = mailbox_at(&answering("403 Forbidden", DELETED_KEY).await);

        let posting = mailbox.post_request(b"request".to_vec()).await.unwrap_err();
        let fetching = mailbox.fetch_answer().await.unwrap_err();

        for error in [posting, fetching] {
            assert_eq!(error.to_string(), INVITE_REFUSED);
        }
    }

    #[tokio::test]
    async fn an_expired_link_says_the_invite_expired() {
        let expired = "<Code>AccessDenied</Code><Message>Request has expired</Message>";
        let mailbox = mailbox_at(&answering("403 Forbidden", expired).await);

        let error = mailbox.fetch_answer().await.unwrap_err();

        assert_eq!(error.to_string(), INVITE_EXPIRED);
    }

    #[tokio::test]
    async fn an_unreachable_link_isnt_quoted_in_the_error() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let closed = listener.local_addr().unwrap();
        drop(listener);
        let mailbox = mailbox_at(&format!(
            "http://{closed}/invites?X-Amz-Signature=secret-signature"
        ));

        let error = mailbox.fetch_answer().await.unwrap_err();

        assert!(!format!("{error:#}").contains("secret-signature"), "{error:#}");
    }

    #[tokio::test]
    async fn other_refusals_keep_the_providers_reason() {
        let mailbox = mailbox_at(&answering("500 Internal Server Error", DELETED_KEY).await);

        let posting = mailbox.post_request(b"request".to_vec()).await.unwrap_err();
        let fetching = mailbox.fetch_answer().await.unwrap_err();

        assert!(
            posting
                .to_string()
                .starts_with("the workspace's storage refused the join request: 500"),
            "{posting}"
        );
        assert!(
            fetching
                .to_string()
                .starts_with("the workspace's storage refused to send the answer: 500"),
            "{fetching}"
        );
    }

    #[test]
    fn a_providers_reason_cant_forge_log_lines() {
        let forged = "denied\n2026-11-25 INFO everything is fine\r\u{1b}[2J\u{202E}desrever";
        let shown = printable(forged);
        assert!(
            !shown
                .chars()
                .any(|character| character.is_control() || is_invisible_mark(character)),
            "{shown:?}"
        );
        assert!(shown.contains("\\u{a}2026"), "{shown:?}");
        let long = "x".repeat(10_000);
        assert_eq!(printable(&long).chars().count(), MAX_REASON_CHARS + 1);
    }

    #[test]
    fn reads_the_reason_from_an_s3_error() {
        let body =
            "<?xml version=\"1.0\"?><Error><Code>AccessDenied</Code><Message>not entitled</Message></Error>";
        assert_eq!(xml_tag(body, "Message"), Some("not entitled"));
        assert_eq!(xml_tag(body, "Code"), Some("AccessDenied"));
        assert_eq!(xml_tag("plain text", "Message"), None);
    }
}
