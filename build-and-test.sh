#!/bin/bash

set -e

echo "🔨 Simple Search Service - Build & Test Script"
echo "==============================================="
echo ""

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print colored output
print_success() {
    echo -e "${GREEN}✓ $1${NC}"
}

print_error() {
    echo -e "${RED}✗ $1${NC}"
}

print_info() {
    echo -e "${YELLOW}ℹ $1${NC}"
}

# Check if Rust is installed
if ! command -v cargo &> /dev/null; then
    print_error "Rust is not installed. Please install from https://rustup.rs/"
    exit 1
fi

print_success "Rust is installed: $(rustc --version)"
echo ""

# Clean previous builds
print_info "Cleaning previous builds..."
cargo clean
print_success "Clean complete"
echo ""

# Run tests (if any are added)
print_info "Running tests..."
cargo test --quiet
print_success "Tests passed"
echo ""

# Build in debug mode
print_info "Building in debug mode..."
cargo build
print_success "Debug build complete"
echo ""

# Build in release mode
print_info "Building in release mode (optimized)..."
cargo build --release
BINARY_SIZE=$(du -h target/release/simple-search-service | cut -f1)
print_success "Release build complete (binary size: $BINARY_SIZE)"
echo ""

# Check if binary works
print_info "Checking binary..."
if [ -f "target/release/simple-search-service" ]; then
    print_success "Binary exists at: target/release/simple-search-service"
else
    print_error "Binary not found!"
    exit 1
fi
echo ""

# Start the service for testing
print_info "Starting service for testing..."
DATA_DIR="/tmp/search-test-$$" PORT=3333 target/release/simple-search-service &
SERVICE_PID=$!
print_info "Service started with PID: $SERVICE_PID"
echo ""

# Wait for service to start
print_info "Waiting for service to be ready..."
sleep 2

# Test health endpoint
print_info "Testing health endpoint..."
HEALTH_RESPONSE=$(curl -s http://localhost:3333/health)
if echo "$HEALTH_RESPONSE" | grep -q "healthy"; then
    print_success "Health check passed"
else
    print_error "Health check failed"
    kill $SERVICE_PID
    exit 1
fi
echo ""

# Test creating an index
print_info "Testing index creation..."
CREATE_RESPONSE=$(curl -s -X POST http://localhost:3333/indices \
    -H "Content-Type: application/json" \
    -d '{"name":"test_index","fields":[{"name":"title","field_type":"text","stored":true,"indexed":true}]}')

if echo "$CREATE_RESPONSE" | grep -q "success.*true"; then
    print_success "Index creation passed"
else
    print_error "Index creation failed"
    echo "$CREATE_RESPONSE"
    kill $SERVICE_PID
    exit 1
fi
echo ""

# Test adding documents
print_info "Testing document addition..."
DOC_RESPONSE=$(curl -s -X POST http://localhost:3333/indices/test_index/documents \
    -H "Content-Type: application/json" \
    -d '{"documents":[{"id":"doc1","fields":{"title":"Test Document"}}]}')

if echo "$DOC_RESPONSE" | grep -q "success.*true"; then
    print_success "Document addition passed"
else
    print_error "Document addition failed"
    echo "$DOC_RESPONSE"
    kill $SERVICE_PID
    exit 1
fi
echo ""

# Test search
print_info "Testing search..."
SEARCH_RESPONSE=$(curl -s -X POST http://localhost:3333/indices/test_index/search \
    -H "Content-Type: application/json" \
    -d '{"query":"test","limit":10}')

if echo "$SEARCH_RESPONSE" | grep -q "took_ms"; then
    print_success "Search passed"
else
    print_error "Search failed"
    echo "$SEARCH_RESPONSE"
    kill $SERVICE_PID
    exit 1
fi
echo ""

# Cleanup
print_info "Cleaning up test service..."
kill $SERVICE_PID
rm -rf "/tmp/search-test-$$"
print_success "Cleanup complete"
echo ""

# Summary
echo "=========================================="
echo -e "${GREEN}✓ All tests passed!${NC}"
echo ""
echo "📦 Binary location: target/release/simple-search-service"
echo "📊 Binary size: $BINARY_SIZE"
echo ""
echo "To run the service:"
echo "  ./target/release/simple-search-service"
echo ""
echo "Or with Docker:"
echo "  docker-compose up -d"
echo ""
echo "=========================================="
