import requests
import json
import random
import time
from datetime import datetime, timedelta

BASE_URL = "http://localhost:3000"
INDEX_NAME = "large_dataset"
TOTAL_DOCS = 10000
BATCH_SIZE = 1000

def create_index():
    print(f"Creating index '{INDEX_NAME}'...")
    payload = {
        "name": INDEX_NAME,
        "fields": [
            {
                "name": "title",
                "field_type": "text",
                "stored": True,
                "indexed": True
            },
            {
                "name": "description",
                "field_type": "text",
                "stored": True,
                "indexed": True
            },
            {
                "name": "category",
                "field_type": "string",
                "stored": True,
                "indexed": True
            },
            {
                "name": "price",
                "field_type": "f64",
                "stored": True,
                "indexed": True
            },
            {
                "name": "created_at",
                "field_type": "date",
                "stored": True,
                "indexed": True,
                "fast": True
            }
        ]
    }
    
    # Try to delete first to ensure clean state
    requests.delete(f"{BASE_URL}/indices/{INDEX_NAME}")
    
    response = requests.post(f"{BASE_URL}/indices", json=payload)
    if response.status_code in [200, 201]:
        print("Index created successfully.")
    else:
        print(f"Failed to create index: {response.text}")
        exit(1)

def generate_document(doc_id):
    adjectives = ["Advanced", "Eco-friendly", "Durable", "Lightweight", "Premium", "Budget", "Smart", "Ergonomic"]
    nouns = ["Widget", "Gadget", "Tool", "Device", "System", "Solution", "Interface", "Module"]
    categories = ["Electronics", "Home", "Office", "Industrial", "Outdoor"]
    
    title = f"{random.choice(adjectives)} {random.choice(nouns)} {doc_id}"
    desc = f"This is a {title.lower()} designed for optimal performance. It features state-of-the-art technology and comes with a 2-year warranty."
    
    # Generate random date within last year
    days_ago = random.randint(0, 365)
    date = (datetime.now() - timedelta(days=days_ago)).isoformat() + "Z"
    
    return {
        "id": f"doc_{doc_id}",
        "fields": {
            "title": title,
            "description": desc,
            "category": random.choice(categories),
            "price": round(random.uniform(10.0, 1000.0), 2),
            "created_at": date
        }
    }

def populate():
    print(f"Generating and indexing {TOTAL_DOCS} documents...")
    start_time = time.time()
    
    for i in range(0, TOTAL_DOCS, BATCH_SIZE):
        batch = []
        for j in range(BATCH_SIZE):
            if i + j < TOTAL_DOCS:
                batch.append(generate_document(i + j))
        
        payload = {"documents": batch}
        response = requests.post(f"{BASE_URL}/indices/{INDEX_NAME}/documents", json=payload)
        
        if response.status_code in [200, 201]:
            print(f"Indexed docs {i} to {min(i + BATCH_SIZE, TOTAL_DOCS)}")
        else:
            print(f"Failed to index batch starting at {i}: {response.text}")
    
    elapsed = time.time() - start_time
    print(f"\nCompleted in {elapsed:.2f} seconds.")
    print(f"Average rate: {TOTAL_DOCS / elapsed:.2f} docs/sec")

if __name__ == "__main__":
    # Check if service is up
    try:
        requests.get(f"{BASE_URL}/health")
    except requests.exceptions.ConnectionError:
        print("Error: Search service is not running at http://localhost:3000")
        print("Please start the service first (e.g., ./target/release/simple-search-service)")
        exit(1)

    create_index()
    populate()
