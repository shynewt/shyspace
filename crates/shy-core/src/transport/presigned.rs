//! An invitee's only way into the bucket: the two presigned links in their
//! invite code, used over plain HTTPS.

use anyhow::{Context, Result, ensure};
use reqwest::StatusCode;

use crate::membership::invite::{Invite, JoinMailbox};

/// The largest answer we accept. It holds a Welcome and the key history.
const MAX_ANSWER_SIZE: usize = 32 * 1024 * 1024;

pub struct PresignedMailbox {
    http: reqwest::Client,
    request_url: String,
    answer_url: String,
}

impl PresignedMailbox {
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

impl JoinMailbox for PresignedMailbox {
    async fn post_request(&self, request: Vec<u8>) -> Result<()> {
        let response = self
            .http
            .put(&self.request_url)
            .body(request)
            .send()
            .await
            .context("can't reach the workspace's storage")?;
        ensure!(
            response.status().is_success(),
            "the workspace's storage refused the join request ({}); the invite may have expired",
            response.status()
        );
        Ok(())
    }

    async fn fetch_answer(&self) -> Result<Option<Vec<u8>>> {
        let response = self
            .http
            .get(&self.answer_url)
            .send()
            .await
            .context("can't reach the workspace's storage")?;
        match response.status() {
            StatusCode::NOT_FOUND => Ok(None),
            status if status.is_success() => read_capped(response).await.map(Some),
            status => anyhow::bail!("the workspace's storage refused to send the answer ({status})"),
        }
    }
}
