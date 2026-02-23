use reqwest::Client;

use crate::{
    error::AppError,
    models::GitHubUser,
};

#[derive(Clone)]
pub struct GitHubClient {
    pub http: Client,
}

impl GitHubClient {
    pub fn new() -> Result<Self, AppError> {
        let http = Client::builder()
            .user_agent("collaborator-audit-dashboard")
            .build()
            .map_err(|e| AppError::Config(format!("failed to build HTTP client: {e}")))?;

        Ok(Self { http })
    }

    pub async fn fetch_authenticated_user(&self, token: &str) -> Result<GitHubUser, AppError> {
        let response = self
            .http
            .get("https://api.github.com/user")
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .bearer_auth(token)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(AppError::Auth);
        }

        let user = response.json::<GitHubUser>().await?;
        Ok(user)
    }
}
