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
    /// Minimum number of SHOULD clauses that must match (for BooleanQuery)
    #[serde(default)]
    pub minimum_should_match: Option<usize>,
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

// Note: Old aggregation types kept for backwards compatibility reference
// The API now uses Tantivy's built-in AggregationResults type which is Elasticsearch-compatible

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub took_ms: f64,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
    pub hits: Vec<SearchHit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregations: Option<tantivy::aggregation::agg_result::AggregationResults>,
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub score: f32,
    pub fields: HashMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlights: Option<HashMap<String, Vec<String>>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnswerRequest {
    pub query: String,
    #[serde(default = "default_answer_limit")]
    pub search_limit: usize,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub fuzzy: bool,
    #[serde(default = "default_true")]
    pub stream: bool,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub system_prompt: Option<String>,
}

fn default_answer_limit() -> usize {
    5
}

#[derive(Debug, Serialize)]
pub struct AnswerResponse {
    pub answer: String,
    pub model: String,
    pub search_took_ms: f64,
    pub llm_took_ms: f64,
    pub total_took_ms: f64,
    pub sources: Vec<SearchHit>,
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

/// Synonym group - all terms in the group are treated as equivalent
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SynonymGroup {
    /// List of terms that are synonyms of each other
    pub terms: Vec<String>,
}

/// Request to add synonyms to an index
#[derive(Debug, Serialize, Deserialize)]
pub struct AddSynonymsRequest {
    /// List of synonym groups
    pub synonyms: Vec<SynonymGroup>,
}

/// Response for synonym operations
#[derive(Debug, Serialize)]
pub struct SynonymsResponse {
    pub synonyms: Vec<SynonymGroup>,
}

/// Pinned result rule - promote specific documents for specific queries
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PinnedRule {
    /// Query terms that trigger this rule (case-insensitive, matches if query contains any term)
    pub queries: Vec<String>,
    /// Document IDs to pin to the top (in order)
    pub document_ids: Vec<String>,
}

/// Request to add pinned rules to an index
#[derive(Debug, Serialize, Deserialize)]
pub struct AddPinnedRulesRequest {
    /// List of pinned rules
    pub rules: Vec<PinnedRule>,
}

/// Response for pinned rules operations
#[derive(Debug, Serialize)]
pub struct PinnedRulesResponse {
    pub rules: Vec<PinnedRule>,
}
