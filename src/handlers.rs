use std::collections::{HashMap, HashSet};

use askama::Template;
use axum::{
    Json,
    extract::{Extension, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
};
use axum_extra::extract::PrivateCookieJar;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use url::Url;

use crate::{
    AppState, auth,
    error::AppError,
    models::{
        DashboardQuery, GitHubAccessTokenResponse, OAuthCallbackQuery, RemoveFailure, RemoveRequest,
        RemoveResponse, RemoveSuccess, SessionData,
    },
    utils,
};

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    rows: Vec<DashboardRow>,
    csrf_token: String,
    ignore_forks: bool,
    ignore_archived: bool,
}

#[derive(Debug, Clone, Serialize)]
struct DashboardRow {
    repo: String,
    collaborator: String,
    permission: String,
    can_remove: bool,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct OAuthTokenExchangeRequest<'a> {
    client_id: &'a str,
    client_secret: &'a str,
    code: &'a str,
    redirect_uri: &'a str,
    state: &'a str,
}

pub async fn index(jar: PrivateCookieJar) -> impl IntoResponse {
    match auth::read_session(&jar) {
        Ok(Some(_)) => Redirect::to("/dashboard").into_response(),
        Ok(None) => Html("<h1>GitHub Collaborator Dashboard</h1><p><a href=\"/auth/login\">Log in with GitHub</a></p>").into_response(),
        Err(_) => Html("<h1>GitHub Collaborator Dashboard</h1><p><a href=\"/auth/login\">Log in with GitHub</a></p>").into_response(),
    }
}

pub async fn health() -> impl IntoResponse {
    "ok"
}

