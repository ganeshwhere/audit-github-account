use axum_extra::extract::{
    PrivateCookieJar,
    cookie::{Cookie, SameSite},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

use crate::{error::AppError, models::SessionData};

pub const SESSION_COOKIE: &str = "gh_session";
pub const OAUTH_STATE_COOKIE: &str = "gh_oauth_state";

pub fn read_session(jar: &PrivateCookieJar) -> Result<Option<SessionData>, AppError> {
    let Some(cookie) = jar.get(SESSION_COOKIE) else {
        return Ok(None);
    };

    let decoded = URL_SAFE_NO_PAD
        .decode(cookie.value())
        .map_err(|_| AppError::Auth)?;
    let session = serde_json::from_slice::<SessionData>(&decoded)?;
    Ok(Some(session))
}

pub fn write_session(
    jar: PrivateCookieJar,
    session: &SessionData,
    secure: bool,
) -> Result<PrivateCookieJar, AppError> {
    let json = serde_json::to_vec(session)?;
    let encoded = URL_SAFE_NO_PAD.encode(json);

    let cookie = Cookie::build((SESSION_COOKIE, encoded))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .build();

    Ok(jar.add(cookie))
}

pub fn clear_session(jar: PrivateCookieJar, secure: bool) -> PrivateCookieJar {
    let cookie = Cookie::build((SESSION_COOKIE, ""))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .build();
    jar.remove(cookie)
}

pub fn set_oauth_state(jar: PrivateCookieJar, state: &str, secure: bool) -> PrivateCookieJar {
    let cookie = Cookie::build((OAUTH_STATE_COOKIE, state.to_owned()))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .build();
    jar.add(cookie)
}

pub fn read_oauth_state(jar: &PrivateCookieJar) -> Option<String> {
    jar.get(OAUTH_STATE_COOKIE)
        .map(|cookie| cookie.value().to_owned())
}

pub fn clear_oauth_state(jar: PrivateCookieJar, secure: bool) -> PrivateCookieJar {
    let cookie = Cookie::build((OAUTH_STATE_COOKIE, ""))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .build();
    jar.remove(cookie)
}

pub fn has_required_scopes(scopes: &str) -> bool {
    let normalized = scopes
        .split([',', ' '])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    normalized.contains(&"repo") && normalized.contains(&"read:org")
}
