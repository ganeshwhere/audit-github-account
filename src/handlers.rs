use axum::response::{Html, IntoResponse};

pub async fn index() -> impl IntoResponse {
    Html("<h1>GitHub Collaborator Dashboard</h1><p><a href=\"/auth/login\">Log in with GitHub</a></p>")
}

pub async fn health() -> impl IntoResponse {
    "ok"
}
