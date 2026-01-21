# Simple Search Service

A lightweight, powerful full-text search service built with Rust, Tantivy, and SQLite. Think Elasticsearch/Solr but much simpler to deploy and operate.

## Features

- 🚀 **Fast**: Built on Tantivy, Rust's answer to Lucene
- 💾 **Simple Storage**: Uses SQLite for metadata and Tantivy's built-in index storage
- 🔌 **RESTful API**: Easy integration with any application
- 🐳 **Easy Deploy**: Single binary or Docker container
- 🔍 **Full-Text Search**: BM25 ranking, phrase queries, fuzzy matching
- 🤖 **Generative Answers**: Mistral-powered, source-grounded responses (optional)
- 🌍 **Multi-language**: Supports Norwegian, English, and more
- 📊 **Lightweight**: Runs on 512MB RAM

## Quick Start

### Option 1: Docker (Recommended)

```bash
# Clone or extract the project
cd search-service

# Start with Docker Compose
docker-compose up -d

# Check health
curl http://localhost:3000/health
```

Docker Compose loads environment variables from `.env` (see `env_file` in `docker-compose.yml`).

### Option 2: Build from Source

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build
cargo build --release

# Run
./target/release/simple-search-service
```

The service will start on `http://localhost:3000`

## API Documentation

### Health Check

```bash
GET /health
```

Response (healthy):
```json
{
  "status": "healthy",
  "service": "simple-search-service",
  "version": "0.2.0",
  "checks": {
    "database": "healthy"
  }
}
```

Response (unhealthy - returns HTTP 503):
```json
{
  "status": "unhealthy",
  "service": "simple-search-service",
  "version": "0.2.0",
  "checks": {
    "database": "unhealthy"
  }
}
```

### Create Index

**Index name requirements:**
- Must start with a letter (a-z, A-Z)
- Can contain letters, numbers, underscores, and hyphens
- Maximum 64 characters
- Cannot contain path separators or `..`

```bash
POST /indices
Content-Type: application/json

{
  "name": "products",
  "fields": [
    {
      "name": "title",
      "field_type": "text",
      "stored": true,
      "indexed": true
    },
    {
      "name": "description",
      "field_type": "text",
      "stored": true,
      "indexed": true
    },
    {
      "name": "price",
      "field_type": "f64",
      "stored": true,
      "indexed": true
    }
  ]
}
```

Field types: `text`, `string`, `i64`, `f64`, `date`, `json`

- **text**: Full-text searchable, tokenized
- **string**: Exact match (no tokenization), useful for IDs, tags
- **i64/f64**: Numeric fields for sorting and range queries
- **date**: DateTime fields, use ISO 8601 format
- **json**: Nested JSON objects, searchable with dot notation (e.g., `attributes.category:electronics`)

For sorting and aggregations, set `"fast": true` on the field (required for date sorting).

### List Indices

```bash
GET /indices
```

Response:
```json
{
  "success": true,
  "data": [
    {
      "name": "products",
      "document_count": 1250,
      "created_at": "2025-01-16T10:30:00Z"
    }
  ]
}
```

### Add Documents

```bash
POST /indices/products/documents
Content-Type: application/json

{
  "documents": [
    {
      "id": "prod_001",
      "fields": {
        "title": "Smil Barnehage Bergen",
        "description": "Modern barnehage i Bergen sentrum med fokus på læring gjennom lek",
        "price": 15000.0
      }
    },
    {
      "id": "prod_002",
      "fields": {
        "title": "Lekeland Barnehage",
        "description": "Familievennlig barnehage med store uteområder",
        "price": 12500.0
      }
    }
  ]
}
```

### Search

```bash
POST /indices/products/search
Content-Type: application/json

{
  "query": "barnehage bergen",
  "limit": 10,
  "fields": ["title", "description"],
  "boost": {
    "title": 2.0
  },
  "fuzzy": true,
  "sort": {
    "field": "starts_at",
    "order": "desc"
  }
}
```

Response:
```json
{
  "success": true,
  "data": {
    "took_ms": 2.4,
    "total": 2,
    "hits": [
      {
        "id": "prod_001",
        "score": 8.42,
        "fields": {
          "id": "prod_001",
          "title": "Smil Barnehage Bergen",
          "description": "Modern barnehage i Bergen sentrum...",
          "price": 15000.0
        }
      }
    ]
  }
}
```

#### Partial and fuzzy matching

- Append an asterisk to any term (for example, `"query": "eventyr*"`) to perform a prefix search that matches tokens beginning with that fragment.
- Set `"fuzzy": true` in the search payload to tolerate a single-character typo (insertions, deletions, substitutions, or transpositions), which helps catch misspellings like `evntyr`.

#### Sorting by date

