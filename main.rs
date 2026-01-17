use axum::{
    middleware,
    routing::{delete, get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

mod auth;
mod handlers;
mod models;
mod search;
mod storage;

use search::SearchEngine;
use storage::MetadataStore;

pub struct AppState {
    search_engine: SearchEngine,
    metadata_store: MetadataStore,
    api_tokens: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    tracing::info!("Starting Simple Search Service v0.2.0");

    // Initialize storage
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    std::fs::create_dir_all(&data_dir)?;

    // Load API tokens from environment
    let api_tokens: Vec<String> = std::env::var("API_TOKENS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().to_string())
        .collect();

    if api_tokens.is_empty() {
        tracing::warn!("No API_TOKENS configured - authentication disabled");
    } else {
        tracing::info!(
            "API authentication enabled with {} token(s)",
            api_tokens.len()
        );
    }

    let metadata_store = MetadataStore::new(&format!("{}/metadata.db", data_dir))?;
    let search_engine = SearchEngine::new(&format!("{}/indices", data_dir))?;

    let state = Arc::new(AppState {
        search_engine,
        metadata_store,
        api_tokens,
    });

    // Public routes (no authentication required)
    let public_routes = Router::new()
        .route("/health", get(handlers::health_check))
        .route("/indices", get(handlers::list_indices))
        .route("/indices/:name/search", post(handlers::search))
        .route("/indices/:name/stats", get(handlers::get_index_stats))
        .route("/indices/:name/suggest", post(handlers::suggest));

    // Protected routes (require authentication when API_TOKENS is set)
    let protected_routes = Router::new()
        .route("/indices", post(handlers::create_index))
        .route("/indices/:name", delete(handlers::delete_index))
        .route("/indices/:name/documents", post(handlers::add_documents))
        .route(
            "/indices/:name/documents/:id",
            delete(handlers::delete_document),
        )
        .route("/indices/:name/bulk", post(handlers::bulk_operation))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ));

    // Combine routes
    let app = Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .layer(CorsLayer::permissive())
        .with_state(state);

    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse::<u16>()
        .unwrap_or(3000);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
