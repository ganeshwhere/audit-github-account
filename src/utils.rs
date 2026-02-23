use std::env;

use rand::{Rng, distributions::Alphanumeric, rngs::ThreadRng};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
use url::Url;

use crate::error::AppError;

#[derive(Clone)]
pub struct AppConfig {
    pub github_client_id: String,
    pub github_client_secret: String,
    pub session_secret: String,
    pub base_url: Url,
    pub max_concurrency: usize,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, AppError> {
        let github_client_id = require_env("GITHUB_CLIENT_ID")?;
        let github_client_secret = require_env("GITHUB_CLIENT_SECRET")?;
        let session_secret = require_env("SESSION_SECRET")?;
        let base_url = Url::parse(&require_env("BASE_URL")?)
            .map_err(|e| AppError::Config(format!("invalid BASE_URL: {e}")))?;

        Ok(Self {
            github_client_id,
            github_client_secret,
            session_secret,
            base_url,
            max_concurrency: 10,
        })
    }
}

pub fn require_env(key: &str) -> Result<String, AppError> {
    env::var(key).map_err(|_| AppError::Config(format!("missing required env var: {key}")))
}

pub fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer())
        .init();
}

pub fn random_token(len: usize) -> String {
    let mut rng: ThreadRng = rand::thread_rng();
    (&mut rng)
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

pub fn parse_next_link(link_header: Option<&str>) -> Option<String> {
    let header = link_header?;
    for item in header.split(',') {
        let trimmed = item.trim();
        if trimmed.contains("rel=\"next\"") {
            let start = trimmed.find('<')?;
            let end = trimmed.find('>')?;
            return Some(trimmed[start + 1..end].to_owned());
        }
    }
    None
}