To sort by a date field, define the field as `"field_type": "date"` and set `"fast": true` when creating the index. Then pass the `sort` object in the search request:

```json
{
  "query": "barnehage",
  "limit": 10,
  "sort": {
    "field": "starts_at",
    "order": "asc"
  }
}
```

Supported sort field types: `i64`, `f64`, `date` (must be `fast: true`).

#### Advanced Query Features

**Field Grouping**: Use parentheses to group terms for a specific field:
```json
{
  "query": "title:(return AND \"pink panther\")"
}
```
This expands to `(title:return AND title:\"pink panther\")`.

**Exists Query**: Find documents where a field has any value:
```json
{
  "query": "_exists_:description"
}
```

**Phrase Wildcards**: Use wildcards in phrase queries:
```json
{
  "query": "\"b.* b.* wolf\""
}
```
This matches phrases where any word starting with "b" appears twice before "wolf".

**Minimum Should Match**: Require a minimum number of terms to match:
```json
{
  "query": "cat dog bird",
  "minimum_should_match": 2
}
```
This requires at least 2 of the 3 terms to match.

**Term Set Query**: Efficiently search for documents matching any of multiple exact terms:
```json
{
  "query": "category:IN[electronics,computers,phones]"
}
```
This is more efficient than `category:electronics OR category:computers OR category:phones` when searching for many exact values.

#### Aggregations

Aggregations allow you to compute statistics and group data across your search results. Supported types:

- **terms**: Group by field values (bucket aggregation)
- **stats**: Compute min, max, avg, sum, count
- **avg/min/max/sum**: Single metric aggregations
- **range**: Group into custom ranges
- **date_histogram**: Group by date intervals
- **histogram**: Group by numeric intervals
- **cardinality**: Count unique values
- **percentiles**: Compute percentile values
- **extended_stats**: Extended statistics including variance and std deviation

Example with nested aggregations:
```json
{
  "query": "*",
  "aggregations": [
    {
      "name": "by_category",
      "agg_type": "terms",
      "field": "category"
    },
    {
      "name": "price_stats",
      "agg_type": "stats",
      "field": "price"
    }
  ]
}
```

Aggregation results are returned in Elasticsearch-compatible format.

### Synonyms

Synonyms allow you to expand search terms with equivalent words. When a user searches for "tariff", documents containing "tariffavtale" or "hovedtariffavtale" can also match.

#### Add Synonyms

```bash
POST /indices/products/synonyms
Content-Type: application/json

{
  "synonyms": [
    {
      "terms": ["tariff", "tariffavtale", "hovedtariffavtale"]
    },
    {
      "terms": ["avtale", "kontrakt", "overenskomst"]
    }
  ]
}
```

```curl
curl -X POST http://localhost:3000/indices/myindex/synonyms \
  -H 'Content-Type: application/json' \
  -d '{
    "synonyms": [
      {"terms": ["tariff", "tariffavtale", "hovedtariffavtale"]}
    ]
  }'
```

All terms in a synonym group are treated as equivalent. When searching for any term in the group, all related terms will be included in the query.

#### Get Synonyms

```bash
GET /indices/products/synonyms
```

Response:
```json
{
  "success": true,
  "data": {
    "synonyms": [
      {"terms": ["tariff", "tariffavtale", "hovedtariffavtale"]},
      {"terms": ["avtale", "kontrakt", "overenskomst"]}
    ]
  }
}
```

#### Clear Synonyms

```bash
DELETE /indices/products/synonyms
```

#### How Synonyms Work

When you search for "tariff" and synonyms are configured:
1. The query is expanded to `(tariff OR tariffavtale OR hovedtariffavtale)`
2. Documents matching any of these terms will be returned
3. Synonyms are applied at query time (no reindexing required)

**Note:** For substring matching within compound words (like finding "hovedtariffavtale" when searching "tariff"), use wildcards: `tariff*`

### Pinned Results (Promoted Documents)

Pinned results allow you to manually override search rankings for specific queries. When a user searches for a query matching a pinned rule, the specified documents are always shown at the top of results.

#### Add Pinned Rules

```bash
POST /indices/products/pinned
Content-Type: application/json

{
  "rules": [
    {
      "queries": ["tariff", "hovedtariff"],
      "document_ids": ["doc_123", "doc_456"]
    },
    {
      "queries": ["sale", "discount"],
      "document_ids": ["promo_001"]
    }
  ]
}
```

Each rule specifies:
- `queries`: List of search terms that trigger this rule (case-insensitive substring match)
- `document_ids`: List of document IDs to pin to the top, in order

#### Get Pinned Rules

```bash
GET /indices/products/pinned
```

Response:
```json
{
  "success": true,
  "data": {
    "rules": [
      {
        "queries": ["tariff", "hovedtariff"],
        "document_ids": ["doc_123", "doc_456"]
      }
    ]
  }
}
```

