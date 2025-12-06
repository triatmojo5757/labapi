use std::sync::Arc;

use axum::{
    middleware::from_fn_with_state,
    routing::{get, patch, post},
    Router,
};
use dotenvy::dotenv;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

mod app_state;
mod errors;
mod models;
mod utils;

mod middleware {
    pub mod auth;
    pub mod rbac;
}

mod routes {
    pub mod auth;
    pub mod profile;
    pub mod accounts;
    pub mod journals;
    pub mod transfers;
    pub mod cash;
    pub mod admin;
}

use app_state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let subscriber = FmtSubscriber::builder().with_max_level(Level::INFO).finish();
    tracing::subscriber::set_global_default(subscriber).ok();

    let db_url = std::env::var("DATABASE_URL")?;
    let jwt_secret = std::env::var("JWT_SECRET")?;

    let pool = PgPoolOptions::new().max_connections(10).connect(&db_url).await?;

    let state = Arc::new(AppState {
        pool,
        jwt_secret: Arc::new(jwt_secret),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any);

    // === Public (tanpa Authorization) ===
    let public = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/journals/:id", get(routes::journals::get_journal_public))
        .route("/journals/public", get(routes::journals::list_journals_public))
        .route("/accounts/verify", post(routes::accounts::verify_account)); 

    // === Auth endpoints (juga public) ===
    let auth_routes = Router::new()
        .route("/register", post(routes::auth::register))
        .route("/login", post(routes::auth::login))
        .route("/refresh", post(routes::auth::refresh))
        .route("/password_reset", post(routes::auth::password_reset));

    // === Protected (wajib Authorization) ===
    let protected = Router::new()
        .route("/me", get(me))
        .route("/auth/logout/:token_id", post(routes::auth::logout))
        .route("/profile", get(routes::profile::get_profile).put(routes::profile::upsert_profile))
        .route("/accounts/open", post(routes::accounts::open_account))
        .route("/accounts", get(routes::accounts::list_accounts))
        .route("/accounts/:account_id/pin", patch(routes::accounts::update_account_pin)) 
        .route("/journals", post(routes::journals::post_journal).get(routes::journals::list_journals))
        .route("/transfers", post(routes::transfers::transfer))
        .route("/accounts/deposit", post(routes::cash::cash_deposit))
        .route("/accounts/withdraw", post(routes::cash::cash_withdraw))
        .route("/accounts/check_pin", post(routes::accounts::check_pin))
        .route("/accounts/list_rekening_pt", get(routes::accounts::list_rekening_pt))
        .layer(from_fn_with_state(state.clone(), middleware::auth::auth_middleware));

    // === Admin (RBAC + Auth) ===
    let admin = Router::new()
        .route("/admin/audit-logs", get(routes::admin::list_audit_logs))
        .layer(from_fn_with_state(state.clone(), middleware::rbac::rbac_middleware))
        .layer(from_fn_with_state(state.clone(), middleware::auth::auth_middleware));

    let app = Router::new()
        .nest("/auth", auth_routes)
        .merge(public)
        .merge(protected)
        .merge(admin)
        .layer(cors)
        .with_state(state.clone());

    let listener = TcpListener::bind("0.0.0.0:8080").await?;
    info!("listening on 0.0.0.0:8080");
    axum::serve(listener, app).await?;
    Ok(())
}

// /me sederhana (protected via middleware)
async fn me(axum::Extension(claims): axum::Extension<models::Claims>) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "user_id": claims.sub, "role": claims.role }))
}