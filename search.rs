use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::{Index, IndexWriter, ReloadPolicy};

use crate::models::{Document, FieldConfig, SearchHit};

pub struct SearchEngine {
    base_path: String,
    indices: Arc<RwLock<HashMap<String, IndexHandle>>>,
}

struct IndexHandle {
    index: Index,
    schema: Schema,
    writer: Arc<RwLock<IndexWriter>>,
    field_map: HashMap<String, Field>,
}

impl SearchEngine {
    pub fn new(base_path: &str) -> Result<Self> {
        std::fs::create_dir_all(base_path)?;
        
        Ok(Self {
            base_path: base_path.to_string(),
            indices: Arc::new(RwLock::new(HashMap::new())),
        })
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
                        options = options.set_indexing_options(
                            TextFieldIndexing::default()
                                .set_tokenizer("default")
                                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                        );
                    }
                    schema_builder.add_text_field(&field_config.name, options)
                }
                "string" => {
                    let mut options = STORED;
                    if field_config.indexed {
                        options = STRING | STORED;
                    }
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
                    schema_builder.add_f64_field(&field_config.name, options)
                }
                _ => {
                    return Err(anyhow!("Unsupported field type: {}", field_config.field_type));
                }
            };
            field_map.insert(field_config.name.clone(), field);
        }

        let schema = schema_builder.build();
        let index_path = Path::new(&self.base_path).join(name);
        std::fs::create_dir_all(&index_path)?;

        let index = Index::create_in_dir(&index_path, schema.clone())?;
        let writer = index.writer(50_000_000)?; // 50MB buffer

        let handle = IndexHandle {
            index,
            schema,
            writer: Arc::new(RwLock::new(writer)),
            field_map,
        };

        self.indices.write().unwrap().insert(name.to_string(), handle);

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
                    match value {
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
                    }
                }
            }

            writer.add_document(tantivy_doc)?;
        }

        writer.commit()?;
        Ok(())
    }

    pub fn search(
        &self,
        index_name: &str,
        query_str: &str,
        limit: usize,
        fields: &[String],
    ) -> Result<(Vec<SearchHit>, f64)> {
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
            handle
                .field_map
                .values()
                .copied()
                .collect()
        } else {
            fields
                .iter()
                .filter_map(|f| handle.field_map.get(f).copied())
                .collect()
        };

        let query_parser = QueryParser::for_index(&handle.index, query_fields);
        let query = query_parser.parse_query(query_str)?;

        let top_docs = searcher.search(&query, &TopDocs::with_limit(limit))?;

        let mut hits = Vec::new();
        for (score, doc_address) in top_docs {
            let retrieved_doc: TantivyDocument = searcher.doc(doc_address)?;
            let mut field_values = HashMap::new();

            for (field_name, field) in &handle.field_map {
                if let Some(field_values_iter) = retrieved_doc.get_all(*field).next() {
                    let value = match field_values_iter {
                        tantivy::schema::Value::Str(s) => serde_json::Value::String(s.to_string()),
                        tantivy::schema::Value::U64(n) => serde_json::json!(n),
                        tantivy::schema::Value::I64(n) => serde_json::json!(n),
                        tantivy::schema::Value::F64(n) => serde_json::json!(n),
                        _ => continue,
                    };
                    field_values.insert(field_name.clone(), value);
                }
            }

            let id = field_values
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            hits.push(SearchHit {
                id,
                score,
                fields: field_values,
            });
        }

        let took_ms = start.elapsed().as_secs_f64() * 1000.0;

        Ok((hits, took_ms))
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

    pub fn list_indices(&self) -> Vec<String> {
        self.indices
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

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
