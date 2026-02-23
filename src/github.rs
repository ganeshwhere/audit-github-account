use reqwest::Client;

#[derive(Clone)]
pub struct GitHubClient {
    pub http: Client,
}

impl GitHubClient {
    pub fn new() -> Self {
        let http = Client::builder()
            .user_agent("collaborator-audit-dashboard")
            .build()
            .expect("reqwest client should build");
        Self { http }
    }
}
