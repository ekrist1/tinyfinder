use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{sse::{Event, KeepAlive, Sse}, IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::llm::{ChatCompletionRequest, ChatCompletionStreamChunk, ChatMessage};
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
            payload.minimum_should_match,
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

pub async fn answer(
    State(state): State<Arc<AppState>>,
    Path(index_name): Path<String>,
    Json(payload): Json<AnswerRequest>,
) -> Result<Response, (StatusCode, Json<ApiResponse<()>>)> {
    validate_index_name(&index_name).map_err(|e| {
        (e.0, Json(ApiResponse::error(e.1.error.clone().unwrap_or_default())))
    })?;

    let llm_client = match state.llm_client.clone() {
        Some(client) => client,
        None => {
            return Err((
                StatusCode::NOT_IMPLEMENTED,
                Json(ApiResponse::error(
                    "MISTRAL_API_KEY not configured".to_string(),
                )),
            ))
        }
    };

    let limit = clamp_pagination_limit(payload.search_limit);
    let total_start = Instant::now();

    let (hits, _total, search_took_ms, _aggregations) = state
        .search_engine
        .search_with_options(
            &index_name,
            &payload.query,
            limit,
            0,
            &payload.fields,
            None,
            &[],
            payload.fuzzy,
            None,
            None, // minimum_should_match not needed for generative search
        )
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(e.to_string())),
            )
        })?;

    let mut sources_lines = Vec::new();
    for (idx, hit) in hits.iter().enumerate() {
        let fields_json = serde_json::to_string(&hit.fields).unwrap_or_default();
        sources_lines.push(format!(
            "[{}] id={} score={:.3} fields={}",
            idx + 1,
            hit.id,
            hit.score,
            fields_json
        ));
    }

    let sources_text = if sources_lines.is_empty() {
        "No sources found.".to_string()
    } else {
        sources_lines.join("\n")
    };

    let system_prompt = payload.system_prompt.unwrap_or_else(|| {
        "You are a helpful assistant. Answer the user's question using only the provided sources. If the answer is not contained in the sources, say you don't know. Use the input language for your answer.".to_string()
    });

    let user_prompt = format!(
        "Question: {}\n\nSources:\n{}",
        payload.query, sources_text
    );

    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system_prompt,
        },
        ChatMessage {
            role: "user".to_string(),
            content: user_prompt,
        },
    ];

    let llm_request = ChatCompletionRequest {
        model: llm_client.model().to_string(),
        messages,
        temperature: payload.temperature,
        max_tokens: payload.max_tokens,
        stream: payload.stream,
    };

    if payload.stream {
        let response = llm_client.stream(llm_request).await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(ApiResponse::error(e.to_string())),
            )
        })?;

        let model = llm_client.model().to_string();
        let meta = serde_json::json!({
            "model": model,
            "search_took_ms": search_took_ms,
            "sources": hits,
        });

        let stream = async_stream::stream! {
            yield Ok::<Event, Infallible>(Event::default().event("meta").data(meta.to_string()));

            let mut buffer = String::new();
            let mut bytes_stream = response.bytes_stream();

            while let Some(chunk) = bytes_stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        while let Some(pos) = buffer.find('\n') {
                            let line = buffer[..pos].trim_end().to_string();
                            buffer = buffer[pos + 1..].to_string();

                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                continue;
                            }

                            if let Some(data) = trimmed.strip_prefix("data:") {
                                let data = data.trim();
                                if data == "[DONE]" {
                                    yield Ok::<Event, Infallible>(Event::default().event("done").data(""));
                                    return;
                                }

                                match serde_json::from_str::<ChatCompletionStreamChunk>(data) {
                                    Ok(chunk) => {
                                        for choice in chunk.choices {
                                            if let Some(content) = choice.delta.content {
                                                yield Ok::<Event, Infallible>(Event::default().data(content));
                                            }
                                        }
                                    }
                                    Err(err) => {
                                        yield Ok::<Event, Infallible>(Event::default().event("error").data(format!("Invalid stream payload: {}", err)));
                                    }
                                }
                            }
                        }
                    }
                    Err(err) => {
                        yield Ok::<Event, Infallible>(Event::default().event("error").data(format!("Stream error: {}", err)));
                        return;
                    }
                }
            }
        };

        let sse = Sse::new(stream).keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        );

        return Ok(sse.into_response());
    }

    let llm_start = Instant::now();
    let response = llm_client.complete(llm_request).await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(ApiResponse::error(e.to_string())),
        )
    })?;

    let answer = response
        .choices
        .first()
        .map(|choice| choice.message.content.clone())
        .unwrap_or_default();

    let llm_took_ms = llm_start.elapsed().as_secs_f64() * 1000.0;
    let total_took_ms = total_start.elapsed().as_secs_f64() * 1000.0;

    let response = AnswerResponse {
        answer,
        model: llm_client.model().to_string(),
        search_took_ms,
        llm_took_ms,
        total_took_ms,
        sources: hits,
    };

    Ok(Json(ApiResponse::success(response)).into_response())
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

