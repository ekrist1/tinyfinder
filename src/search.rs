use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, RwLock};
use tantivy::aggregation::agg_req::Aggregations;
use tantivy::aggregation::agg_result::AggregationResults;
use tantivy::aggregation::AggregationCollector;
use tantivy::collector::TopDocs;
use tantivy::query::{
    BooleanQuery, ExistsQuery, FuzzyTermQuery, Occur, Query, QueryParser, RegexPhraseQuery,
    RegexQuery, TermSetQuery,
};
use tantivy::schema::*;
use tantivy::tokenizer::{LowerCaser, SimpleTokenizer, Stemmer, TextAnalyzer};
use tantivy::{Index, IndexWriter, Order, ReloadPolicy, TantivyDocument, Term};

use crate::models::{
    AggregationRequest, Document, FieldConfig, FieldStats, HighlightOptions, IndexStats,
    PinnedRule, SearchHit, SortOption, SortOrder, SynonymGroup,
};

/// Default index writer memory budget (100MB)
const DEFAULT_INDEX_WRITER_MEMORY: usize = 100_000_000;

/// Check if a word is a boolean operator (for query parsing)
fn is_operator(word: &str) -> bool {
    matches!(word.to_uppercase().as_str(), "AND" | "OR" | "NOT" | "TO")
}

pub type SearchResult = Result<(Vec<SearchHit>, usize, f64, Option<AggregationResults>)>;

pub struct SearchEngine {
    base_path: String,
    indices: Arc<RwLock<HashMap<String, IndexHandle>>>,
    /// Synonyms stored per index: index_name -> list of synonym groups
    synonyms: Arc<RwLock<HashMap<String, Vec<SynonymGroup>>>>,
    /// Pinned rules stored per index: index_name -> list of pinned rules
    pinned_rules: Arc<RwLock<HashMap<String, Vec<PinnedRule>>>>,
}

pub struct IndexHandle {
    pub index: Index,
    pub schema: Schema,
    pub writer: Arc<RwLock<IndexWriter>>,
    pub field_map: HashMap<String, Field>,
    pub field_configs: Vec<FieldConfig>,
}

