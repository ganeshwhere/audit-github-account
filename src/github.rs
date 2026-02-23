use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures::{StreamExt, stream};
use reqwest::{
    Client, RequestBuilder, Response, StatusCode,
    header::{HeaderMap, RETRY_AFTER},
};
use tokio::{sync::Semaphore, time::sleep};
use tracing::{info, warn};

use crate::{
    error::AppError,
    models::{
        Collaborator, CollaboratorPermission, GitHubUser, RepoFilterOptions, RepoWithCollaborators,
        Repository,
    },
    utils,
};

#[derive(Clone)]
pub struct GitHubClient {
    pub http: Client,
}

#[derive(Debug)]
pub enum CollaboratorFetchOutcome {
    Success(Vec<Collaborator>),
    Forbidden,
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

    pub async fn fetch_owned_repos(
        &self,
        token: &str,
        options: &RepoFilterOptions,
    ) -> Result<Vec<Repository>, AppError> {
        let mut next_url = Some(
            "https://api.github.com/user/repos?affiliation=owner&per_page=100&page=1".to_string(),
        );
        let mut repositories = Vec::new();

        while let Some(url) = next_url {
            let response = self
                .send_with_retry(|| self.authorized_request(self.http.get(url.clone()), token))
                .await?;

            if !response.status().is_success() {
                return Err(AppError::Upstream(format!(
                    "failed to fetch repositories: {}",
                    response.status()
                )));
            }

            let next_link = utils::parse_next_link(
                response.headers().get("link").and_then(|v| v.to_str().ok()),
            );

            let page_repos = response.json::<Vec<Repository>>().await?;
            if page_repos.is_empty() {
                break;
            }

            repositories.extend(page_repos.into_iter().filter(|repo| {
                !(options.ignore_forks && repo.fork || options.ignore_archived && repo.archived)
            }));

            next_url = next_link;
        }

        Ok(repositories)
    }

    pub async fn fetch_repo_collaborators(
        &self,
        token: &str,
        owner: &str,
        repo: &str,
    ) -> Result<CollaboratorFetchOutcome, AppError> {
        let mut next_url = Some(format!(
            "https://api.github.com/repos/{owner}/{repo}/collaborators?per_page=100&page=1"
        ));
        let mut collaborators = Vec::new();

        while let Some(url) = next_url {
            let response = self
                .send_with_retry(|| self.authorized_request(self.http.get(url.clone()), token))
                .await?;

            if response.status() == StatusCode::FORBIDDEN {
                warn!(
                    owner,
                    repo, "insufficient permissions while fetching collaborators"
                );
                return Ok(CollaboratorFetchOutcome::Forbidden);
            }

            if !response.status().is_success() {
                return Err(AppError::Upstream(format!(
                    "failed to fetch collaborators for {owner}/{repo}: {}",
                    response.status()
                )));
            }

            let next_link = utils::parse_next_link(
                response.headers().get("link").and_then(|v| v.to_str().ok()),
            );

            let page_collaborators = response.json::<Vec<Collaborator>>().await?;
            if page_collaborators.is_empty() {
                break;
            }

            collaborators.extend(page_collaborators);
            next_url = next_link;
        }

        Ok(CollaboratorFetchOutcome::Success(collaborators))
    }

    pub async fn fetch_repos_with_collaborators(
        &self,
        token: &str,
        viewer: &str,
        options: RepoFilterOptions,
        max_concurrency: usize,
    ) -> Result<Vec<RepoWithCollaborators>, AppError> {
        let repos = self.fetch_owned_repos(token, &options).await?;
        let semaphore = Arc::new(Semaphore::new(max_concurrency));
        let client = self.clone();
        let viewer_login = viewer.to_string();

        let rows = stream::iter(repos.into_iter().map(|repo| {
            let semaphore = semaphore.clone();
            let client = client.clone();
            let token = token.to_string();
            let viewer_login = viewer_login.clone();

            async move {
                let permit = semaphore
                    .acquire_owned()
                    .await
                    .map_err(|_| AppError::Internal)?;
                let owner = repo.owner.login.clone();
                let repo_name = repo.name.clone();

                let collaborators = match client
                    .fetch_repo_collaborators(&token, &owner, &repo_name)
                    .await?
                {
                    CollaboratorFetchOutcome::Success(c) => c,
                    CollaboratorFetchOutcome::Forbidden => {
                        drop(permit);
                        return Ok(None);
                    }
                };

                let filtered = collaborators
                    .into_iter()
                    .filter(|c| c.login != viewer_login)
                    .collect::<Vec<_>>();

                if filtered.is_empty() {
                    drop(permit);
                    return Ok(None);
                }

                let can_remove = match client
                    .fetch_effective_permission(&token, &owner, &repo_name, &viewer_login)
                    .await
                {
                    Ok(Some(permission)) => Self::is_admin_permission(&permission),
                    Ok(None) => false,
                    Err(err) => {
                        warn!(
                            owner,
                            repo = repo_name,
                            error = %err,
                            "permission check failed, disabling removal"
                        );
                        false
                    }
                };

                drop(permit);

                Ok(Some(RepoWithCollaborators {
                    repo,
                    collaborators: filtered,
                    can_remove,
                }))
            }
        }))
        .buffer_unordered(max_concurrency)
        .collect::<Vec<Result<Option<RepoWithCollaborators>, AppError>>>()
        .await;

        let mut output = Vec::new();
        for item in rows {
            if let Some(repo) = item? {
                output.push(repo);
            }
        }

        info!(
            repo_count = output.len(),
            "fetched repositories with collaborators"
        );
        Ok(output)
    }