pub async fn auth_login(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
) -> Result<(PrivateCookieJar, Redirect), AppError> {
    if auth::read_session(&jar)?.is_some() {
        return Ok((jar, Redirect::to("/dashboard")));
    }

    let oauth_state = utils::random_token(32);
    let secure_cookie = state.config.base_url.scheme() == "https";
    let jar = auth::set_oauth_state(jar, &oauth_state, secure_cookie);

    let redirect_uri = state
        .config
        .base_url
        .join("auth/callback")
        .map_err(|e| AppError::Config(format!("invalid callback URL: {e}")))?;

    let authorization_url = Url::parse_with_params(
        "https://github.com/login/oauth/authorize",
        &[
            ("client_id", state.config.github_client_id.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("scope", "repo read:org"),
            ("state", oauth_state.as_str()),
        ],
    )
    .map_err(|_e| AppError::Internal)?;

    info!("starting github oauth flow");
    Ok((jar, Redirect::to(authorization_url.as_str())))
}

pub async fn auth_callback(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<(PrivateCookieJar, Redirect), AppError> {
    let secure_cookie = state.config.base_url.scheme() == "https";
    let Some(expected_state) = auth::read_oauth_state(&jar) else {
        return Err(AppError::Auth);
    };

    if expected_state != query.state {
        error!("oauth state mismatch");
        return Err(AppError::Auth);
    }

    let redirect_uri = state
        .config
        .base_url
        .join("auth/callback")
        .map_err(|e| AppError::Config(format!("invalid callback URL: {e}")))?;

    let token_payload = OAuthTokenExchangeRequest {
        client_id: &state.config.github_client_id,
        client_secret: &state.config.github_client_secret,
        code: &query.code,
        redirect_uri: redirect_uri.as_str(),
        state: &query.state,
    };

    let token_response = state
        .github
        .http
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .form(&token_payload)
        .send()
        .await?;

    if !token_response.status().is_success() {
        error!(status = %token_response.status(), "oauth token exchange failed");
        return Err(AppError::Auth);
    }

    let token = token_response.json::<GitHubAccessTokenResponse>().await?;

    if !token.token_type.eq_ignore_ascii_case("bearer") {
        return Err(AppError::Auth);
    }

    if !auth::has_required_scopes(&token.scope) {
        return Err(AppError::BadRequest(
            "OAuth scopes are insufficient. Required scopes: repo, read:org".to_string(),
        ));
    }

    let user = state
        .github
        .fetch_authenticated_user(&token.access_token)
        .await?;

    let session = SessionData {
        access_token: token.access_token,
        user_login: user.login,
        csrf_token: utils::random_token(32),
    };

    let jar = auth::write_session(jar, &session, secure_cookie)?;
    let jar = auth::clear_oauth_state(jar, secure_cookie);

    info!("github oauth completed successfully");
    Ok((jar, Redirect::to("/dashboard")))
}

pub async fn dashboard(
    State(state): State<AppState>,
    Extension(session): Extension<SessionData>,
    Query(query): Query<DashboardQuery>,
) -> Result<Html<String>, AppError> {
    let ignore_forks = query.ignore_forks;
    let ignore_archived = query.ignore_archived;

    let data = state
        .github
        .fetch_repos_with_collaborators(
            &session.access_token,
            &session.user_login,
            query.into(),
            state.config.max_concurrency,
        )
        .await?;

    let rows = data
        .into_iter()
        .flat_map(|repo_row| {
            let repo_name = repo_row.repo.name;
            let can_remove = repo_row.can_remove;
            repo_row.collaborators.into_iter().map(move |c| {
                let permission = c.permission_label().to_string();
                DashboardRow {
                    repo: repo_name.clone(),
                    collaborator: c.login,
                    permission,
                    can_remove,
                }
            })
        })
        .collect::<Vec<_>>();

    let template = DashboardTemplate {
        rows,
        csrf_token: session.csrf_token,
        ignore_forks,
        ignore_archived,
    };
    let rendered = template.render()?;
    Ok(Html(rendered))
}

pub async fn logout(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
) -> Result<(PrivateCookieJar, Redirect), AppError> {
    let secure_cookie = state.config.base_url.scheme() == "https";
    let jar = auth::clear_session(jar, secure_cookie);
    Ok((jar, Redirect::to("/")))
}

pub async fn remove_collaborators(
    State(state): State<AppState>,
    Extension(session): Extension<SessionData>,
    Json(payload): Json<RemoveRequest>,
) -> Result<(StatusCode, Json<RemoveResponse>), AppError> {
    if payload.items.is_empty() {
        return Err(AppError::BadRequest("items must not be empty".to_string()));
    }

    let mut success = Vec::new();
    let mut failed = Vec::new();

    let mut repos_seen = HashSet::new();
    for item in &payload.items {
        if item.repo.trim().is_empty() || item.username.trim().is_empty() {
            failed.push(RemoveFailure {
                repo: item.repo.clone(),
                username: item.username.clone(),
                reason: "repo and username must be non-empty".to_string(),
            });
            continue;
        }
        repos_seen.insert(item.repo.clone());
    }

    let mut ownership_cache: HashMap<String, bool> = HashMap::new();
    let mut admin_cache: HashMap<String, bool> = HashMap::new();

    for repo in repos_seen {
        let owned = match state
            .github
            .repo_exists_for_owner(&session.access_token, &session.user_login, &repo)
            .await
        {
            Ok(value) => value,
            Err(err) => {
                warn!(repo, error = %err, "ownership validation failed");
                false
            }
        };
        ownership_cache.insert(repo.clone(), owned);

        let is_admin = if owned {
            match state
                .github
                .fetch_effective_permission(
                    &session.access_token,
                    &session.user_login,
                    &repo,
                    &session.user_login,
                )
                .await
            {
                Ok(Some(permission)) => permission.permission.eq_ignore_ascii_case("admin"),
                Ok(None) => false,
                Err(err) => {
                    warn!(repo, error = %err, "admin check failed");
                    false
                }
            }
        } else {
            false
        };

        admin_cache.insert(repo, is_admin);
    }

    for item in payload.items {
        if item.username == session.user_login {
            failed.push(RemoveFailure {
                repo: item.repo,
                username: item.username,
                reason: "cannot remove authenticated user".to_string(),
            });
            continue;
        }

        if !ownership_cache.get(&item.repo).copied().unwrap_or(false) {
            failed.push(RemoveFailure {
                repo: item.repo,
                username: item.username,
                reason: "repository is not owned by authenticated user".to_string(),
            });
            continue;
        }

        if !admin_cache.get(&item.repo).copied().unwrap_or(false) {
            failed.push(RemoveFailure {
                repo: item.repo,
                username: item.username,
                reason: "authenticated user does not have admin permission".to_string(),
            });
            continue;
        }

        info!(repo = item.repo, username = item.username, "attempting collaborator deletion");
        let status = match state
            .github
            .remove_collaborator(
                &session.access_token,
                &session.user_login,
                &item.repo,
                &item.username,
            )
            .await
        {
            Ok(status) => status,
            Err(err) => {
                warn!(repo = item.repo, username = item.username, error = %err, "collaborator deletion request failed");
                failed.push(RemoveFailure {
                    repo: item.repo,
                    username: item.username,
                    reason: "upstream request failed".to_string(),
                });
                continue;
            }
        };

        match status {
            StatusCode::NO_CONTENT => success.push(RemoveSuccess {
                repo: item.repo,
                username: item.username,
            }),
            StatusCode::FORBIDDEN => failed.push(RemoveFailure {
                repo: item.repo,
                username: item.username,
                reason: "insufficient permissions".to_string(),
            }),
            StatusCode::UNPROCESSABLE_ENTITY => failed.push(RemoveFailure {
                repo: item.repo,
                username: item.username,
                reason: "validation failed or abuse detection triggered".to_string(),
            }),
            StatusCode::NOT_FOUND => failed.push(RemoveFailure {
                repo: item.repo,
                username: item.username,
                reason: "collaborator not found".to_string(),
            }),
            other => failed.push(RemoveFailure {
                repo: item.repo,
                username: item.username,
                reason: format!("unexpected response status: {other}"),
            }),
        }
    }

    Ok((
        StatusCode::OK,
        Json(RemoveResponse { success, failed }),
    ))
}
