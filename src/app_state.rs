use std::sync::Arc;

use serde::Deserialize;
use sqlx::PgPool;

#[derive(Clone, Deserialize)]
pub struct FirebaseServiceAccount {
    pub project_id: String,
    pub client_email: String,
    pub private_key: String,
    pub token_uri: Option<String>,
}

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub pool2: PgPool,
    pub jwt_secret: Arc<String>,
    pub firebase: Option<Arc<FirebaseServiceAccount>>,
    pub digiflazz: DigiflazzConfig,
}

pub type SharedState = Arc<AppState>;

#[derive(Clone)]
pub struct DigiflazzConfig {
    pub username: String,
    pub dev_key: String,
    pub prod_key: String,
    pub use_production: bool,
}
