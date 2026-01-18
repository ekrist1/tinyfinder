use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;

use crate::models::*;
use crate::validation::{
    clamp_pagination_limit, validate_bulk_operation_count, validate_document_count,
    validate_index_name,
};
use crate::AppState;

pub async fn health_check(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let db_status = match state.metadata_store.health_check() {
        Ok(_) => "healthy",
        Err(_) => "unhealthy",
    };

    let is_healthy = db_status == "healthy";

    let status_code = if is_healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status_code,
        Json(serde_json::json!({
            "status": if is_healthy { "healthy" } else { "unhealthy" },
            "service": "simple-search-service",
            "version": "0.2.0",
            "checks": {
                "database": db_status
            }
        })),
    )
}

pub async fn create_index(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateIndexRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<()>>)> {
    validate_index_name(&payload.name)?;

    // Set default fields if none provided
    let fields = if payload.fields.is_empty() {
        vec![
            FieldConfig {
                name: "title".to_string(),
                field_type: "text".to_string(),
                stored: true,
                indexed: true,
                analyzer: "default".to_string(),
                fast: false,
            },
            FieldConfig {
                name: "content".to_string(),
                field_type: "text".to_string(),
                stored: true,
                indexed: true,
                analyzer: "default".to_string(),
                fast: false,
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
    validate_index_name(&name)?;

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
    validate_index_name(&index_name)?;
    validate_document_count(payload.documents.len())?;

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
    validate_index_name(&index_name)?;

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
    validate_index_name(&index_name).map_err(|e| {
        (e.0, Json(ApiResponse::error(e.1.error.clone().unwrap_or_default())))
    })?;

    let limit = clamp_pagination_limit(payload.limit);

    let (hits, total, took_ms, aggregations) = state
        .search_engine
        .search_with_options(
            &index_name,
            &payload.query,
            limit,
            payload.offset,
            &payload.fields,
            payload.highlight.as_ref(),
            &payload.aggregations,
            payload.fuzzy,
            payload.sort.as_ref(),
        )
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(e.to_string())),
            )
        })?;

    let has_more = payload.offset + hits.len() < total;

    let response = SearchResponse {
        took_ms,
        total,
        offset: payload.offset,
        limit,
        has_more,
        hits,
        aggregations,
    };

    Ok(Json(ApiResponse::success(response)))
}

pub async fn get_index_stats(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<IndexStats>>)> {
    validate_index_name(&name).map_err(|e| {
        (e.0, Json(ApiResponse::error(e.1.error.clone().unwrap_or_default())))
    })?;

    // Get created_at from metadata store
    let indices = state.metadata_store.list_indices().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error(e.to_string())),
        )
    })?;

    let index_info = indices.iter().find(|i| i.name == name).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::error(format!("Index not found: {}", name))),
        )
    })?;

    let stats = state
        .search_engine
        .get_index_stats(&name, &index_info.created_at)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(e.to_string())),
            )
        })?;

    Ok(Json(ApiResponse::success(stats)))
}

pub async fn suggest(
    State(state): State<Arc<AppState>>,
    Path(index_name): Path<String>,
    Json(payload): Json<SuggestRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<SuggestResponse>>)> {
    validate_index_name(&index_name).map_err(|e| {
        (e.0, Json(ApiResponse::error(e.1.error.clone().unwrap_or_default())))
    })?;

    let (suggestions, took_ms) = state
        .search_engine
        .suggest(
            &index_name,
            &payload.prefix,
            payload.field.as_deref(),
            payload.limit,
        )
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(e.to_string())),
            )
        })?;

    let response = SuggestResponse {
        suggestions,
        took_ms,
    };

    Ok(Json(ApiResponse::success(response)))
}

pub async fn bulk_operation(
    State(state): State<Arc<AppState>>,
    Path(index_name): Path<String>,
    Json(payload): Json<BulkRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<BulkResponse>>)> {
    validate_index_name(&index_name).map_err(|e| {
        (e.0, Json(ApiResponse::error(e.1.error.clone().unwrap_or_default())))
    })?;
    validate_bulk_operation_count(payload.operations.len()).map_err(|e| {
        (e.0, Json(ApiResponse::error(e.1.error.clone().unwrap_or_default())))
    })?;

    let mut successful = 0;
    let mut failed = 0;
    let mut errors = Vec::new();

    for (idx, op) in payload.operations.iter().enumerate() {
        let result = match op.operation.as_str() {
            "index" => {
                if let Some(doc) = &op.document {
                    match state
                        .search_engine
                        .add_documents(&index_name, std::slice::from_ref(doc))
                    {
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
