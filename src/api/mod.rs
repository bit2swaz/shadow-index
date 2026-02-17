mod handlers;
mod state;

pub use state::AppState;

use axum::{routing::get, Router};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

/// Build the Axum router with all routes and middleware
pub fn create_router(state: AppState) -> Router {
    // Configure CORS to allow any origin (permissive for development)
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Health check endpoint
        .route("/api/health", get(handlers::health_check))
        // Block endpoints
        .route("/api/blocks/latest", get(handlers::get_latest_blocks))
        // Transaction endpoints
        .route("/api/tx/:hash", get(handlers::get_transaction))
        // Add state to all routes
        .with_state(state)
        // Add middleware
        .layer(cors)
        .layer(TraceLayer::new_for_http())
}
