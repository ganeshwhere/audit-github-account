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
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub push: bool,
    #[serde(default)]
    pub pull: bool,
    #[serde(default)]
    pub maintain: bool,
    #[serde(default)]
    pub triage: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collaborator {
    pub login: String,
    pub id: u64,
    #[serde(default)]
    pub permissions: Permissions,
    pub role_name: Option<String>,
}

impl Collaborator {
    pub fn permission_label(&self) -> &'static str {
        if self.permissions.admin {
            "admin"
        } else if self.permissions.maintain {
            "maintain"
        } else if self.permissions.push {
            "write"
        } else if self.permissions.triage {
            "triage"
        } else {
            "read"
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoWithCollaborators {
    pub repo: Repository,
    pub collaborators: Vec<Collaborator>,
    pub can_remove: bool,
}

#[derive(Debug, Deserialize)]
pub struct OAuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
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
    pub access_token: Option<String>,
    pub scope: Option<String>,
    pub token_type: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GitHubUser {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CollaboratorPermission {
    pub permission: String,
    pub role_name: Option<String>,
    pub user: GitHubUser,
}

#[derive(Debug, Deserialize)]
pub struct DashboardQuery {
    #[serde(default)]
    pub ignore_forks: bool,
    #[serde(default)]
    pub ignore_archived: bool,
}

#[derive(Debug, Clone)]
pub struct RepoFilterOptions {
    pub ignore_forks: bool,
    pub ignore_archived: bool,
}

impl From<DashboardQuery> for RepoFilterOptions {
    fn from(value: DashboardQuery) -> Self {
        Self {
            ignore_forks: value.ignore_forks,
            ignore_archived: value.ignore_archived,
        }
    }
}
