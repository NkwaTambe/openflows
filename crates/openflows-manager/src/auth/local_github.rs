//! GitHub App user authorization for a locally running Manager.
//!
//! Device authorization and renewal use only the public App client ID. The
//! App private key and client secret never enter this client.

use crate::error::ManagerError;
use serde::Deserialize;
use std::time::Duration;

const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

#[derive(Deserialize)]
pub struct DeviceChallenge {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

impl std::fmt::Debug for DeviceChallenge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeviceChallenge([REDACTED])")
    }
}

#[derive(Deserialize)]
pub struct LocalUserToken {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub refresh_token_expires_in: u64,
}

impl std::fmt::Debug for LocalUserToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LocalUserToken([REDACTED])")
    }
}

#[derive(Debug, Deserialize)]
struct TokenReply {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    refresh_token_expires_in: Option<u64>,
    error: Option<String>,
    interval: Option<u64>,
}

impl TokenReply {
    fn into_token(self) -> Result<LocalUserToken, ManagerError> {
        Ok(LocalUserToken {
            access_token: self.access_token.ok_or_else(auth_failed)?,
            refresh_token: self.refresh_token.ok_or_else(auth_failed)?,
            expires_in: self.expires_in.ok_or_else(auth_failed)?,
            refresh_token_expires_in: self.refresh_token_expires_in.ok_or_else(auth_failed)?,
        })
    }
}

fn auth_failed() -> ManagerError {
    ManagerError::api("GITHUB_REAUTH_REQUIRED", "GitHub authorization is required")
}

pub struct LocalGithubAuth {
    client: reqwest::Client,
    client_id: String,
    device_url: String,
    token_url: String,
}

impl LocalGithubAuth {
    pub fn new(client_id: String) -> Result<Self, ManagerError> {
        if client_id.trim().is_empty() {
            return Err(ManagerError::Config(
                "GitHub App client ID is required".into(),
            ));
        }
        let client = reqwest::Client::builder()
            .user_agent("openflows-manager")
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ManagerError::Config("cannot initialize GitHub client".into()))?;
        Ok(Self {
            client,
            client_id,
            device_url: DEVICE_CODE_URL.into(),
            token_url: TOKEN_URL.into(),
        })
    }

    pub async fn start(&self) -> Result<DeviceChallenge, ManagerError> {
        let response = self
            .client
            .post(&self.device_url)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("scope", "offline_access"),
            ])
            .send()
            .await
            .map_err(|_| auth_failed())?;
        if !response.status().is_success() {
            return Err(auth_failed());
        }
        response.json().await.map_err(|_| auth_failed())
    }

    /// Poll once. The caller must wait at least `challenge.interval` seconds
    /// between calls, adding five seconds after a `slow_down` response.
    pub async fn poll(&self, challenge: &DeviceChallenge) -> Result<PollResult, ManagerError> {
        let response = self
            .client
            .post(&self.token_url)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("device_code", challenge.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await
            .map_err(|_| auth_failed())?;
        if !response.status().is_success() {
            return Err(auth_failed());
        }
        let reply: TokenReply = response.json().await.map_err(|_| auth_failed())?;
        match reply.error.as_deref() {
            Some("authorization_pending") => Ok(PollResult::Pending),
            Some("slow_down") => Ok(PollResult::SlowDown(reply.interval)),
            Some(_) => Err(auth_failed()),
            None => Ok(PollResult::Authorized(reply.into_token()?)),
        }
    }

    /// Wait for browser approval without exceeding GitHub's polling rate or
    /// the device-code lifetime.
    pub async fn await_authorization(
        &self,
        challenge: &DeviceChallenge,
    ) -> Result<LocalUserToken, ManagerError> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(challenge.expires_in);
        let mut interval = challenge.interval.max(1);
        loop {
            tokio::time::sleep(Duration::from_secs(interval)).await;
            if tokio::time::Instant::now() >= deadline {
                return Err(auth_failed());
            }
            match self.poll(challenge).await? {
                PollResult::Authorized(token) => return Ok(token),
                PollResult::Pending => {}
                PollResult::SlowDown(next) => {
                    interval = next
                        .unwrap_or(interval.saturating_add(5))
                        .max(interval.saturating_add(5));
                }
            }
        }
    }

    /// A device-flow refresh requires the public client ID, not the App secret.
    /// GitHub rotates both tokens; callers must persist the pair atomically.
    pub async fn refresh(&self, refresh_token: &str) -> Result<LocalUserToken, ManagerError> {
        if refresh_token.is_empty() {
            return Err(auth_failed());
        }
        let response = self
            .client
            .post(&self.token_url)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await
            .map_err(|_| auth_failed())?;
        if !response.status().is_success() {
            return Err(auth_failed());
        }
        let reply: TokenReply = response.json().await.map_err(|_| auth_failed())?;
        if reply.error.is_some() {
            return Err(auth_failed());
        }
        reply.into_token()
    }
}

#[derive(Debug)]
pub enum PollResult {
    Pending,
    SlowDown(Option<u64>),
    Authorized(LocalUserToken),
}