impl SearchEngine {
    pub fn new(base_path: &str) -> Result<Self> {
        std::fs::create_dir_all(base_path)?;

        // Load synonyms from file if exists
        let synonyms_path = Path::new(base_path).join("synonyms.json");
        let synonyms: HashMap<String, Vec<SynonymGroup>> = if synonyms_path.exists() {
            let content = std::fs::read_to_string(&synonyms_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            HashMap::new()
        };

        // Load pinned rules from file if exists
        let pinned_path = Path::new(base_path).join("pinned_rules.json");
        let pinned_rules: HashMap<String, Vec<PinnedRule>> = if pinned_path.exists() {
            let content = std::fs::read_to_string(&pinned_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            HashMap::new()
        };

        Ok(Self {
            base_path: base_path.to_string(),
            indices: Arc::new(RwLock::new(HashMap::new())),
            synonyms: Arc::new(RwLock::new(synonyms)),
            pinned_rules: Arc::new(RwLock::new(pinned_rules)),
        })
    }

    /// Save pinned rules to disk
    fn save_pinned_rules(&self) -> Result<()> {
        let rules = self.pinned_rules.read().unwrap();
        let pinned_path = Path::new(&self.base_path).join("pinned_rules.json");
        let content = serde_json::to_string_pretty(&*rules)?;
        std::fs::write(pinned_path, content)?;
        Ok(())
    }

    /// Add pinned rules for an index
    pub fn add_pinned_rules(&self, index_name: &str, rules: Vec<PinnedRule>) -> Result<()> {
        let mut pinned = self.pinned_rules.write().unwrap();
        let entry = pinned.entry(index_name.to_string()).or_default();
        entry.extend(rules);
        drop(pinned);
        self.save_pinned_rules()?;
        Ok(())
    }

    /// Get pinned rules for an index
    pub fn get_pinned_rules(&self, index_name: &str) -> Vec<PinnedRule> {
        let rules = self.pinned_rules.read().unwrap();
        rules.get(index_name).cloned().unwrap_or_default()
    }

    /// Clear all pinned rules for an index
    pub fn clear_pinned_rules(&self, index_name: &str) -> Result<()> {
        let mut rules = self.pinned_rules.write().unwrap();
        rules.remove(index_name);
        drop(rules);
        self.save_pinned_rules()?;
        Ok(())
    }

    /// Get pinned document IDs for a query
    fn get_pinned_doc_ids(&self, index_name: &str, query_str: &str) -> Vec<String> {
        let rules = self.pinned_rules.read().unwrap();
        let query_lower = query_str.to_lowercase();
        
        if let Some(index_rules) = rules.get(index_name) {
            for rule in index_rules {
                // Check if query matches any of the trigger terms
                for trigger in &rule.queries {
                    if query_lower.contains(&trigger.to_lowercase()) {
                        return rule.document_ids.clone();
                    }
                }
            }
        }
        
        Vec::new()
    }

    /// Save synonyms to disk
    fn save_synonyms(&self) -> Result<()> {
        let synonyms = self.synonyms.read().unwrap();
        let synonyms_path = Path::new(&self.base_path).join("synonyms.json");
        let content = serde_json::to_string_pretty(&*synonyms)?;
        std::fs::write(synonyms_path, content)?;
        Ok(())
    }

    /// Add synonyms for an index
    pub fn add_synonyms(&self, index_name: &str, synonym_groups: Vec<SynonymGroup>) -> Result<()> {
        let mut synonyms = self.synonyms.write().unwrap();
        let entry = synonyms.entry(index_name.to_string()).or_default();
        entry.extend(synonym_groups);
        drop(synonyms);
        self.save_synonyms()?;
        Ok(())
    }

    /// Get synonyms for an index
    pub fn get_synonyms(&self, index_name: &str) -> Vec<SynonymGroup> {
        let synonyms = self.synonyms.read().unwrap();
        synonyms.get(index_name).cloned().unwrap_or_default()
    }

    /// Clear all synonyms for an index
    pub fn clear_synonyms(&self, index_name: &str) -> Result<()> {
        let mut synonyms = self.synonyms.write().unwrap();
        synonyms.remove(index_name);
        drop(synonyms);
        self.save_synonyms()?;
        Ok(())
    }

    /// Expand a query term with its synonyms
    fn expand_with_synonyms(&self, index_name: &str, term: &str) -> Vec<String> {
        let synonyms = self.synonyms.read().unwrap();
        let term_lower = term.to_lowercase();
        
        if let Some(groups) = synonyms.get(index_name) {
            for group in groups {
                // Check if this term is in any synonym group
                if group.terms.iter().any(|t| t.to_lowercase() == term_lower) {
                    // Return all terms in the group (including the original)
                    return group.terms.iter()
                        .map(|t| t.to_lowercase())
                        .collect();
                }
            }
        }
        
        // No synonyms found, return just the original term
        vec![term_lower]
    }

    /// Expand a full query string with synonyms
    fn expand_query_with_synonyms(&self, index_name: &str, query_str: &str) -> String {
        // Simple tokenization - split on whitespace and handle quoted phrases
        let mut result = String::new();
        let mut in_quotes = false;
        let mut current_word = String::new();
        
        for ch in query_str.chars() {
            if ch == '"' {
                in_quotes = !in_quotes;
                result.push(ch);
            } else if ch.is_whitespace() && !in_quotes {
                if !current_word.is_empty() {
                    // Check if this is an operator or special syntax
                    if is_operator(&current_word) 
                        || current_word.contains(':') 
                        || current_word.contains('*')
                        || current_word.contains('?') 
                    {
                        result.push_str(&current_word);
                    } else {
                        // Expand with synonyms
                        let expanded = self.expand_with_synonyms(index_name, &current_word);
                        if expanded.len() > 1 {
                            // Multiple synonyms - wrap in parentheses with OR
                            result.push('(');
                            result.push_str(&expanded.join(" OR "));
                            result.push(')');
                        } else {
                            result.push_str(&expanded[0]);
                        }
                    }
                    current_word.clear();
                }
                result.push(ch);
            } else {
                current_word.push(ch);
            }
        }
        
        // Handle last word
        if !current_word.is_empty() {
            if is_operator(&current_word) 
                || current_word.contains(':') 
                || current_word.contains('*')
                || current_word.contains('?') 
            {
                result.push_str(&current_word);
            } else {
                let expanded = self.expand_with_synonyms(index_name, &current_word);
                if expanded.len() > 1 {
                    result.push('(');
                    result.push_str(&expanded.join(" OR "));
                    result.push(')');
                } else {
                    result.push_str(&expanded[0]);
                }
            }
        }
        
        result
    }

    pub fn load_indices(&self) -> Result<Vec<String>> {
        let mut loaded = Vec::new();
        let base_path = Path::new(&self.base_path);

        if !base_path.exists() {
            return Ok(loaded);
        }

        for entry in std::fs::read_dir(base_path)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }

            let index_name = entry.file_name().to_string_lossy().to_string();
            let index_path = entry.path();

            match Index::open_in_dir(&index_path) {
                Ok(index) => {
                    Self::register_analyzers(&index);
                    let schema = index.schema();
                    let field_map = schema
                        .fields()
                        .map(|(field, field_entry)| (field_entry.name().to_string(), field))
                        .collect::<HashMap<_, _>>();
                    let field_configs = Self::field_configs_from_schema(&schema);

                    match index.writer(DEFAULT_INDEX_WRITER_MEMORY) {
                        Ok(writer) => {
                            let handle = IndexHandle {
                                index,
                                schema,
                                writer: Arc::new(RwLock::new(writer)),
                                field_map,
                                field_configs,
                            };

                            match self.indices.write() {
                                Ok(mut indices) => {
                                    indices.insert(index_name.clone(), handle);
                                    loaded.push(index_name);
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Failed to acquire write lock for index '{}': {}",
                                        index_name,
                                        e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to create writer for index '{}': {}",
                                index_name,
                                e
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to load index from {}: {}",
                        index_path.display(),
                        e
                    );
                }
            }
        }

        Ok(loaded)
    }

    pub fn collect_document_ids(&self, index_name: &str) -> Result<Vec<String>> {
        let indices = self.indices.read()
            .map_err(|e| anyhow!("Failed to acquire read lock: {}", e))?;
        let handle = indices
            .get(index_name)
            .ok_or_else(|| anyhow!("Index not found: {}", index_name))?;

        let reader = handle
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        let searcher = reader.searcher();

        let id_field = *handle
            .field_map
            .get("id")
            .ok_or_else(|| anyhow!("ID field not found for index: {}", index_name))?;

        let mut ids = Vec::new();

        for segment_reader in searcher.segment_readers() {
            let store_reader = segment_reader.get_store_reader(0)?;
            let max_doc = segment_reader.max_doc();
            let alive_bitset = segment_reader.alive_bitset();

            for doc_id in 0..max_doc {
                if let Some(bitset) = alive_bitset {
                    if !bitset.is_alive(doc_id) {
                        continue;
                    }
                }

                let doc: TantivyDocument = store_reader.get(doc_id)?;
                let id_value = {
                    let mut values = doc.get_all(id_field);
                    values.next().and_then(|field_value| {
                        let owned_value: tantivy::schema::OwnedValue = field_value.into();
                        if let tantivy::schema::OwnedValue::Str(s) = owned_value {
                            Some(s.to_string())
                        } else {
                            None
                        }
                    })
                };

                if let Some(id) = id_value {
                    ids.push(id);
                }
            }
        }

        Ok(ids)
    }

    fn field_configs_from_schema(schema: &Schema) -> Vec<FieldConfig> {
        let mut configs = Vec::new();

        for (_field, entry) in schema.fields() {
            let name = entry.name();
            if name == "id" {
                continue;
            }

            match entry.field_type() {
                FieldType::Str(options) => {
                    let indexing = options.get_indexing_options();
                    let indexed = indexing.is_some();
                    let stored = options.is_stored();

                    let (field_type, analyzer) = if let Some(indexing) = indexing {
                        let tokenizer = indexing.tokenizer().to_string();
                        let index_option = indexing.index_option();
                        let is_string = tokenizer == "raw" && index_option == IndexRecordOption::Basic;
                        (
                            if is_string { "string" } else { "text" },
                            tokenizer,
                        )
                    } else {
                        ("text", "default".to_string())
                    };

                    configs.push(FieldConfig {
                        name: name.to_string(),
                        field_type: field_type.to_string(),
                        stored,
                        indexed,
                        analyzer,
                        fast: false,
                    });
                }
                FieldType::I64(options) => {
                    configs.push(FieldConfig {
                        name: name.to_string(),
                        field_type: "i64".to_string(),
                        stored: options.is_stored(),
                        indexed: options.is_indexed(),
                        analyzer: "default".to_string(),
                        fast: options.is_fast(),
                    });
                }
                FieldType::F64(options) => {
                    configs.push(FieldConfig {
                        name: name.to_string(),
                        field_type: "f64".to_string(),
                        stored: options.is_stored(),
                        indexed: options.is_indexed(),
                        analyzer: "default".to_string(),
                        fast: options.is_fast(),
                    });
                }
                FieldType::Date(options) => {
                    configs.push(FieldConfig {
                        name: name.to_string(),
                        field_type: "date".to_string(),
                        stored: options.is_stored(),
                        indexed: options.is_indexed(),
                        analyzer: "default".to_string(),
                        fast: options.is_fast(),
                    });
                }
                FieldType::JsonObject(options) => {
                    configs.push(FieldConfig {
                        name: name.to_string(),
                        field_type: "json".to_string(),
                        stored: options.is_stored(),
                        indexed: options.get_text_indexing_options().is_some(),
                        analyzer: "default".to_string(),
                        fast: options.is_expand_dots_enabled(),
                    });
                }
                _ => {}
            }
        }

        configs
    }

    fn register_analyzers(index: &Index) {
        // Register Norwegian analyzer with stemming
        let norwegian = TextAnalyzer::builder(SimpleTokenizer::default())
            .filter(LowerCaser)
            .filter(Stemmer::new(tantivy::tokenizer::Language::Norwegian))
            .build();
        index.tokenizers().register("norwegian", norwegian);

        // Register raw analyzer (no tokenization)
        let raw = TextAnalyzer::builder(tantivy::tokenizer::RawTokenizer::default()).build();
        index.tokenizers().register("raw", raw);
    }

    pub fn create_index(&self, name: &str, fields: &[FieldConfig]) -> Result<()> {
        let mut schema_builder = Schema::builder();
        let mut field_map = HashMap::new();

        // Always add an ID field
        let id_field = schema_builder.add_text_field("id", STRING | STORED);
        field_map.insert("id".to_string(), id_field);

        // Add custom fields
        for field_config in fields {
            let field = match field_config.field_type.as_str() {
                "text" => {
                    let mut options = TextOptions::default();
                    if field_config.stored {
                        options = options.set_stored();
                    }
                    if field_config.indexed {
                        let tokenizer = match field_config.analyzer.as_str() {
                            "norwegian" => "norwegian",
                            "raw" => "raw",
                            _ => "default",
                        };
                        options = options.set_indexing_options(
                            TextFieldIndexing::default()
                                .set_tokenizer(tokenizer)
                                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                        );
                    }
                    schema_builder.add_text_field(&field_config.name, options)
                }
                "string" => {
                    let options = if field_config.indexed {
                        STRING | STORED
                    } else {
                        TextOptions::default().set_stored()
                    };
                    schema_builder.add_text_field(&field_config.name, options)
                }
                "i64" => {
                    let mut options = NumericOptions::default();
                    if field_config.stored {
                        options = options.set_stored();
                    }
                    if field_config.indexed {
                        options = options.set_indexed();
                    }
                    if field_config.fast {
                        options = options.set_fast();
                    }
                    schema_builder.add_i64_field(&field_config.name, options)
                }
                "f64" => {
                    let mut options = NumericOptions::default();
                    if field_config.stored {
                        options = options.set_stored();
                    }
                    if field_config.indexed {
                        options = options.set_indexed();
                    }
                    if field_config.fast {
                        options = options.set_fast();
                    }
                    schema_builder.add_f64_field(&field_config.name, options)
                }
                "date" => {
                    let mut options = DateOptions::default()
                        .set_precision(tantivy::schema::DateTimePrecision::Seconds);
                    if field_config.stored {
                        options = options.set_stored();
                    }
                    if field_config.indexed {
                        options = options.set_indexed();
                    }
                    if field_config.fast {
                        options = options.set_fast();
                    }
                    schema_builder.add_date_field(&field_config.name, options)
                }
                "json" => {
                    // JSON field for dynamic/schemaless data
                    let mut options = JsonObjectOptions::default();
                    if field_config.stored {
                        options = options.set_stored();
                    }
                    if field_config.indexed {
                        options = options.set_indexing_options(
                            TextFieldIndexing::default()
                                .set_tokenizer("default")
                                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                        );
                    }
                    if field_config.fast {
                        options = options.set_expand_dots_enabled();
                    }
                    schema_builder.add_json_field(&field_config.name, options)
                }
                _ => {
                    return Err(anyhow!(
                        "Unsupported field type: {}",
                        field_config.field_type
                    ));
                }
            };
            field_map.insert(field_config.name.clone(), field);
        }

        let schema = schema_builder.build();
        let index_path = Path::new(&self.base_path).join(name);
        std::fs::create_dir_all(&index_path)?;

        let index = Index::create_in_dir(&index_path, schema.clone())?;

        // Register custom analyzers
        Self::register_analyzers(&index);

        let writer = index.writer(DEFAULT_INDEX_WRITER_MEMORY)?;

        let handle = IndexHandle {
            index,
            schema,
            writer: Arc::new(RwLock::new(writer)),
            field_map,
            field_configs: fields.to_vec(),
        };

        self.indices
            .write()
            .unwrap()
            .insert(name.to_string(), handle);

        Ok(())
    }

    pub fn add_documents(&self, index_name: &str, documents: &[Document]) -> Result<()> {
        let indices = self.indices.read().unwrap();
        let handle = indices
            .get(index_name)
            .ok_or_else(|| anyhow!("Index not found: {}", index_name))?;

        let mut writer = handle.writer.write().unwrap();

        for doc in documents {
            let mut tantivy_doc = TantivyDocument::default();

            // Add ID field
            let id_field = handle.field_map.get("id").unwrap();
            tantivy_doc.add_text(*id_field, &doc.id);

            // Add custom fields
            for (field_name, value) in &doc.fields {
                if let Some(field) = handle.field_map.get(field_name) {
                    // Get field config to check type
                    let field_type = handle
                        .field_configs
                        .iter()
                        .find(|fc| fc.name == *field_name)
                        .map(|fc| fc.field_type.as_str())
                        .unwrap_or("text");

                    match field_type {
                        "date" => {
                            // Parse date from RFC3339 string or Unix timestamp
                            if let Some(date_str) = value.as_str() {
                                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(date_str) {
                                    let tantivy_dt =
                                        tantivy::DateTime::from_timestamp_secs(dt.timestamp());
                                    tantivy_doc.add_date(*field, tantivy_dt);
                                }
                            } else if let Some(ts) = value.as_i64() {
                                let tantivy_dt = tantivy::DateTime::from_timestamp_secs(ts);
                                tantivy_doc.add_date(*field, tantivy_dt);
                            }
                        }
                        "json" => {
                            // JSON field - convert serde_json::Value to OwnedValue
                            use tantivy::schema::OwnedValue;
                            let owned_value = OwnedValue::from(value.clone());
                            tantivy_doc.add_field_value(*field, &owned_value);
                        }
                        _ => match value {
                            serde_json::Value::String(s) => {
                                tantivy_doc.add_text(*field, s);
                            }
                            serde_json::Value::Number(n) => {
                                if let Some(i) = n.as_i64() {
                                    tantivy_doc.add_i64(*field, i);
                                } else if let Some(f) = n.as_f64() {
                                    tantivy_doc.add_f64(*field, f);
                                }
                            }
                            serde_json::Value::Bool(b) => {
                                tantivy_doc.add_i64(*field, if *b { 1 } else { 0 });
                            }
                            _ => {}
                        },
                    }
                }
            }

            writer.add_document(tantivy_doc)?;
        }

        writer.commit()?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)]
    pub fn search(
        &self,
        index_name: &str,
        query_str: &str,
        limit: usize,
        offset: usize,
        fields: &[String],
        highlight_options: Option<&HighlightOptions>,
        aggregations: &[AggregationRequest],
    ) -> SearchResult {
        self.search_internal(
            index_name,
            query_str,
            limit,
            offset,
            fields,
            highlight_options,
            aggregations,
            false,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn search_with_options(
        &self,
        index_name: &str,
        query_str: &str,
        limit: usize,
        offset: usize,
        fields: &[String],
        highlight_options: Option<&HighlightOptions>,
        aggregations: &[AggregationRequest],
        fuzzy: bool,
        sort: Option<&SortOption>,
        minimum_should_match: Option<usize>,
    ) -> SearchResult {
        self.search_internal(
            index_name,
            query_str,
            limit,
            offset,
            fields,
            highlight_options,
            aggregations,
            fuzzy,
            sort,
            minimum_should_match,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn search_internal(
        &self,
        index_name: &str,
        query_str: &str,
        limit: usize,
        offset: usize,
        fields: &[String],
        highlight_options: Option<&HighlightOptions>,
        aggregations: &[AggregationRequest],
        fuzzy: bool,
        sort: Option<&SortOption>,
        minimum_should_match: Option<usize>,
    ) -> SearchResult {
        let start = std::time::Instant::now();

        // Get pinned document IDs for this query BEFORE synonym expansion
        // (we want to match on the original user query)
        let pinned_ids = self.get_pinned_doc_ids(index_name, query_str);
        let pinned_count = pinned_ids.len();

        // Expand query with synonyms before processing
        let expanded_query = self.expand_query_with_synonyms(index_name, query_str);
        let query_str = expanded_query.as_str();

        let indices = self.indices.read().unwrap();
        let handle = indices
            .get(index_name)
            .ok_or_else(|| anyhow!("Index not found: {}", index_name))?;

        let reader = handle
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        let searcher = reader.searcher();

        // Build query parser for specified fields or all text fields
        let query_fields: Vec<Field> = if fields.is_empty() {
            // Only include text fields in the default query parser to avoid parse errors
            handle
                .field_map
                .iter()
                .filter(|(_, field)| {
                    matches!(
                        handle.schema.get_field_entry(**field).field_type(),
                        FieldType::Str(_)
                    )
                })
                .map(|(_, field)| *field)
                .collect()
        } else {
            fields
                .iter()
                .filter_map(|f| handle.field_map.get(f).copied())
                .collect()
        };

        let mut query = Self::build_query(handle, query_str, &query_fields, fuzzy)?;

        // Apply minimum_should_match if specified
        // This wraps the query in a BooleanQuery with the minimum_should_match setting
        if let Some(min_match) = minimum_should_match {
            if min_match > 0 {
                let mut bool_query = BooleanQuery::from(vec![(Occur::Should, query)]);
                bool_query.set_minimum_number_should_match(min_match);
                query = Box::new(bool_query);
            }
        }

        // Get total document count that matches the query
        let mut total = searcher.search(query.as_ref(), &tantivy::collector::Count)?;

        // Fallback: if no hits, try a keyword-only query (removes question/stop words)
        if total == 0 {
            if let Some(fallback_query) = Self::fallback_query_string(query_str) {
                if fallback_query != query_str {
                    let fallback = Self::build_query(handle, &fallback_query, &query_fields, fuzzy)?;
                    let fallback_total = searcher.search(fallback.as_ref(), &tantivy::collector::Count)?;
                    if fallback_total > 0 {
                        query = fallback;
                        total = fallback_total;
                    }
                }
            }
        }

        let mut hits = Vec::new();
        let mut add_hit = |score: f32, doc_address: tantivy::DocAddress| -> Result<()> {
            let retrieved_doc: TantivyDocument = searcher.doc(doc_address)?;
            let mut field_values = HashMap::new();

            for (field_name, field) in &handle.field_map {
                if let Some(field_value) = retrieved_doc.get_all(*field).next() {
                    let owned_value: tantivy::schema::OwnedValue = field_value.into();
                    let value = match owned_value {
                        tantivy::schema::OwnedValue::Str(s) => {
                            serde_json::Value::String(s.to_string())
                        }
                        tantivy::schema::OwnedValue::U64(n) => serde_json::json!(n),
                        tantivy::schema::OwnedValue::I64(n) => serde_json::json!(n),
                        tantivy::schema::OwnedValue::F64(n) => serde_json::json!(n),
                        tantivy::schema::OwnedValue::Date(d) => {
                            serde_json::Value::String(d.into_utc().to_string())
                        }
                        _ => continue,
                    };
                    field_values.insert(field_name.clone(), value);
                }
            }

            // Generate highlights if requested
            let highlights = if let Some(opts) = highlight_options {
                if opts.enabled {
                    let mut highlight_map = HashMap::new();
                    let highlight_fields: Vec<&String> = if opts.fields.is_empty() {
                        query_fields
                            .iter()
                            .filter_map(|f| {
                                handle.field_map.iter().find_map(|(name, field)| {
                                    if field == f {
                                        Some(name)
                                    } else {
                                        None
                                    }
                                })
                            })
                            .collect()
                    } else {
                        opts.fields.iter().collect()
                    };

                    for field_name in highlight_fields {
                        if let Some(field) = handle.field_map.get(field_name) {
                            // Check if this is a text field
                            let field_entry = handle.schema.get_field_entry(*field);
                            if let FieldType::Str(_) = field_entry.field_type() {
                                if let Ok(snippet_gen) = tantivy::snippet::SnippetGenerator::create(
                                    &searcher,
                                    query.as_ref(),
                                    *field,
                                ) {
                                    let mut snippet = snippet_gen.snippet_from_doc(&retrieved_doc);
                                    // Use custom highlight tags via the Snippet method
                                    snippet.set_snippet_prefix_postfix(&opts.pre_tag, &opts.post_tag);
                                    let highlighted = snippet.to_html();
                                    if !highlighted.is_empty() {
                                        highlight_map.insert(field_name.clone(), vec![highlighted]);
                                    }
                                }
                            }
                        }
                    }
                    if highlight_map.is_empty() {
                        None
                    } else {
                        Some(highlight_map)
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let id = field_values
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            hits.push(SearchHit {
                id,
                score,
                fields: field_values,
                highlights,
            });

            Ok(())
        };

        if let Some(sort) = sort {
            let field_name = sort.field.as_str();
            let _field = handle
                .field_map
                .get(field_name)
                .ok_or_else(|| anyhow!("Sort field not found: {}", field_name))?;
            let field_config = handle
                .field_configs
                .iter()
                .find(|fc| fc.name == field_name)
                .ok_or_else(|| anyhow!("Sort field not found: {}", field_name))?;
            if !field_config.fast {
                return Err(anyhow!(
                    "Sort field '{}' must be configured with fast: true",
                    field_name
                ));
            }

            let order = match sort.order {
                SortOrder::Asc => Order::Asc,
                SortOrder::Desc => Order::Desc,
            };

            // Fetch extra results to ensure pinned documents are included
            let fetch_limit = limit + pinned_count;

            match field_config.field_type.as_str() {
                "i64" => {
                    let collector = TopDocs::with_limit(fetch_limit)
                        .and_offset(offset)
                        .order_by_fast_field::<i64>(field_name, order);
                    let top_docs = searcher.search(query.as_ref(), &collector)?;
                    for (_sort_value, doc_address) in top_docs {
                        let score = query
                            .explain(&searcher, doc_address)
                            .map(|e| e.value())
                            .unwrap_or(0.0);
                        add_hit(score, doc_address)?;
                    }
                }
                "f64" => {
                    let collector = TopDocs::with_limit(fetch_limit)
                        .and_offset(offset)
                        .order_by_fast_field::<f64>(field_name, order);
                    let top_docs = searcher.search(query.as_ref(), &collector)?;
                    for (_sort_value, doc_address) in top_docs {
                        let score = query
                            .explain(&searcher, doc_address)
                            .map(|e| e.value())
                            .unwrap_or(0.0);
                        add_hit(score, doc_address)?;
                    }
                }
                "date" => {
                    let collector = TopDocs::with_limit(fetch_limit)
                        .and_offset(offset)
                        .order_by_fast_field::<tantivy::DateTime>(field_name, order);
                    let top_docs = searcher.search(query.as_ref(), &collector)?;
                    for (_sort_value, doc_address) in top_docs {
                        let score = query
                            .explain(&searcher, doc_address)
                            .map(|e| e.value())
                            .unwrap_or(0.0);
                        add_hit(score, doc_address)?;
                    }
                }
                _ => {
                    return Err(anyhow!(
                        "Sorting is only supported on fast i64, f64, date, or string fields. Field '{}' is type '{}'.",
                        field_name,
                        field_config.field_type
                    ));
                }
            }
        } else {
            // Fetch extra results to ensure pinned documents are included
            let fetch_limit = offset + limit + pinned_count;
            let top_docs = searcher.search(query.as_ref(), &TopDocs::with_limit(fetch_limit))?;
            for (score, doc_address) in top_docs.into_iter().skip(offset) {
                add_hit(score, doc_address)?;
            }
        }

        // Process aggregations using Tantivy's built-in AggregationCollector
        let agg_results = if !aggregations.is_empty() {
            match Self::build_aggregation_request(aggregations) {
                Ok(agg_req) => {
                    let collector = AggregationCollector::from_aggs(agg_req, Default::default());
                    match searcher.search(query.as_ref(), &collector) {
                        Ok(results) => Some(results),
                        Err(e) => {
                            tracing::warn!("Aggregation failed: {}", e);
                            None
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to build aggregation request: {}", e);
                    None
                }
            }
        } else {
            None
        };

        let took_ms = start.elapsed().as_secs_f64() * 1000.0;

        // Reorder hits based on pinned rules and truncate to requested limit
        let hits = self.apply_pinned_results(&pinned_ids, hits, limit);

        Ok((hits, total, took_ms, agg_results))
    }

    /// Apply pinned results - move pinned documents to the top in the specified order
    /// and truncate to the requested limit
    fn apply_pinned_results(
        &self,
        pinned_ids: &[String],
        mut hits: Vec<SearchHit>,
        limit: usize,
    ) -> Vec<SearchHit> {
        if pinned_ids.is_empty() {
            // No pinned rules, just truncate to limit
            hits.truncate(limit);
            return hits;
        }

        // Extract pinned hits from the result set (maintain pinned order)
        let mut pinned_hits: Vec<SearchHit> = Vec::new();
        let mut remaining_hits: Vec<SearchHit> = Vec::new();

        // Create a set of pinned IDs for quick lookup
        let pinned_set: std::collections::HashSet<&String> = pinned_ids.iter().collect();

        // Separate pinned and non-pinned hits
        for hit in hits.drain(..) {
            if pinned_set.contains(&hit.id) {
                pinned_hits.push(hit);
            } else {
                remaining_hits.push(hit);
            }
        }

        // Sort pinned hits according to the order in pinned_ids
        pinned_hits.sort_by(|a, b| {
            let pos_a = pinned_ids.iter().position(|id| id == &a.id).unwrap_or(usize::MAX);
            let pos_b = pinned_ids.iter().position(|id| id == &b.id).unwrap_or(usize::MAX);
            pos_a.cmp(&pos_b)
        });

        // Combine: pinned first, then remaining
        pinned_hits.extend(remaining_hits);
        
        // Truncate to the requested limit
        pinned_hits.truncate(limit);
        pinned_hits
    }

    fn build_query(
        handle: &IndexHandle,
        query_str: &str,
        query_fields: &[Field],
        fuzzy: bool,
    ) -> Result<Box<dyn Query>> {
        // Preprocess field grouping syntax: title:(foo AND bar) -> (title:foo AND title:bar)
        let query_str = Self::expand_field_grouping(query_str);
        let query_str = query_str.as_str();
        
        let query_parser = QueryParser::for_index(&handle.index, query_fields.to_vec());
        
        // Check for _exists_ query (e.g., "_exists_:field_name")
        if let Some(field_name) = query_str.strip_prefix("_exists_:") {
            let field_name = field_name.trim();
            if handle.field_map.contains_key(field_name) {
                // ExistsQuery::new(field_name, json_subpaths) - second param enables JSON subpath matching
                return Ok(Box::new(ExistsQuery::new(field_name.to_string(), false)));
            } else {
                return Err(anyhow!("Field not found for exists query: {}", field_name));
            }
        }
        
        // Check for TermSetQuery syntax: field:IN[term1,term2,term3]
        // This is more efficient than field:term1 OR field:term2 OR field:term3
        if let Some(in_pos) = query_str.find(":IN[") {
            let field_name = &query_str[..in_pos];
            if let Some(field) = handle.field_map.get(field_name) {
                // Find closing bracket
                if let Some(close_pos) = query_str[in_pos..].find(']') {
                    let terms_str = &query_str[in_pos + 4..in_pos + close_pos];
                    let terms: Vec<Term> = terms_str
                        .split(',')
                        .map(|t| t.trim())
                        .filter(|t| !t.is_empty())
                        .map(|t| Term::from_field_text(*field, t))
                        .collect();
                    
                    if !terms.is_empty() {
                        return Ok(Box::new(TermSetQuery::new(terms)));
                    }
                }
            }
        }
        
        // Check if the query contains wildcards (* or ?)
        let has_wildcard = query_str.chars().any(|ch| matches!(ch, '*' | '?'));
        
        // Check if this is a phrase query with wildcards (e.g., "b.* b.* wolf")
        // RegexPhraseQuery handles multi-term wildcard phrase searches
        if has_wildcard && query_str.starts_with('"') && query_str.ends_with('"') {
            let phrase_content = &query_str[1..query_str.len() - 1];
            let query_lower = phrase_content.to_lowercase();
            
            // Split into terms and convert each to regex pattern
            let terms: Vec<String> = query_lower
                .split_whitespace()
                .map(|term| {
                    // Convert wildcard to regex: * -> .*, ? -> .
                    term.chars()
                        .map(|c| match c {
                            '*' => ".*".to_string(),
                            '?' => ".".to_string(),
                            '.' | '+' | '^' | '$' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\' => {
                                format!("\\{}", c)
                            }
                            _ => c.to_string(),
                        })
                        .collect::<String>()
                })
                .collect();
            
            // Need at least 2 terms for a phrase query
            if terms.len() >= 2 {
                let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
                
                for field in query_fields {
                    let field_entry = handle.schema.get_field_entry(*field);
                    if matches!(field_entry.field_type(), FieldType::Str(_)) {
                        let regex_phrase_query = RegexPhraseQuery::new(*field, terms.clone());
                        clauses.push((Occur::Should, Box::new(regex_phrase_query)));
                    }
                }
                
                if !clauses.is_empty() {
                    return Ok(if clauses.len() == 1 {
                        clauses.into_iter().next().unwrap().1
                    } else {
                        Box::new(BooleanQuery::from(clauses))
                    });
                }
            }
        }
        
        // For non-phrase wildcard queries, we use RegexQuery
        // because Tantivy's default QueryParser doesn't support single-term wildcards
        if has_wildcard {
            // Convert wildcard syntax to regex syntax
            // * becomes .* and ? becomes .
            // Also lowercase the query to match indexed (lowercased) terms
            let query_lower = query_str.to_lowercase();
            
            // Check if it's a field-specific query like "title:eventyr*"
            let (target_fields, pattern) = if let Some(colon_pos) = query_lower.find(':') {
                let field_name = &query_lower[..colon_pos];
                let pattern_part = &query_lower[colon_pos + 1..];
                
                // Find the matching field
                let target_field = handle.field_map.get(field_name).copied();
                let fields = if let Some(f) = target_field {
                    vec![f]
                } else {
                    // Field not found, use default fields
                    query_fields.to_vec()
                };
                (fields, pattern_part.to_string())
            } else {
                (query_fields.to_vec(), query_lower)
            };
            
            // Convert wildcard pattern to regex pattern
            // Escape regex special chars first, then convert wildcards
            let regex_pattern = pattern
                .chars()
                .map(|c| match c {
                    '*' => ".*".to_string(),
                    '?' => ".".to_string(),
                    // Escape regex special characters
                    '.' | '+' | '^' | '$' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\' => {
                        format!("\\{}", c)
                    }
                    _ => c.to_string(),
                })
                .collect::<String>();
            
            // Create regex queries for each target field
            let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
            for field in &target_fields {
                // Only create regex queries for text fields
                let field_entry = handle.schema.get_field_entry(*field);
                if matches!(field_entry.field_type(), FieldType::Str(_)) {
                    if let Ok(regex_query) = RegexQuery::from_pattern(&regex_pattern, *field) {
                        clauses.push((Occur::Should, Box::new(regex_query)));
                    }
                }
            }
            
            if !clauses.is_empty() {
                let wildcard_query: Box<dyn Query> = if clauses.len() == 1 {
                    clauses.into_iter().next().unwrap().1
                } else {
                    Box::new(BooleanQuery::from(clauses))
                };
                
                // If fuzzy is enabled, also add fuzzy queries for the non-wildcard part
                if fuzzy {
                    // Extract the prefix (part before the first wildcard)
                    let prefix = pattern.split(['*', '?']).next().unwrap_or("");
                    if !prefix.is_empty() && prefix.len() >= 2 {
                        let mut fuzzy_clauses: Vec<(Occur, Box<dyn Query>)> = vec![
                            (Occur::Should, wildcard_query)
                        ];
                        
                        for field in &target_fields {
                            let field_entry = handle.schema.get_field_entry(*field);
                            if matches!(field_entry.field_type(), FieldType::Str(_)) {
                                let term = Term::from_field_text(*field, prefix);
                                fuzzy_clauses.push((
                                    Occur::Should,
                                    Box::new(FuzzyTermQuery::new(term, 1, true))
                                ));
                            }
                        }
                        
                        return Ok(Box::new(BooleanQuery::from(fuzzy_clauses)));
                    }
                }
                
                return Ok(wildcard_query);
            }
        }
        
        // For non-wildcard queries, use the standard query parser
        let base_query = query_parser.parse_query(query_str)?;

        if !fuzzy {
            return Ok(base_query);
        }

        let tokens: Vec<&str> = query_str.split_whitespace().collect();
        if tokens.is_empty() {
            return Ok(base_query);
        }

        let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        for token in tokens.iter() {
            if token.chars().any(|ch| {
                matches!(
                    ch,
                    '*' | '?'
                        | ':'
                        | '"'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | '('
                        | ')'
                        | '!'
                        | '^'
                        | '~'
                        | '\\'
                        | '/'
                        | '<'
                        | '>'
                        | '='
                )
            }) {
                continue;
            }

            let trimmed =
                token.trim_matches(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'));
            if trimmed.is_empty() {
                continue;
            }

            let normalized = trimmed.to_lowercase();
            if normalized.is_empty() {
                continue;
            }
            if matches!(normalized.as_str(), "and" | "or" | "not") {
                continue;
            }

            let mut field_clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
            for field in query_fields {
                let term = Term::from_field_text(*field, &normalized);
                field_clauses.push((Occur::Should, Box::new(FuzzyTermQuery::new(term, 1, true))));
            }

            if !field_clauses.is_empty() {
                let clause: Box<dyn Query> = if field_clauses.len() == 1 {
                    field_clauses.into_iter().next().unwrap().1
                } else {
                    Box::new(BooleanQuery::from(field_clauses))
                };
                clauses.push((Occur::Must, clause));
            }
        }

        if clauses.is_empty() {
            return Ok(base_query);
        }

        let fuzzy_query: Box<dyn Query> = if clauses.len() == 1 {
            clauses.into_iter().next().unwrap().1
        } else {
            Box::new(BooleanQuery::from(clauses))
        };

        let combined: Vec<(Occur, Box<dyn Query>)> = vec![
            (Occur::Should, base_query),
            (Occur::Should, fuzzy_query),
        ];

        Ok(Box::new(BooleanQuery::from(combined)))
    }

    /// Expand field grouping syntax: title:(foo AND bar) -> (title:foo AND title:bar)
    /// This enables Elasticsearch-style field grouping in queries
    fn expand_field_grouping(query_str: &str) -> String {
        // Pattern: field_name:(content)
        // We need to find these and expand them
        let mut i = 0;
        let chars: Vec<char> = query_str.chars().collect();
        let mut output = String::new();
        
        while i < chars.len() {
            // Check if this could be the start of a field name
            if chars[i].is_alphanumeric() || chars[i] == '_' {
                // Collect potential field name
                let field_start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let field_name: String = chars[field_start..i].iter().collect();
                
                // Check if followed by :(
                if i + 1 < chars.len() && chars[i] == ':' && chars[i + 1] == '(' {
                    // Find matching closing parenthesis
                    let content_start = i + 2;
                    let mut depth = 1;
                    let mut content_end = content_start;
                    
                    while content_end < chars.len() && depth > 0 {
                        if chars[content_end] == '(' {
                            depth += 1;
                        } else if chars[content_end] == ')' {
                            depth -= 1;
                        }
                        content_end += 1;
                    }
                    
                    if depth == 0 {
                        // Extract the content (excluding the final closing paren)
                        let content: String = chars[content_start..content_end - 1].iter().collect();
                        
                        // Expand: add field: prefix to each term that doesn't have a field
                        let expanded = Self::add_field_prefix_to_terms(&field_name, &content);
                        output.push('(');
                        output.push_str(&expanded);
                        output.push(')');
                        i = content_end;
                        continue;
                    }
                }
                
                // Not a field grouping, output as-is
                output.push_str(&field_name);
                continue;
            }
            
            output.push(chars[i]);
            i += 1;
        }
        
        output
    }
    
    /// Add field: prefix to terms in an expression that don't already have a field prefix
    fn add_field_prefix_to_terms(field: &str, content: &str) -> String {
        // Simple tokenization: split by spaces and operators, add prefix to words
        let mut result = String::new();
        let mut current_word = String::new();
        let mut in_quotes = false;
        let mut quote_char = '"';
        
        for c in content.chars() {
            if (c == '"' || c == '\'') && !in_quotes {
                // Starting a quote - output current word and start quoted section
                if !current_word.is_empty() {
                    if !current_word.contains(':') && !is_operator(&current_word) {
                        result.push_str(field);
                        result.push(':');
                    }
                    result.push_str(&current_word);
                    current_word.clear();
                }
                in_quotes = true;
                quote_char = c;
                result.push(c);
            } else if c == quote_char && in_quotes {
                // Ending a quote
                in_quotes = false;
                // For phrases in quotes, prefix the whole quoted section
                if !current_word.is_empty() {
                    result.push_str(field);
                    result.push(':');
                    result.push(quote_char);
                    result.push_str(&current_word);
                }
                result.push(c);
                current_word.clear();
            } else if in_quotes {
                current_word.push(c);
            } else if c.is_whitespace() || c == '(' || c == ')' {
                // End of word
                if !current_word.is_empty() {
                    if !current_word.contains(':') && !is_operator(&current_word) {
                        result.push_str(field);
                        result.push(':');
                    }
                    result.push_str(&current_word);
                    current_word.clear();
                }
                result.push(c);
            } else {
                current_word.push(c);
            }
        }
        
        // Handle final word
        if !current_word.is_empty() {
            if !current_word.contains(':') && !is_operator(&current_word) {
                result.push_str(field);
                result.push(':');
            }
            result.push_str(&current_word);
        }
        
        result
    }

    fn fallback_query_string(query_str: &str) -> Option<String> {
        let stopwords: HashSet<&'static str> = [
            "hva", "hvem", "hvor", "hvilken", "hvilke", "hvordan", "når", "hvorfor",
            "what", "who", "where", "which", "how", "when", "why",
            "er", "var", "bli", "blir", "være",
            "og", "eller", "for", "av", "til", "med", "i", "på", "om", "som",
            "en", "et", "den", "det", "de", "du", "jeg", "vi", "oss",
        ]
        .into_iter()
        .collect();

        let cleaned: String = query_str
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { ' ' })
            .collect();

        let tokens: Vec<String> = cleaned
            .split_whitespace()
            .filter(|token| token.len() > 1 && !stopwords.contains(*token))
            .map(|token| token.to_string())
            .collect();

        if tokens.is_empty() {
            None
        } else {
            Some(tokens.join(" "))
        }
    }

    /// Build an Elasticsearch-compatible aggregation request from our AggregationRequest format
    fn build_aggregation_request(aggregations: &[AggregationRequest]) -> Result<Aggregations> {
        let mut agg_map = serde_json::Map::new();

        for agg_req in aggregations {
            let agg_def = match agg_req.agg_type.as_str() {
                "terms" => {
                    let mut terms = serde_json::json!({
                        "field": agg_req.field
                    });
                    if let Some(size) = agg_req.size {
                        terms["size"] = serde_json::json!(size);
                    }
                    serde_json::json!({ "terms": terms })
                }
                "stats" => {
                    serde_json::json!({
                        "stats": { "field": agg_req.field }
                    })
                }
                "avg" => {
                    serde_json::json!({
                        "avg": { "field": agg_req.field }
                    })
                }
                "min" => {
                    serde_json::json!({
                        "min": { "field": agg_req.field }
                    })
                }
                "max" => {
                    serde_json::json!({
                        "max": { "field": agg_req.field }
                    })
                }
                "sum" => {
                    serde_json::json!({
                        "sum": { "field": agg_req.field }
                    })
                }
                "count" => {
                    serde_json::json!({
                        "value_count": { "field": agg_req.field }
                    })
                }
                "cardinality" => {
                    serde_json::json!({
                        "cardinality": { "field": agg_req.field }
                    })
                }
                "histogram" => {
                    let interval = agg_req.interval.unwrap_or(10.0);
                    serde_json::json!({
                        "histogram": {
                            "field": agg_req.field,
                            "interval": interval
                        }
                    })
                }
                "range" => {
                    let ranges: Vec<serde_json::Value> = agg_req
                        .ranges
                        .as_ref()
                        .map(|r| {
                            r.iter()
                                .map(|range| {
                                    let mut obj = serde_json::Map::new();
                                    if let Some(from) = range.from {
                                        obj.insert("from".to_string(), serde_json::json!(from));
                                    }
                                    if let Some(to) = range.to {
                                        obj.insert("to".to_string(), serde_json::json!(to));
                                    }
                                    serde_json::Value::Object(obj)
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    serde_json::json!({
                        "range": {
                            "field": agg_req.field,
                            "ranges": ranges
                        }
                    })
                }
                "percentiles" => {
                    serde_json::json!({
                        "percentiles": { "field": agg_req.field }
                    })
                }
                "extended_stats" => {
                    serde_json::json!({
                        "extended_stats": { "field": agg_req.field }
                    })
                }
                _ => {
                    return Err(anyhow!("Unsupported aggregation type: {}", agg_req.agg_type));
                }
            };

            agg_map.insert(agg_req.name.clone(), agg_def);
        }

        let agg_json = serde_json::Value::Object(agg_map);
        let aggregations: Aggregations = serde_json::from_value(agg_json)
            .map_err(|e| anyhow!("Failed to parse aggregations: {}", e))?;

        Ok(aggregations)
    }

    pub fn suggest(
        &self,
        index_name: &str,
        prefix: &str,
        field: Option<&str>,
        limit: usize,
    ) -> Result<(Vec<String>, f64)> {
        let start = std::time::Instant::now();

        let indices = self.indices.read().unwrap();
        let handle = indices
            .get(index_name)
            .ok_or_else(|| anyhow!("Index not found: {}", index_name))?;

        let reader = handle
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        let searcher = reader.searcher();

        // Build prefix query
        let query_fields: Vec<Field> = if let Some(f) = field {
            handle
                .field_map
                .get(f)
                .map(|f| vec![*f])
                .unwrap_or_default()
        } else {
            handle.field_map.values().copied().collect()
        };

        let prefix_query = format!("{}*", prefix);
        let query_parser = QueryParser::for_index(&handle.index, query_fields.clone());
        let query = query_parser.parse_query(&prefix_query)?;

        let top_docs = searcher.search(&query, &TopDocs::with_limit(limit * 10))?;

        // Collect unique field values
        let mut suggestions: HashSet<String> = HashSet::new();

        for (_score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_address)?;

            for field in &query_fields {
                if let Some(field_value) = doc.get_all(*field).next() {
                    let owned_value: tantivy::schema::OwnedValue = field_value.into();
                    if let tantivy::schema::OwnedValue::Str(s) = owned_value {
                        // Check if any word starts with the prefix
                        for word in s.split_whitespace() {
                            if word.to_lowercase().starts_with(&prefix.to_lowercase()) {
                                suggestions.insert(word.to_string());
                            }
                        }
                    }
                }
            }

            if suggestions.len() >= limit {
                break;
            }
        }

        let took_ms = start.elapsed().as_secs_f64() * 1000.0;

        let mut result: Vec<_> = suggestions.into_iter().collect();
        result.sort();
        result.truncate(limit);

        Ok((result, took_ms))
    }

    pub fn get_index_stats(&self, index_name: &str, created_at: &str) -> Result<IndexStats> {
        let indices = self.indices.read().unwrap();
        let handle = indices
            .get(index_name)
            .ok_or_else(|| anyhow!("Index not found: {}", index_name))?;

        let reader = handle
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        let searcher = reader.searcher();
        let doc_count = searcher.num_docs();

        // Calculate index size
        let index_path = Path::new(&self.base_path).join(index_name);
        let size_bytes = Self::dir_size(&index_path).unwrap_or(0);

        // Build field stats
        let fields: Vec<FieldStats> = handle
            .field_configs
            .iter()
            .map(|fc| FieldStats {
                name: fc.name.clone(),
                field_type: fc.field_type.clone(),
                indexed: fc.indexed,
                stored: fc.stored,
            })
            .collect();

        Ok(IndexStats {
            name: index_name.to_string(),
            document_count: doc_count,
            size_bytes,
            fields,
            created_at: created_at.to_string(),
        })
    }

    fn dir_size(path: &Path) -> std::io::Result<u64> {
        let mut size = 0;
        if path.is_dir() {
            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                let metadata = entry.metadata()?;
                if metadata.is_file() {
                    size += metadata.len();
                } else if metadata.is_dir() {
                    size += Self::dir_size(&entry.path())?;
                }
            }
        }
        Ok(size)
    }

    pub fn delete_document(&self, index_name: &str, doc_id: &str) -> Result<()> {
        let indices = self.indices.read().unwrap();
        let handle = indices
            .get(index_name)
            .ok_or_else(|| anyhow!("Index not found: {}", index_name))?;

        let mut writer = handle.writer.write().unwrap();
        let id_field = handle.field_map.get("id").unwrap();

        writer.delete_term(Term::from_field_text(*id_field, doc_id));
        writer.commit()?;

        Ok(())
    }

    pub fn delete_index(&self, index_name: &str) -> Result<()> {
        let mut indices = self.indices.write().unwrap();
        indices.remove(index_name);

        let index_path = Path::new(&self.base_path).join(index_name);
        if index_path.exists() {
            std::fs::remove_dir_all(&index_path)?;
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn list_indices(&self) -> Vec<String> {
        self.indices.read().unwrap().keys().cloned().collect()
    }

    #[allow(dead_code)]
    pub fn get_document_count(&self, index_name: &str) -> Result<u64> {
        let indices = self.indices.read().unwrap();
        let handle = indices
            .get(index_name)
            .ok_or_else(|| anyhow!("Index not found: {}", index_name))?;

        let reader = handle
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        let searcher = reader.searcher();
        Ok(searcher.num_docs())
    }
}