```curl
# Add a rule: when searching "tariff", pin doc_123 to top
curl -X POST http://localhost:3000/indices/myindex/pinned \
  -H 'Content-Type: application/json' \
  -d '{
    "rules": [
      {"queries": ["tariff"], "document_ids": ["doc_123", "doc_456"]}
    ]
  }'
``

#### Clear Pinned Rules

```bash
DELETE /indices/products/pinned
```

#### How Pinned Results Work

When you search for "tariff" and a pinned rule is configured:
1. The search executes normally with BM25 ranking
2. Documents matching the pinned rule are extracted from results
3. Pinned documents are moved to the top in the specified order
4. Remaining results follow in their original order

**Example:**
```bash
# Without pinned rules, search for "tariff" returns:
# 1. doc_789 (score: 8.5)
# 2. doc_456 (score: 7.2)
# 3. doc_123 (score: 6.8)

# Add pinned rule
curl -X POST http://localhost:3000/indices/myindex/pinned \
  -H 'Content-Type: application/json' \
  -d '{
    "rules": [
      {"queries": ["tariff"], "document_ids": ["doc_123", "doc_456"]}
    ]
  }'

# Now search for "tariff" returns:
# 1. doc_123 (pinned)
# 2. doc_456 (pinned)
# 3. doc_789 (score: 8.5)
```

**Notes:**
- Pinned documents must exist in search results to be promoted (they are reordered, not injected)
- Multiple rules can exist; the first matching rule is applied
- Query matching is case-insensitive and uses substring matching
- Pinned rules are persisted to disk and survive restarts

### Generative Answers (Mistral)

This endpoint runs a search, then asks Mistral to summarize the top hits into a grounded answer.
If `stream` is `true` (default), the response is an SSE stream.

```bash
POST /indices/products/answer
Content-Type: application/json

{
  "query": "hvor er familievennlig barnehage",
  "search_limit": 5,
  "fields": ["title", "description", "location"],
  "fuzzy": true,
  "stream": false,
  "temperature": 0.2
}
```

Response (non-streaming):
```json
{
  "success": true,
  "data": {
    "answer": "...",
    "model": "mistral-large-latest",
    "search_took_ms": 3.1,
    "llm_took_ms": 412.7,
    "total_took_ms": 418.5,
    "sources": [
      {
        "id": "kg_001",
        "score": 8.42,
        "fields": {
          "title": "Lekeland Barnehage",
          "description": "Familievennlig barnehage ..."
        }
      }
    ]
  }
}
```

Streaming (SSE) example:
```bash
curl -N http://localhost:3000/indices/kindergartens/answer \
  -H "Content-Type: application/json" \
  -d '{"query":"hvor er familievennlig barnehage","stream":true}'
```

The stream emits:
- `event: meta` with JSON containing `model`, `search_took_ms`, and `sources`
- `data:` chunks with partial answer text
- `event: done` when finished

#### Highlighting

To highlight search terms in the results, set `"highlight": true` in the search payload. This will wrap matching terms in `<em>` tags.

Usage example:

POST /indices/myindex/search
{
  "query": "kindergarten",
  "highlight": {
    "enabled": true,
    "fields": ["title", "content"],
    "pre_tag": "<mark>",
    "post_tag": "</mark>",
    "max_num_chars": 200
  }
}

```curl
curl -X POST http://localhost:3000/indices/kindergartens/search \
  -H "Content-Type: application/json" \
  -d '{"query": "eventyr", "highlight": {"enabled": true}}'
```

### Delete Document

```bash
DELETE /indices/products/documents/prod_001
```

### Delete Index

```bash
DELETE /indices/products
```

```curl
curl -s -X DELETE http://localhost:3000/indices/kindergartens  
```

### Bulk Operations

```bash
POST /indices/products/bulk
Content-Type: application/json

{
  "operations": [
    {
      "operation": "index",
      "document": {
        "id": "prod_003",
        "fields": {
          "title": "New Product",
          "description": "Description here"
        }
      }
    },
    {
      "operation": "delete",
      "id": "prod_001"
    }
  ]
}
```

## Integration Examples

### Laravel/PHP

```php
use Illuminate\Support\Facades\Http;

// Create index
$response = Http::post('http://localhost:3000/indices', [
    'name' => 'kindergartens',
    'fields' => [
        ['name' => 'title', 'field_type' => 'text', 'stored' => true, 'indexed' => true],
        ['name' => 'description', 'field_type' => 'text', 'stored' => true, 'indexed' => true],
    ]
]);

// Add documents
$response = Http::post('http://localhost:3000/indices/kindergartens/documents', [
    'documents' => [
        [
            'id' => 'kg_001',
            'fields' => [
                'title' => 'Smil Barnehage',
                'description' => 'En flott barnehage i Bergen',
            ]
        ]
    ]
]);

