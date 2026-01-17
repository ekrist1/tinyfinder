# Simple Search Service

A lightweight, powerful full-text search service built with Rust, Tantivy, and SQLite. Think Elasticsearch/Solr but much simpler to deploy and operate.

## Features

- 🚀 **Fast**: Built on Tantivy, Rust's answer to Lucene
- 💾 **Simple Storage**: Uses SQLite for metadata and Tantivy's built-in index storage
- 🔌 **RESTful API**: Easy integration with any application
- 🐳 **Easy Deploy**: Single binary or Docker container
- 🔍 **Full-Text Search**: BM25 ranking, phrase queries, fuzzy matching
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

Response:
```json
{
  "status": "healthy",
  "service": "simple-search-service",
  "version": "0.1.0"
}
```

### Create Index

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

Field types: `text`, `string`, `i64`, `f64`

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
  "fuzzy": true
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

### Delete Document

```bash
DELETE /indices/products/documents/prod_001
```

### Delete Index

```bash
DELETE /indices/products
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

## Performance Tips

1. **Bulk Operations**: Use bulk endpoints for adding multiple documents
2. **Field Selection**: Only store fields you need to display in results
3. **Index Size**: Expect index size to be 10-20% of original text
4. **Memory**: Allocate ~50MB per active index + buffer

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