/// Add synonyms to an index
pub async fn add_synonyms(
    State(state): State<Arc<AppState>>,
    Path(index_name): Path<String>,
    Json(payload): Json<AddSynonymsRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<()>>)> {
    validate_index_name(&index_name).map_err(|e| {
        (e.0, Json(ApiResponse::error(e.1.error.clone().unwrap_or_default())))
    })?;

    state
        .search_engine
        .add_synonyms(&index_name, payload.synonyms)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(e.to_string())),
            )
        })?;

    Ok(Json(ApiResponse::success(serde_json::json!({
        "message": "Synonyms added successfully"
    }))))
}

/// Get synonyms for an index
pub async fn get_synonyms(
    State(state): State<Arc<AppState>>,
    Path(index_name): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<()>>)> {
    validate_index_name(&index_name).map_err(|e| {
        (e.0, Json(ApiResponse::error(e.1.error.clone().unwrap_or_default())))
    })?;

    let synonyms = state.search_engine.get_synonyms(&index_name);

    Ok(Json(ApiResponse::success(SynonymsResponse { synonyms })))
}

/// Clear all synonyms for an index
pub async fn clear_synonyms(
    State(state): State<Arc<AppState>>,
    Path(index_name): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<()>>)> {
    validate_index_name(&index_name).map_err(|e| {
        (e.0, Json(ApiResponse::error(e.1.error.clone().unwrap_or_default())))
    })?;

    state
        .search_engine
        .clear_synonyms(&index_name)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(e.to_string())),
            )
        })?;

    Ok(Json(ApiResponse::success(serde_json::json!({
        "message": "Synonyms cleared successfully"
    }))))
}

/// Add pinned rules to an index
pub async fn add_pinned_rules(
    State(state): State<Arc<AppState>>,
    Path(index_name): Path<String>,
    Json(payload): Json<AddPinnedRulesRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<()>>)> {
    validate_index_name(&index_name).map_err(|e| {
        (e.0, Json(ApiResponse::error(e.1.error.clone().unwrap_or_default())))
    })?;

    state
        .search_engine
        .add_pinned_rules(&index_name, payload.rules)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(e.to_string())),
            )
        })?;

    Ok(Json(ApiResponse::success(serde_json::json!({
        "message": "Pinned rules added successfully"
    }))))
}

/// Get pinned rules for an index
pub async fn get_pinned_rules(
    State(state): State<Arc<AppState>>,
    Path(index_name): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<()>>)> {
    validate_index_name(&index_name).map_err(|e| {
        (e.0, Json(ApiResponse::error(e.1.error.clone().unwrap_or_default())))
    })?;

    let rules = state.search_engine.get_pinned_rules(&index_name);

    Ok(Json(ApiResponse::success(PinnedRulesResponse { rules })))
}

/// Clear all pinned rules for an index
pub async fn clear_pinned_rules(
    State(state): State<Arc<AppState>>,
    Path(index_name): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<()>>)> {
    validate_index_name(&index_name).map_err(|e| {
        (e.0, Json(ApiResponse::error(e.1.error.clone().unwrap_or_default())))
    })?;

    state
        .search_engine
        .clear_pinned_rules(&index_name)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(e.to_string())),
            )
        })?;

    Ok(Json(ApiResponse::success(serde_json::json!({
        "message": "Pinned rules cleared successfully"
    }))))
}
