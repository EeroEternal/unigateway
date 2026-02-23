use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct ModelList {
    pub object: &'static str,
    pub data: Vec<ModelItem>,
}

#[derive(Serialize)]
pub struct ModelItem {
    pub id: String,
    pub object: &'static str,
    pub created: i64,
    pub owned_by: &'static str,
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Provider {
    pub id: i64,
    pub name: String,
    pub provider_type: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model_mapping: Option<String>,
    pub weight: i64,
    pub is_enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateProviderForm {
    pub name: String,
    pub provider_type: String,
    pub base_url: Option<String>,
    pub api_key: String,
}
