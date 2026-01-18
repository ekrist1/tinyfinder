use axum::http::StatusCode;
use axum::Json;
use once_cell::sync::Lazy;
use regex::Regex;

use crate::models::ApiResponse;

/// Maximum length for index names
pub const MAX_INDEX_NAME_LENGTH: usize = 64;

/// Maximum number of documents in a single request
pub const MAX_DOCUMENTS_PER_REQUEST: usize = 1000;

/// Maximum number of bulk operations in a single request
pub const MAX_BULK_OPERATIONS: usize = 1000;

/// Maximum pagination limit
pub const MAX_PAGINATION_LIMIT: usize = 1000;

/// Default request body size limit (10MB)
pub const MAX_REQUEST_BODY_SIZE: usize = 10 * 1024 * 1024;

/// Regex pattern for valid index names: alphanumeric, underscore, hyphen
static INDEX_NAME_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[a-zA-Z][a-zA-Z0-9_-]*$").expect("Invalid regex pattern")
});

/// Validates an index name for security and consistency
pub fn validate_index_name(name: &str) -> Result<(), (StatusCode, Json<ApiResponse<()>>)> {
    if name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("Index name cannot be empty".to_string())),
        ));
    }

    if name.len() > MAX_INDEX_NAME_LENGTH {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error(format!(
                "Index name exceeds maximum length of {} characters",
                MAX_INDEX_NAME_LENGTH
            ))),
        ));
    }

    if !INDEX_NAME_PATTERN.is_match(name) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error(
                "Index name must start with a letter and contain only alphanumeric characters, underscores, or hyphens".to_string()
            )),
        ));
    }

    // Check for path traversal attempts
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error("Index name contains invalid characters".to_string())),
        ));
    }

    Ok(())
}

/// Validates document count in a request
pub fn validate_document_count(count: usize) -> Result<(), (StatusCode, Json<ApiResponse<()>>)> {
    if count > MAX_DOCUMENTS_PER_REQUEST {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error(format!(
                "Too many documents in request. Maximum allowed: {}",
                MAX_DOCUMENTS_PER_REQUEST
            ))),
        ));
    }
    Ok(())
}

/// Validates bulk operation count
pub fn validate_bulk_operation_count(count: usize) -> Result<(), (StatusCode, Json<ApiResponse<()>>)> {
    if count > MAX_BULK_OPERATIONS {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::error(format!(
                "Too many operations in bulk request. Maximum allowed: {}",
                MAX_BULK_OPERATIONS
            ))),
        ));
    }
    Ok(())
}

/// Clamps pagination limit to maximum allowed value
pub fn clamp_pagination_limit(limit: usize) -> usize {
    limit.min(MAX_PAGINATION_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_index_names() {
        assert!(validate_index_name("myindex").is_ok());
        assert!(validate_index_name("my_index").is_ok());
        assert!(validate_index_name("my-index").is_ok());
        assert!(validate_index_name("MyIndex123").is_ok());
        assert!(validate_index_name("a").is_ok());
    }

    #[test]
    fn test_invalid_index_names() {
        assert!(validate_index_name("").is_err());
        assert!(validate_index_name("123abc").is_err()); // starts with number
        assert!(validate_index_name("_index").is_err()); // starts with underscore
        assert!(validate_index_name("-index").is_err()); // starts with hyphen
        assert!(validate_index_name("my index").is_err()); // contains space
        assert!(validate_index_name("../etc").is_err()); // path traversal
        assert!(validate_index_name("my/index").is_err()); // contains slash
        assert!(validate_index_name("my\\index").is_err()); // contains backslash
    }
}
