use axum::{
    extract::State,
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::PrivateCookieJar;

use crate::{AppState, auth, models::SessionData};

pub async fn require_auth(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = request.uri().path().to_owned();

    match auth::read_session(&jar) {
        Ok(Some(session)) => run_authenticated(next, request, session).await,
        _ => unauthenticated_response(
            path.as_str(),
            jar,
            state.config.base_url.scheme() == "https",
        ),
    }
}

pub async fn csrf_protect(
    jar: PrivateCookieJar,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let method = request.method().clone();

    if !requires_csrf(&method) {
        return next.run(request).await;
    }

    let header_token = request
        .headers()
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .map(str::trim);

    let session = match auth::read_session(&jar) {
        Ok(Some(session)) => session,
        _ => return (StatusCode::UNAUTHORIZED, "authentication required").into_response(),
    };

    match header_token {
        Some(token) if token == session.csrf_token => next.run(request).await,
        _ => (StatusCode::FORBIDDEN, "invalid csrf token").into_response(),
    }
}

async fn run_authenticated(
    next: Next,
    mut request: Request<axum::body::Body>,
    session: SessionData,
) -> Response {
    request.extensions_mut().insert(session);
    next.run(request).await
}

fn unauthenticated_response(path: &str, jar: PrivateCookieJar, secure: bool) -> Response {
    let cleared = auth::clear_session(jar, secure);

    if path.starts_with("/remove") {
        return (
            cleared,
            (StatusCode::UNAUTHORIZED, "authentication required"),
        )
            .into_response();
    }

    (cleared, Redirect::to("/auth/login")).into_response()
}

fn requires_csrf(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}
