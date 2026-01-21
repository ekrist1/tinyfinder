use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{delete, get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

mod auth;
mod handlers;
mod llm;
mod models;
mod search;
mod storage;
mod validation;

use search::SearchEngine;
use storage::MetadataStore;
use llm::LlmClient;

pub struct AppState {
    search_engine: SearchEngine,
    metadata_store: MetadataStore,
    api_tokens: Vec<String>,
    llm_client: Option<LlmClient>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    tracing::info!("Starting Simple Search Service v0.2.0");

    // Load environment variables from .env if present
    dotenvy::dotenv().ok();

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
    let llm_client = LlmClient::from_env();

    if llm_client.is_none() {
        tracing::warn!(
            "MISTRAL_API_KEY not set - generative answer endpoint disabled"
        );
    }

    let loaded_indices = search_engine.load_indices()?;
    if loaded_indices.is_empty() {
        tracing::info!("No existing indices found to load");
    } else {
        tracing::info!("Loaded {} index(es): {:?}", loaded_indices.len(), loaded_indices);
        metadata_store.sync_indices_from_disk(&loaded_indices)?;

        for index_name in &loaded_indices {
            match search_engine.collect_document_ids(index_name) {
                Ok(doc_ids) => {
                    if let Err(e) = metadata_store.reset_index_documents(index_name, &doc_ids) {
                        tracing::warn!(
                            "Failed to rebuild metadata documents for index '{}': {}",
                            index_name,
                            e
                        );
                    } else {
                        tracing::info!(
                            "Rebuilt metadata for index '{}' with {} document(s)",
                            index_name,
                            doc_ids.len()
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to collect document IDs for index '{}': {}",
                        index_name,
                        e
                    );
                }
            }
        }
    }

    let state = Arc::new(AppState {
        search_engine,
        metadata_store,
        api_tokens,
        llm_client,
    });

    // Public routes (no authentication required)
    let public_routes = Router::new()
        .route("/health", get(handlers::health_check))
        .route("/indices", get(handlers::list_indices))
        .route("/indices/:name/search", post(handlers::search))
        .route("/indices/:name/answer", post(handlers::answer))
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
        .route("/indices/:name/synonyms", post(handlers::add_synonyms))
        .route("/indices/:name/synonyms", get(handlers::get_synonyms))
        .route("/indices/:name/synonyms", delete(handlers::clear_synonyms))
        .route("/indices/:name/pinned", post(handlers::add_pinned_rules))
        .route("/indices/:name/pinned", get(handlers::get_pinned_rules))
        .route("/indices/:name/pinned", delete(handlers::clear_pinned_rules))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ));

    // Configure CORS based on environment
    let cors_layer = build_cors_layer();

    // Combine routes
    let app = Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .layer(cors_layer)
        .layer(TraceLayer::new_for_http())
        .layer(DefaultBodyLimit::max(validation::MAX_REQUEST_BODY_SIZE))
        .with_state(state);

    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse::<u16>()
        .unwrap_or(3000);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Graceful shutdown handling
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("Server shutdown complete");
    Ok(())
}

/// Build CORS layer based on CORS_ORIGINS environment variable
fn build_cors_layer() -> CorsLayer {
    let origins = std::env::var("CORS_ORIGINS").unwrap_or_default();

    if origins.is_empty() || origins == "*" {
        tracing::warn!("CORS_ORIGINS not set or set to '*' - allowing all origins (not recommended for production)");
        CorsLayer::permissive()
    } else {
        let allowed_origins: Vec<_> = origins
            .split(',')
            .filter_map(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    trimmed.parse().ok()
                }
            })
            .collect();

        if allowed_origins.is_empty() {
            tracing::warn!("No valid CORS origins parsed, falling back to permissive");
            CorsLayer::permissive()
        } else {
            tracing::info!("CORS configured for {} origin(s)", allowed_origins.len());
            CorsLayer::new()
                .allow_origin(AllowOrigin::list(allowed_origins))
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::DELETE,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([
                    axum::http::header::CONTENT_TYPE,
                    axum::http::header::AUTHORIZATION,
                ])
        }
    }
}

/// Graceful shutdown signal handler
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C, initiating graceful shutdown...");
        }
        _ = terminate => {
            tracing::info!("Received SIGTERM, initiating graceful shutdown...");
        }
    }
}
