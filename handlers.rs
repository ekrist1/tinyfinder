use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;

use crate::models::*;
use crate::AppState;

pub async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "simple-search-service",
        "version": "0.1.0"
    }))
}

pub async fn create_index(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateIndexRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<()>>)> {
    // Set default fields if none provided
    let fields = if payload.fields.is_empty() {
        vec![
            FieldConfig {
                name: "title".to_string(),
                field_type: "text".to_string(),
                stored: true,
                indexed: true,
            },
            FieldConfig {
                name: "content".to_string(),
                field_type: "text".to_string(),
                stored: true,
                indexed: true,
            },
        ]
    } else {
        payload.fields
    };

    state
        .search_engine
        .create_index(&payload.name, &fields)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(e.to_string())),
            )
        })?;

    state
        .metadata_store
        .create_index(&payload.name)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(e.to_string())),
            )
        })?;

    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success(serde_json::json!({
            "message": "Index created successfully",
            "name": payload.name
        }))),
    ))
}

pub async fn list_indices(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<Vec<IndexInfo>>>)> {
    let indices = state.metadata_store.list_indices().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error(e.to_string())),
        )
    })?;

    Ok(Json(ApiResponse::success(indices)))
}

pub async fn delete_index(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<()>>)> {
    state.search_engine.delete_index(&name).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error(e.to_string())),
        )
    })?;

    state.metadata_store.delete_index(&name).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error(e.to_string())),
        )
    })?;

    Ok((
        StatusCode::OK,
        Json(ApiResponse::success(serde_json::json!({
            "message": "Index deleted successfully"
        }))),
    ))
}

pub async fn add_documents(
    State(state): State<Arc<AppState>>,
    Path(index_name): Path<String>,
    Json(payload): Json<AddDocumentsRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<()>>)> {
    state
        .search_engine
        .add_documents(&index_name, &payload.documents)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(e.to_string())),
            )
        })?;

    // Update metadata
    for doc in &payload.documents {
        state
            .metadata_store
            .add_document(&index_name, &doc.id)
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::error(e.to_string())),
                )
            })?;
    }

    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success(serde_json::json!({
            "message": "Documents added successfully",
            "count": payload.documents.len()
        }))),
    ))
}

pub async fn delete_document(
    State(state): State<Arc<AppState>>,
    Path((index_name, doc_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<()>>)> {
    state
        .search_engine
        .delete_document(&index_name, &doc_id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(e.to_string())),
            )
        })?;

    state.metadata_store.delete_document(&doc_id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error(e.to_string())),
        )
    })?;

    Ok((
        StatusCode::OK,
        Json(ApiResponse::success(serde_json::json!({
            "message": "Document deleted successfully"
        }))),
    ))
}

pub async fn search(
    State(state): State<Arc<AppState>>,
    Path(index_name): Path<String>,
    Json(payload): Json<SearchRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<SearchResponse>>)> {
    let (hits, took_ms) = state
        .search_engine
        .search(&index_name, &payload.query, payload.limit, &payload.fields)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(e.to_string())),
            )
        })?;

    let response = SearchResponse {
        took_ms,
        total: hits.len(),
        hits,
    };

    Ok(Json(ApiResponse::success(response)))
}

pub async fn bulk_operation(
    State(state): State<Arc<AppState>>,
    Path(index_name): Path<String>,
    Json(payload): Json<BulkRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<BulkResponse>>)> {
    let mut successful = 0;
    let mut failed = 0;
    let mut errors = Vec::new();

    for (idx, op) in payload.operations.iter().enumerate() {
        let result = match op.operation.as_str() {
            "index" => {
                if let Some(doc) = &op.document {
                    match state.search_engine.add_documents(&index_name, &[doc.clone()]) {
                        Ok(_) => {
                            let _ = state.metadata_store.add_document(&index_name, &doc.id);
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    Err(anyhow::anyhow!("Missing document for index operation"))
                }
            }
            "delete" => {
                if let Some(id) = &op.id {
                    match state.search_engine.delete_document(&index_name, id) {
                        Ok(_) => {
                            let _ = state.metadata_store.delete_document(id);
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    Err(anyhow::anyhow!("Missing id for delete operation"))
                }
            }
            _ => Err(anyhow::anyhow!("Unknown operation: {}", op.operation)),
        };

        match result {
            Ok(_) => successful += 1,
            Err(e) => {
                failed += 1;
                errors.push(format!("Operation {} failed: {}", idx, e));
            }
        }
    }

    let response = BulkResponse {
        total: payload.operations.len(),
        successful,
        failed,
        errors,
    };

    Ok(Json(ApiResponse::success(response)))
}
