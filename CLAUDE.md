# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Simple Search Service is a lightweight full-text search HTTP service built with Rust. It uses Tantivy (Rust's Lucene equivalent) for indexing/search and SQLite for metadata tracking.

## Build & Development Commands

```bash
# Build
cargo build --release          # Production build (optimized)
cargo build                    # Debug build

# Run
cargo run --release            # Run locally (starts on port 3000)

# Test
cargo test                     # Run unit tests
./build-and-test.sh           # Full CI: build + test + integration tests

# Linting & Formatting
cargo clippy -- -D warnings    # Lint with strict warnings
cargo fmt                      # Format code
cargo fmt --check              # Check formatting

# Makefile shortcuts
make build                     # Release build
make test                      # Run tests
make check                     # Clippy + fmt check
make run                       # Run the service
```

## Architecture

The source files are in the project root (not in `src/`):

```
main.rs      → Entry point: Tokio runtime, Axum router setup, AppState initialization
handlers.rs  → HTTP request handlers for all 8 REST endpoints
models.rs    → Request/response structs with Serde derive
search.rs    → Tantivy search engine wrapper (SearchEngine, IndexHandle)
storage.rs   → SQLite metadata store (MetadataStore)
```

### Data Flow

```
HTTP Request → handlers.rs → SearchEngine (Tantivy) + MetadataStore (SQLite)
                                    ↓                        ↓
                            ./data/indices/         ./data/metadata.db
```

### Key Components

- **AppState** (`main.rs:18`): Shared state holding `SearchEngine` and `MetadataStore`, wrapped in `Arc` for thread-safe access
- **SearchEngine** (`search.rs:12`): Manages multiple Tantivy indices with `RwLock<HashMap<String, IndexHandle>>`
- **IndexHandle** (`search.rs:17`): Per-index wrapper containing Tantivy Index, Schema, IndexWriter, and field mappings
- **MetadataStore** (`storage.rs:8`): SQLite connection for tracking index/document metadata with timestamps

### REST API Endpoints

All handlers in `handlers.rs` follow the pattern: extract state + params → call search_engine/metadata_store → return JSON ApiResponse

- `GET /health` - Health check
- `POST /indices` - Create index with field schema
- `GET /indices` - List indices with document counts
- `DELETE /indices/:name` - Delete index
- `POST /indices/:name/documents` - Add documents
- `DELETE /indices/:name/documents/:id` - Delete document
- `POST /indices/:name/search` - Full-text search
- `POST /indices/:name/bulk` - Bulk index/delete operations

### Field Types

Supported in `search.rs:44-89`: `text` (tokenized + stored), `string` (exact match), `i64`, `f64`

## Configuration

Environment variables:
- `DATA_DIR` - Data directory path (default: `./data`)
- `PORT` - Server port (default: `3000`)
- `RUST_LOG` - Log level: `trace`, `debug`, `info`, `warn`, `error`
