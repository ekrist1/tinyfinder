.PHONY: help build run test clean docker-build docker-up docker-down install deploy

# Default target
help:
	@echo "Simple Search Service - Makefile"
	@echo ""
	@echo "Available targets:"
	@echo "  make build         - Build the project in release mode"
	@echo "  make build-debug   - Build the project in debug mode"
	@echo "  make run           - Run the service locally"
	@echo "  make test          - Run tests"
	@echo "  make clean         - Clean build artifacts"
	@echo "  make docker-build  - Build Docker image"
	@echo "  make docker-up     - Start with Docker Compose"
	@echo "  make docker-down   - Stop Docker Compose"
	@echo "  make demo          - Run the demo script"
	@echo "  make install       - Install to /usr/local/bin"
	@echo "  make check         - Run clippy and formatting checks"
	@echo ""

# Build targets
build:
	@echo "Building in release mode..."
	cargo build --release
	@echo "Binary: target/release/simple-search-service"

build-debug:
	@echo "Building in debug mode..."
	cargo build
	@echo "Binary: target/debug/simple-search-service"

# Run the service
run:
	@echo "Starting Simple Search Service..."
	@mkdir -p data
	cargo run --release

# Test
test:
	@echo "Running tests..."
	cargo test

# Clean
clean:
	@echo "Cleaning build artifacts..."
	cargo clean
	rm -rf data/

# Docker targets
docker-build:
	@echo "Building Docker image..."
	docker build -t simple-search-service:latest .

docker-up:
	@echo "Starting with Docker Compose..."
	docker-compose up -d
	@echo "Service available at http://localhost:3000"
	@echo "Check status: docker-compose ps"
	@echo "View logs: docker-compose logs -f"

docker-down:
	@echo "Stopping Docker Compose..."
	docker-compose down

docker-logs:
	docker-compose logs -f

# Demo
demo:
	@echo "Running demo script..."
	@chmod +x examples/demo.sh
	./examples/demo.sh

# Install system-wide
install: build
	@echo "Installing to /usr/local/bin..."
	sudo cp target/release/simple-search-service /usr/local/bin/
	@echo "Installed. Run with: simple-search-service"

# Code quality
check:
	@echo "Running clippy..."
	cargo clippy -- -D warnings
	@echo "Checking formatting..."
	cargo fmt --check

fmt:
	@echo "Formatting code..."
	cargo fmt

# Development helpers
dev:
	@echo "Starting in development mode with auto-reload..."
	cargo watch -x run

# Quick build and test
quick: build-debug test
	@echo "Quick build and test complete!"

# Full CI pipeline
ci: check test build
	@echo "CI pipeline complete!"

# Create release binary for distribution
release: build
	@echo "Creating release package..."
	@mkdir -p release
	cp target/release/simple-search-service release/
	cp README.md QUICKSTART.md LICENSE release/
	cp -r examples release/
	cd release && tar -czf simple-search-service-linux-x86_64.tar.gz *
	@echo "Release package: release/simple-search-service-linux-x86_64.tar.gz"

# Health check
health:
	@echo "Checking service health..."
	@curl -s http://localhost:3000/health | jq '.' || echo "Service not running or not responding"

# Show current indices
indices:
	@echo "Listing indices..."
	@curl -s http://localhost:3000/indices | jq '.data' || echo "Service not running"
