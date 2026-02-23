use askama::Template;
use axum::{
    Json,
    extract::{Query, State},
    response::{Html, IntoResponse, Redirect},
};
use axum_extra::extract::PrivateCookieJar;
use serde::Deserialize;
use tracing::{error, info};
use url::Url;

use crate::{
    AppState, auth,
    error::AppError,
    models::{GitHubAccessTokenResponse, OAuthCallbackQuery, SessionData},
    utils,
};

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate;

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
    .map_err(|e| AppError::Internal)?;

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

pub async fn dashboard() -> Result<Html<String>, AppError> {
    let template = DashboardTemplate;
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

pub async fn remove_placeholder() -> Json<serde_json::Value> {
    Json(serde_json::json!({"success": [], "failed": []}))
}
