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
    #[serde(default = "default_analyzer")]
    pub analyzer: String, // "default", "norwegian", "raw"
    #[serde(default)]
    pub fast: bool, // Enable FAST flag for aggregations
}

fn default_field_type() -> String {
    "text".to_string()
}

fn default_analyzer() -> String {
    "default".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    pub offset: usize,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub boost: HashMap<String, f32>,
    #[serde(default)]
    pub fuzzy: bool,
    #[serde(default)]
    pub sort: Option<SortOption>,
    #[serde(default)]
    pub highlight: Option<HighlightOptions>,
    #[serde(default)]
    pub aggregations: Vec<AggregationRequest>,
}

fn default_limit() -> usize {
    10
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HighlightOptions {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default = "default_pre_tag")]
    pub pre_tag: String,
    #[serde(default = "default_post_tag")]
    pub post_tag: String,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    #[default]
    Asc,
    Desc,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SortOption {
    pub field: String,
    #[serde(default)]
    pub order: SortOrder,
}

fn default_true() -> bool {
    true
}

fn default_pre_tag() -> String {
    "<em>".to_string()
}

fn default_post_tag() -> String {
    "</em>".to_string()
}

impl Default for HighlightOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            fields: Vec::new(),
            pre_tag: default_pre_tag(),
            post_tag: default_post_tag(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AggregationRequest {
    pub name: String,
    pub agg_type: String, // "terms", "histogram", "range", "stats"
    pub field: String,
    #[serde(default)]
    pub size: Option<usize>,
    #[serde(default)]
    pub interval: Option<f64>,
    #[serde(default)]
    pub ranges: Option<Vec<RangeSpec>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RangeSpec {
    pub from: Option<f64>,
    pub to: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct AggregationResult {
    pub name: String,
    pub buckets: Option<Vec<AggregationBucket>>,
    pub stats: Option<StatsResult>,
}

#[derive(Debug, Serialize)]
pub struct AggregationBucket {
    pub key: serde_json::Value,
    pub doc_count: u64,
}

#[derive(Debug, Serialize)]
pub struct StatsResult {
    pub count: u64,
    pub sum: f64,
    pub avg: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub took_ms: f64,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
    pub hits: Vec<SearchHit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregations: Option<Vec<AggregationResult>>,
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub score: f32,
    pub fields: HashMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlights: Option<HashMap<String, Vec<String>>>,
}

#[derive(Debug, Serialize)]
pub struct IndexInfo {
    pub name: String,
    pub document_count: u64,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct IndexStats {
    pub name: String,
    pub document_count: u64,
    pub size_bytes: u64,
    pub fields: Vec<FieldStats>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct FieldStats {
    pub name: String,
    pub field_type: String,
    pub indexed: bool,
    pub stored: bool,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct SuggestRequest {
    pub prefix: String,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default = "default_suggest_limit")]
    pub limit: usize,
}

fn default_suggest_limit() -> usize {
    10
}

#[derive(Debug, Serialize)]
pub struct SuggestResponse {
    pub suggestions: Vec<String>,
    pub took_ms: f64,
}
