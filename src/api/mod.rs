use anyhow::{anyhow, Context, Result};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::keychain::Session;

#[derive(Clone)]
pub struct ApiClient {
    base_url: String,
    http: reqwest::Client,
}

impl ApiClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    pub async fn get<R: DeserializeOwned>(
        &self,
        path: &str,
        session: Option<&Session>,
    ) -> Result<R> {
        let mut request = self
            .http
            .get(format!("{}{}", self.base_url, path))
            .header(ACCEPT, "application/json");
        if let Some(session) = session {
            request = request.header(AUTHORIZATION, format!("Bearer {}", session.session_token));
        }
        decode(request.send().await?).await
    }

    pub async fn post<T: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
        session: Option<&Session>,
    ) -> Result<R> {
        let mut request = self
            .http
            .post(format!("{}{}", self.base_url, path))
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .json(body);
        if let Some(session) = session {
            request = request.header(AUTHORIZATION, format!("Bearer {}", session.session_token));
        }
        decode(request.send().await?).await
    }

    pub async fn delete<R: DeserializeOwned>(
        &self,
        path: &str,
        session: Option<&Session>,
    ) -> Result<R> {
        let mut request = self
            .http
            .delete(format!("{}{}", self.base_url, path))
            .header(ACCEPT, "application/json");
        if let Some(session) = session {
            request = request.header(AUTHORIZATION, format!("Bearer {}", session.session_token));
        }
        decode(request.send().await?).await
    }
}

async fn decode<R: DeserializeOwned>(response: reqwest::Response) -> Result<R> {
    let status = response.status();
    let text = response
        .text()
        .await
        .context("failed to read API response")?;
    if !status.is_success() {
        if let Ok(error) = serde_json::from_str::<ApiError>(&text) {
            return Err(anyhow!("{}", error.message));
        }
        return Err(anyhow!(
            "API request failed with status {}: {}",
            status,
            text
        ));
    }
    serde_json::from_str(&text).with_context(|| format!("failed to decode API response: {}", text))
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
}

#[derive(Debug, Deserialize)]
pub struct DeviceStartResponse {
    pub cli_pairing_code: String,
    pub device_code: String,
    pub device_id: String,
}

#[derive(Debug, Deserialize)]
pub struct EmailStartResponse {
    pub request_id: String,
    pub masked_email: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct EmailVerifyResponse {
    pub user: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct DevicePollResponse {
    pub status: String,
    pub user: Option<String>,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub session_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct InstallResponse {
    pub install_url: String,
    pub org: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OrgListResponse {
    pub selected_org: Option<String>,
    pub orgs: Vec<Org>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Org {
    pub org: String,
    pub account_login: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AgentListResponse {
    pub org: String,
    pub agents: Vec<Agent>,
}

#[derive(Debug, Deserialize)]
pub struct Agent {
    pub name: String,
    #[serde(rename = "display_name", alias = "display")]
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
pub struct PermissionListResponse {
    pub permissions: Vec<Permission>,
}

#[derive(Debug, Deserialize)]
pub struct Permission {
    pub agent: String,
    pub repos: Vec<String>,
    pub branches: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct MeResponse {
    pub email: String,
    pub device_id: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct DeleteAgentResponse {
    pub org: String,
    pub agent: String,
    pub display: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct DisconnectOrgResponse {
    pub org: String,
    pub github_removed: bool,
    pub manage_url: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct DeleteAccountResponse {
    pub deleted: bool,
}

#[derive(Debug, Deserialize)]
pub struct GitHubTokenResponse {
    pub username: String,
    pub token: String,
}
