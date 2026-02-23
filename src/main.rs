mod auth;
mod error;
mod github;
mod handlers;
mod middleware;
mod models;
mod utils;

use std::net::SocketAddr;

use axum::{Router, routing::get};
use axum_extra::extract::cookie::Key;
use github::GitHubClient;
use tokio::net::TcpListener;
use tracing::info;

use crate::utils::AppConfig;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub github: GitHubClient,
    pub cookie_key: Key,
}

impl axum::extract::FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.cookie_key.clone()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    utils::init_tracing();

    let config = AppConfig::from_env()?;
    let cookie_key = Key::from(config.session_secret.as_bytes());
    let github = GitHubClient::new();

    let state = AppState {
        config,
        github,
        cookie_key,
    };

    let app = Router::new()
        .route("/", get(handlers::index))
        .route("/health", get(handlers::health))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = TcpListener::bind(addr).await?;
    info!("listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
