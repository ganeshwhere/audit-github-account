use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub access_token: String,
    pub user_login: String,
    pub csrf_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Owner {
    pub login: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub id: u64,
    pub name: String,
    pub owner: Owner,
    pub private: bool,
    pub archived: bool,
    pub fork: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Permissions {
    pub admin: bool,
    pub push: bool,
    pub pull: bool,
    pub maintain: Option<bool>,
    pub triage: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collaborator {
    pub login: String,
    pub id: u64,
    pub permissions: Permissions,
    pub role_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoWithCollaborators {
    pub repo: Repository,
    pub collaborators: Vec<Collaborator>,
    pub can_remove: bool,
}

#[derive(Debug, Deserialize)]
pub struct OAuthCallbackQuery {
    pub code: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
pub struct RemoveRequest {
    pub items: Vec<RemoveItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RemoveItem {
    pub repo: String,
    pub username: String,
}

#[derive(Debug, Serialize)]
pub struct RemoveSuccess {
    pub repo: String,
    pub username: String,
}

#[derive(Debug, Serialize)]
pub struct RemoveFailure {
    pub repo: String,
    pub username: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct RemoveResponse {
    pub success: Vec<RemoveSuccess>,
    pub failed: Vec<RemoveFailure>,
}

#[derive(Debug, Deserialize)]
pub struct GitHubAccessTokenResponse {
    pub access_token: String,
    pub scope: String,
    pub token_type: String,
}

#[derive(Debug, Deserialize)]
pub struct GitHubUser {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct CollaboratorPermission {
    pub permission: String,
    pub role_name: Option<String>,
    pub user: GitHubUser,
}
