use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateIndexRequest {
    pub name: String,
    #[serde(default)]
    pub fields: Vec<FieldConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FieldConfig {
    pub name: String,
    #[serde(default = "default_field_type")]
    pub field_type: String, // "text", "string", "i64", "f64", "date"
    #[serde(default)]
    pub stored: bool,
    #[serde(default)]
    pub indexed: bool,
}

fn default_field_type() -> String {
    "text".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub fields: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddDocumentsRequest {
    pub documents: Vec<Document>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub boost: HashMap<String, f32>,
    #[serde(default)]
    pub fuzzy: bool,
}

fn default_limit() -> usize {
    10
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub took_ms: f64,
    pub total: usize,
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub score: f32,
    pub fields: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct IndexInfo {
    pub name: String,
    pub document_count: u64,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(message: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BulkOperation {
    pub operation: String, // "index" or "delete"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<Document>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BulkRequest {
    pub operations: Vec<BulkOperation>,
}

#[derive(Debug, Serialize)]
pub struct BulkResponse {
    pub total: usize,
    pub successful: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}
