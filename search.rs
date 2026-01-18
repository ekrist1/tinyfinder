use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, RwLock};
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, FuzzyTermQuery, Occur, Query, QueryParser, RegexQuery};
use tantivy::schema::*;
use tantivy::tokenizer::{LowerCaser, SimpleTokenizer, Stemmer, TextAnalyzer};
use tantivy::{Index, IndexWriter, Order, ReloadPolicy, TantivyDocument, Term};

use crate::models::{
    AggregationBucket, AggregationRequest, AggregationResult, Document, FieldConfig, FieldStats,
    HighlightOptions, IndexStats, SearchHit, SortOption, SortOrder, StatsResult,
};

pub type SearchResult = Result<(Vec<SearchHit>, usize, f64, Option<Vec<AggregationResult>>)>;

pub struct SearchEngine {
    base_path: String,
    indices: Arc<RwLock<HashMap<String, IndexHandle>>>,
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

        Ok(Self {
            base_path: base_path.to_string(),
            indices: Arc::new(RwLock::new(HashMap::new())),
        })
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

        let writer = index.writer(50_000_000)?; // 50MB buffer

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
    ) -> SearchResult {
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

        let query = Self::build_query(handle, query_str, &query_fields, fuzzy)?;

        // Get total document count that matches the query
        let total = searcher.search(query.as_ref(), &tantivy::collector::Count)?;

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
                                if let Ok(mut snippet_gen) = tantivy::snippet::SnippetGenerator::create(
                                    &searcher,
                                    query.as_ref(),
                                    *field,
                                ) {
                                    snippet_gen.set_max_num_chars(opts.max_num_chars);
                                    let snippet = snippet_gen.snippet_from_doc(&retrieved_doc);
                                    let html = snippet.to_html();
                                    // Replace default <b> tags with custom tags
                                    let highlighted = html
                                        .replace("<b>", &opts.pre_tag)
                                        .replace("</b>", &opts.post_tag);
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

            match field_config.field_type.as_str() {
                "i64" => {
                    let collector = TopDocs::with_limit(limit)
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
                    let collector = TopDocs::with_limit(limit)
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
                    let collector = TopDocs::with_limit(limit)
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
                        "Sorting is only supported on fast i64, f64, or date fields. Field '{}' is type '{}'.",
                        field_name,
                        field_config.field_type
                    ));
                }
            }
        } else {
            // Fetch offset + limit results, then skip offset
            let top_docs = searcher.search(query.as_ref(), &TopDocs::with_limit(offset + limit))?;
            for (score, doc_address) in top_docs.into_iter().skip(offset) {
                add_hit(score, doc_address)?;
            }
        }

        // Process aggregations
        let agg_results = if !aggregations.is_empty() {
            let mut results = Vec::new();
            for agg_req in aggregations {
                if let Some(agg_result) =
                    self.compute_aggregation(handle, &searcher, query.as_ref(), agg_req)?
                {
                    results.push(agg_result);
                }
            }
            if results.is_empty() {
                None
            } else {
                Some(results)
            }
        } else {
            None
        };

        let took_ms = start.elapsed().as_secs_f64() * 1000.0;

        Ok((hits, total, took_ms, agg_results))
    }

    fn build_query(
        handle: &IndexHandle,
        query_str: &str,
        query_fields: &[Field],
        fuzzy: bool,
    ) -> Result<Box<dyn Query>> {
        let query_parser = QueryParser::for_index(&handle.index, query_fields.to_vec());
        
        // Check if the query contains wildcards (* or ?)
        let has_wildcard = query_str.chars().any(|ch| matches!(ch, '*' | '?'));
        
        // For wildcard queries, we need to create RegexQuery manually
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
                    let prefix = pattern.split(|c| c == '*' || c == '?').next().unwrap_or("");
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

        let mut combined: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        combined.push((Occur::Should, base_query));
        combined.push((Occur::Should, fuzzy_query));

        Ok(Box::new(BooleanQuery::from(combined)))
    }

    fn compute_aggregation(
        &self,
        handle: &IndexHandle,
        searcher: &tantivy::Searcher,
        query: &dyn tantivy::query::Query,
        agg_req: &AggregationRequest,
    ) -> Result<Option<AggregationResult>> {
        let field = match handle.field_map.get(&agg_req.field) {
            Some(f) => *f,
            None => return Ok(None),
        };

        match agg_req.agg_type.as_str() {
            "terms" => {
                // Collect unique values and their counts
                let top_docs = searcher.search(query, &TopDocs::with_limit(10000))?;
                let mut term_counts: HashMap<String, u64> = HashMap::new();

                for (_score, doc_address) in top_docs {
                    let doc: TantivyDocument = searcher.doc(doc_address)?;
                    let key: Option<String> =
                        doc.get_all(field).next().map(|v| -> tantivy::schema::OwnedValue { v.into() }).and_then(|value| match value {
                            tantivy::schema::OwnedValue::Str(s) => Some(s.to_string()),
                            tantivy::schema::OwnedValue::I64(n) => Some(n.to_string()),
                            tantivy::schema::OwnedValue::U64(n) => Some(n.to_string()),
                            tantivy::schema::OwnedValue::F64(n) => Some(n.to_string()),
                            _ => None,
                        });
                    if let Some(k) = key {
                        *term_counts.entry(k).or_insert(0) += 1;
                    }
                }

                let size = agg_req.size.unwrap_or(10);
                let mut buckets: Vec<_> = term_counts.into_iter().collect();
                buckets.sort_by(|a, b| b.1.cmp(&a.1));
                buckets.truncate(size);

                let result_buckets = buckets
                    .into_iter()
                    .map(|(key, count)| AggregationBucket {
                        key: serde_json::Value::String(key),
                        doc_count: count,
                    })
                    .collect();

                Ok(Some(AggregationResult {
                    name: agg_req.name.clone(),
                    buckets: Some(result_buckets),
                    stats: None,
                }))
            }
            "stats" => {
                let top_docs = searcher.search(query, &TopDocs::with_limit(100000))?;
                let mut values: Vec<f64> = Vec::new();

                for (_score, doc_address) in top_docs {
                    let doc: TantivyDocument = searcher.doc(doc_address)?;
                    let num: Option<f64> =
                        doc.get_all(field).next().map(|v| -> tantivy::schema::OwnedValue { v.into() }).and_then(|value| match value {
                            tantivy::schema::OwnedValue::I64(n) => Some(n as f64),
                            tantivy::schema::OwnedValue::U64(n) => Some(n as f64),
                            tantivy::schema::OwnedValue::F64(n) => Some(n),
                            _ => None,
                        });
                    if let Some(n) = num {
                        values.push(n);
                    }
                }

                if values.is_empty() {
                    return Ok(None);
                }

                let count = values.len() as u64;
                let sum: f64 = values.iter().sum();
                let avg = if count > 0 {
                    Some(sum / count as f64)
                } else {
                    None
                };
                let min = values.iter().copied().fold(f64::INFINITY, f64::min);
                let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);

                Ok(Some(AggregationResult {
                    name: agg_req.name.clone(),
                    buckets: None,
                    stats: Some(StatsResult {
                        count,
                        sum,
                        avg,
                        min: Some(min),
                        max: Some(max),
                    }),
                }))
            }
            "histogram" => {
                let interval = agg_req.interval.unwrap_or(10.0);
                let top_docs = searcher.search(query, &TopDocs::with_limit(100000))?;
                let mut bucket_counts: HashMap<i64, u64> = HashMap::new();

                for (_score, doc_address) in top_docs {
                    let doc: TantivyDocument = searcher.doc(doc_address)?;
                    let num: Option<f64> =
                        doc.get_all(field).next().map(|v| -> tantivy::schema::OwnedValue { v.into() }).and_then(|value| match value {
                            tantivy::schema::OwnedValue::I64(n) => Some(n as f64),
                            tantivy::schema::OwnedValue::U64(n) => Some(n as f64),
                            tantivy::schema::OwnedValue::F64(n) => Some(n),
                            _ => None,
                        });
                    if let Some(n) = num {
                        let bucket_key = (n / interval).floor() as i64;
                        *bucket_counts.entry(bucket_key).or_insert(0) += 1;
                    }
                }

                let mut buckets: Vec<_> = bucket_counts.into_iter().collect();
                buckets.sort_by_key(|(k, _)| *k);

                let result_buckets = buckets
                    .into_iter()
                    .map(|(key, count)| AggregationBucket {
                        key: serde_json::json!(key as f64 * interval),
                        doc_count: count,
                    })
                    .collect();

                Ok(Some(AggregationResult {
                    name: agg_req.name.clone(),
                    buckets: Some(result_buckets),
                    stats: None,
                }))
            }
            "range" => {
                let ranges = match &agg_req.ranges {
                    Some(r) => r,
                    None => return Ok(None),
                };

                let top_docs = searcher.search(query, &TopDocs::with_limit(100000))?;
                let mut values: Vec<f64> = Vec::new();

                for (_score, doc_address) in top_docs {
                    let doc: TantivyDocument = searcher.doc(doc_address)?;
                    let num: Option<f64> =
                        doc.get_all(field).next().map(|v| -> tantivy::schema::OwnedValue { v.into() }).and_then(|value| match value {
                            tantivy::schema::OwnedValue::I64(n) => Some(n as f64),
                            tantivy::schema::OwnedValue::U64(n) => Some(n as f64),
                            tantivy::schema::OwnedValue::F64(n) => Some(n),
                            _ => None,
                        });
                    if let Some(n) = num {
                        values.push(n);
                    }
                }

                let result_buckets = ranges
                    .iter()
                    .map(|range| {
                        let count = values
                            .iter()
                            .filter(|&&v| {
                                let above_from = range.from.map(|f| v >= f).unwrap_or(true);
                                let below_to = range.to.map(|t| v < t).unwrap_or(true);
                                above_from && below_to
                            })
                            .count() as u64;

                        let key = format!(
                            "{}-{}",
                            range
                                .from
                                .map(|f| f.to_string())
                                .unwrap_or_else(|| "*".to_string()),
                            range
                                .to
                                .map(|t| t.to_string())
                                .unwrap_or_else(|| "*".to_string())
                        );

                        AggregationBucket {
                            key: serde_json::Value::String(key),
                            doc_count: count,
                        }
                    })
                    .collect();

                Ok(Some(AggregationResult {
                    name: agg_req.name.clone(),
                    buckets: Some(result_buckets),
                    stats: None,
                }))
            }
            _ => Ok(None),
        }
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
