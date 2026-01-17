use axum::{
    routing::{get, post, delete},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing_subscriber;

mod handlers;
mod models;
mod search;
mod storage;

use search::SearchEngine;
use storage::MetadataStore;

pub struct AppState {
    search_engine: SearchEngine,
    metadata_store: MetadataStore,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    tracing::info!("Starting Simple Search Service");

    // Initialize storage
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    std::fs::create_dir_all(&data_dir)?;

    let metadata_store = MetadataStore::new(&format!("{}/metadata.db", data_dir))?;
    let search_engine = SearchEngine::new(&format!("{}/indices", data_dir))?;

    let state = Arc::new(AppState {
        search_engine,
        metadata_store,
    });

    // Build router
    let app = Router::new()
        .route("/health", get(handlers::health_check))
        .route("/indices", get(handlers::list_indices))
        .route("/indices", post(handlers::create_index))
        .route("/indices/:name", delete(handlers::delete_index))
        .route("/indices/:name/documents", post(handlers::add_documents))
        .route("/indices/:name/documents/:id", delete(handlers::delete_document))
        .route("/indices/:name/search", post(handlers::search))
        .route("/indices/:name/bulk", post(handlers::bulk_operation))
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