    pub async fn fetch_effective_permission(
        &self,
        token: &str,
        owner: &str,
        repo: &str,
        username: &str,
    ) -> Result<Option<CollaboratorPermission>, AppError> {
        let endpoint = format!(
            "https://api.github.com/repos/{owner}/{repo}/collaborators/{username}/permission"
        );

        let response = self
            .send_with_retry(|| self.authorized_request(self.http.get(endpoint.clone()), token))
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(AppError::Upstream(format!(
                "permission check failed for {owner}/{repo}: {}",
                response.status()
            )));
        }

        Ok(Some(response.json::<CollaboratorPermission>().await?))
    }

    pub async fn repo_exists_for_owner(
        &self,
        token: &str,
        owner: &str,
        repo: &str,
    ) -> Result<bool, AppError> {
        let endpoint = format!("https://api.github.com/repos/{owner}/{repo}");
        let response = self
            .send_with_retry(|| self.authorized_request(self.http.get(endpoint.clone()), token))
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(false);
        }

        if !response.status().is_success() {
            return Err(AppError::Upstream(format!(
                "repository ownership check failed for {owner}/{repo}: {}",
                response.status()
            )));
        }

        Ok(true)
    }

    pub async fn remove_collaborator(
        &self,
        token: &str,
        owner: &str,
        repo: &str,
        username: &str,
    ) -> Result<StatusCode, AppError> {
        let endpoint =
            format!("https://api.github.com/repos/{owner}/{repo}/collaborators/{username}");

        let response = self
            .send_with_retry(|| self.authorized_request(self.http.delete(endpoint.clone()), token))
            .await?;

        Ok(response.status())
    }

    pub fn is_admin_permission(permission: &CollaboratorPermission) -> bool {
        permission.permission.eq_ignore_ascii_case("admin")
            || permission
                .role_name
                .as_ref()
                .is_some_and(|value| value.eq_ignore_ascii_case("admin"))
    }

    fn authorized_request(&self, request: RequestBuilder, token: &str) -> RequestBuilder {
        request
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .bearer_auth(token)
    }

    async fn send_with_retry<F>(&self, mut build: F) -> Result<Response, AppError>
    where
        F: FnMut() -> RequestBuilder,
    {
        let max_attempts = 5u8;

        for attempt in 1..=max_attempts {
            let response = build().send().await?;

            if let Some(backoff) = Self::rate_limit_backoff(response.status(), response.headers()) {
                let backoff_ms = backoff.as_millis() as u64;
                warn!(attempt, backoff_ms, "rate limit hit, backing off");
                sleep(backoff).await;
                continue;
            }

            if response.status() != StatusCode::TOO_MANY_REQUESTS
                && response.status() != StatusCode::FORBIDDEN
            {
                return Ok(response);
            }

            return Ok(response);
        }

        Err(AppError::Upstream(
            "request failed repeatedly due to rate limiting".to_string(),
        ))
    }

    fn rate_limit_backoff(status: StatusCode, headers: &HeaderMap) -> Option<Duration> {
        if status != StatusCode::TOO_MANY_REQUESTS
            && !(status == StatusCode::FORBIDDEN && Self::is_rate_limited(headers))
        {
            return None;
        }

        if let Some(delay) = Self::retry_after_delay(headers) {
            return Some(delay);
        }

        if let Some(delay) = Self::reset_time_delay(headers) {
            return Some(delay);
        }

        // GitHub recommends waiting at least one minute when secondary rate limiting
        // occurs without explicit timing headers.
        Some(Duration::from_secs(60))
    }

    fn is_rate_limited(headers: &HeaderMap) -> bool {
        headers
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == "0")
            || headers.contains_key(RETRY_AFTER)
    }

    fn retry_after_delay(headers: &HeaderMap) -> Option<Duration> {
        let retry_after = headers
            .get(RETRY_AFTER)?
            .to_str()
            .ok()?
            .parse::<u64>()
            .ok()?;
        Some(Duration::from_secs(retry_after.max(1)))
    }

    fn reset_time_delay(headers: &HeaderMap) -> Option<Duration> {
        let reset_at = headers
            .get("x-ratelimit-reset")?
            .to_str()
            .ok()?
            .parse::<u64>()
            .ok()?;

        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        let wait = if reset_at > now { reset_at - now } else { 1 };
        Some(Duration::from_secs(wait))
    }
}