// Search
$response = Http::post('http://localhost:3000/indices/kindergartens/search', [
    'query' => 'barnehage bergen',
    'limit' => 10
]);

$results = $response->json()['data'];
```

### JavaScript/Node.js

```javascript
// Add documents
const response = await fetch('http://localhost:3000/indices/products/documents', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    documents: [
      {
        id: 'prod_001',
        fields: {
          title: 'Product Name',
          description: 'Product description'
        }
      }
    ]
  })
});

// Search
const searchResponse = await fetch('http://localhost:3000/indices/products/search', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    query: 'search term',
    limit: 10
  })
});

const results = await searchResponse.json();
```

## Configuration

Environment variables:

- `DATA_DIR`: Data directory path (default: `./data`)
- `PORT`: Server port (default: `3000`)
- `RUST_LOG`: Log level (default: `info`, options: `trace`, `debug`, `info`, `warn`, `error`)
- `API_TOKENS`: Comma-separated list of API tokens for authentication (optional, protects write endpoints)
- `CORS_ORIGINS`: Comma-separated list of allowed CORS origins (default: `*` allows all origins)
- `MISTRAL_API_KEY`: API key for Mistral (enables `/indices/:name/answer`)
- `MISTRAL_MODEL`: Mistral model name (default: `mistral-large-latest`)
- `MISTRAL_BASE_URL`: Base URL for Mistral-compatible API (default: `https://api.mistral.ai/v1`)

### CORS Configuration

For production, configure specific origins:

```bash
export CORS_ORIGINS="https://app.example.com,https://admin.example.com"
```

### API Limits

The service enforces the following limits to prevent abuse:

| Limit | Value | Description |
|-------|-------|-------------|
| Request body size | 10 MB | Maximum size of JSON payloads |
| Documents per request | 1,000 | Maximum documents in `POST /indices/:name/documents` |
| Bulk operations | 1,000 | Maximum operations in `POST /indices/:name/bulk` |
| Pagination limit | 1,000 | Maximum `limit` parameter (silently capped) |
| Index name length | 64 chars | Maximum length for index names |

## Performance Tips

1. **Bulk Operations**: Use bulk endpoints for adding multiple documents
2. **Field Selection**: Only store fields you need to display in results
3. **Index Size**: Expect index size to be 10-20% of original text
4. **Memory**: Allocate ~50MB per active index + buffer

## Performance Testing

To populate the index with 10,000 synthetic records for testing:

```bash
# Install dependencies (if needed)
pip install requests

# Run the populate script
python3 populate_index.py
```

This creates an index named `large_dataset` and populates it with random products.

## Production Deployment

### Systemd Service

Create `/etc/systemd/system/search-service.service`:

```ini
[Unit]
Description=Simple Search Service
After=network.target

[Service]
Type=simple
User=search
WorkingDirectory=/opt/search-service
Environment="DATA_DIR=/var/lib/search-service"
Environment="PORT=3000"
Environment="CORS_ORIGINS=https://app.example.com"
Environment="API_TOKENS=your-secret-token"
ExecStart=/opt/search-service/simple-search-service
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable search-service
sudo systemctl start search-service
```

### Nginx Reverse Proxy

```nginx
server {
    listen 80;
    server_name search.yourdomain.com;

    location / {
        proxy_pass http://localhost:3000;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    }
}
```

## Monitoring

The service exposes a `/health` endpoint for health checks:

```bash
# Docker health check
HEALTHCHECK --interval=30s --timeout=10s --retries=3 \
  CMD curl -f http://localhost:3000/health || exit 1
```

## Backup

The data directory contains:
- `metadata.db`: SQLite database with metadata
- `indices/`: Directory with Tantivy index files

Simply backup the entire data directory:

```bash
# Backup
tar -czf search-backup-$(date +%Y%m%d).tar.gz data/

# Restore
tar -xzf search-backup-20250116.tar.gz
```

## Use Cases

- **E-commerce**: Product search with faceted filtering
- **Documentation**: Technical documentation search
- **CRM**: Customer and contact search
- **Content Management**: Article and page search
- **Internal Tools**: Log search, ticket search

## Comparison with Elasticsearch

| Feature | Simple Search Service | Elasticsearch |
|---------|----------------------|---------------|
| Memory | ~512MB | ~2GB minimum |
| Deployment | Single binary | JVM + cluster |
| Setup Time | < 1 minute | 15-30 minutes |
| Cluster | No | Yes |
| Scaling | Vertical | Horizontal |
| Best For | Single server, <10M docs | Distributed, >10M docs |

## License

MIT License - feel free to use in commercial projects

## Support

For issues or questions, please open an issue on the GitHub repository.
