use std::{sync::Arc, time::Duration};

use futures::{StreamExt, stream};
use reqwest::{Client, RequestBuilder, Response, StatusCode};
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
                    Ok(Some(permission)) => is_admin_permission(&permission),
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

            if response.status() != StatusCode::TOO_MANY_REQUESTS {
                return Ok(response);
            }

            let backoff_ms = 250u64.saturating_mul(2u64.saturating_pow(u32::from(attempt - 1)));
            warn!(attempt, backoff_ms, "rate limit hit, backing off");
            sleep(Duration::from_millis(backoff_ms)).await;
        }

        Err(AppError::Upstream(
            "request failed repeatedly due to rate limiting".to_string(),
        ))
    }
}

fn is_admin_permission(permission: &CollaboratorPermission) -> bool {
    permission.permission.eq_ignore_ascii_case("admin")
        || permission
            .role_name
            .as_ref()
            .is_some_and(|value| value.eq_ignore_ascii_case("admin"))
}
