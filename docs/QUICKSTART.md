# Quick Start Guide

Get up and running with Simple Search Service in under 5 minutes.

## Prerequisites

- Docker and Docker Compose (recommended), OR
- Rust 1.75+ (for building from source)

## Option 1: Docker (Recommended)

### 1. Start the Service

```bash
cd search-service
docker-compose up -d
```

The service will be available at `http://localhost:3000`

### 2. Verify It's Running

```bash
curl http://localhost:3000/health
```

You should see:
```json
{
  "status": "healthy",
  "service": "simple-search-service",
  "version": "0.1.0"
}
```

### 3. Create Your First Index

```bash
curl -X POST http://localhost:3000/indices \
  -H "Content-Type: application/json" \
  -d '{
    "name": "products",
    "fields": [
      {"name": "title", "field_type": "text", "stored": true, "indexed": true},
      {"name": "description", "field_type": "text", "stored": true, "indexed": true}
    ]
  }'
```

### 4. Add Some Documents

```bash
curl -X POST http://localhost:3000/indices/products/documents \
  -H "Content-Type: application/json" \
  -d '{
    "documents": [
      {
        "id": "1",
        "fields": {
          "title": "Gaming Laptop",
          "description": "High-performance laptop for gaming"
        }
      },
      {
        "id": "2",
        "fields": {
          "title": "Business Laptop",
          "description": "Professional laptop for work"
        }
      }
    ]
  }'
```

### 5. Search!

```bash
curl -X POST http://localhost:3000/indices/products/search \
  -H "Content-Type: application/json" \
  -d '{
    "query": "gaming",
    "limit": 10
  }'
```

## Option 2: Build from Source

### 1. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

### 2. Build the Project

```bash
cd search-service
cargo build --release
```

### 3. Run the Service

```bash
./target/release/simple-search-service
```

Or with custom settings:

```bash
DATA_DIR=/var/lib/search PORT=3000 ./target/release/simple-search-service
```

## Running the Demo

We've included a comprehensive demo script:

```bash
chmod +x examples/demo.sh
./examples/demo.sh
```

This will:
- Create a sample index
- Add Norwegian kindergarten data
- Demonstrate various search queries
- Show bulk operations

## Laravel Integration

Copy the Laravel client from `examples/laravel-integration.php` to your Laravel app:

```bash
# In your Laravel project
cp examples/laravel-integration.php app/Services/SearchService.php
```

Add to `.env`:
```
SEARCH_SERVICE_URL=http://localhost:3000
```

Use in your controllers:
```php
$search = new SearchService();
$results = $search->search('products', 'laptop', 10);
```

## Production Deployment

### Linux Server with Systemd

1. Copy binary to server:
```bash
scp target/release/simple-search-service user@server:/opt/search-service/
```

2. Set up systemd service:
```bash
sudo cp examples/search-service.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable search-service
sudo systemctl start search-service
```

3. Set up Nginx reverse proxy:
```bash
sudo cp examples/nginx.conf /etc/nginx/sites-available/search-service
sudo ln -s /etc/nginx/sites-available/search-service /etc/nginx/sites-enabled/
sudo nginx -t
sudo systemctl reload nginx
```

## Troubleshooting

### Service won't start

Check logs:
```bash
# Docker
docker-compose logs

# Systemd
sudo journalctl -u search-service -f
```

### Port already in use

Change the port:
```bash
PORT=3001 ./target/release/simple-search-service
```

Or in docker-compose.yml:
```yaml
environment:
  - PORT=3001
ports:
  - "3001:3001"
```

### Permission denied for data directory

```bash
sudo chown -R $USER:$USER ./data
```

## Next Steps

- Read the full [README.md](README.md) for comprehensive documentation
- Check out [examples/demo.sh](examples/demo.sh) for more API examples
- See [examples/laravel-integration.php](examples/laravel-integration.php) for PHP integration
- Review [examples/nginx.conf](examples/nginx.conf) for production setup

## Getting Help

- Check the logs first
- Review the API documentation in README.md
- Ensure your data directory has proper permissions
- Verify the service is accessible: `curl http://localhost:3000/health`

## Common Use Cases

### E-commerce Product Search
```json
{
  "query": "laptop 16GB RAM",
  "limit": 20,
  "boost": {
    "title": 2.0
  }
}
```

### Location-Based Search
```json
{
  "query": "barnehage bergen",
  "fields": ["title", "description", "location"]
}
```

### Fuzzy Search (typo tolerance)
```json
{
  "query": "gaming",
  "fuzzy": true
}
```

Enjoy using Simple Search Service! 🚀
