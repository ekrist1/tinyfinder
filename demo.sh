#!/bin/bash

# Simple Search Service - Example Usage Script
# This script demonstrates how to use the API

BASE_URL="http://localhost:3000"

echo "🚀 Simple Search Service - Demo Script"
echo "========================================"
echo ""

# Health check
echo "1️⃣  Checking service health..."
curl -s "$BASE_URL/health" | jq '.'
echo ""

# Create index
echo "2️⃣  Creating 'kindergartens' index..."
curl -s -X POST "$BASE_URL/indices" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "kindergartens",
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
        "name": "location",
        "field_type": "text",
        "stored": true,
        "indexed": true
      },
      {
        "name": "capacity",
        "field_type": "i64",
        "stored": true,
        "indexed": true
      }
    ]
  }' | jq '.'
echo ""

# Add documents
echo "3️⃣  Adding sample documents..."
curl -s -X POST "$BASE_URL/indices/kindergartens/documents" \
  -H "Content-Type: application/json" \
  -d '{
    "documents": [
      {
        "id": "kg_001",
        "fields": {
          "title": "Smil Barnehage Bergen",
          "description": "Modern barnehage i Bergen sentrum med fokus på læring gjennom lek. Vi har erfarne pedagoger og flotte lokaler.",
          "location": "Bergen",
          "capacity": 45
        }
      },
      {
        "id": "kg_002",
        "fields": {
          "title": "Lekeland Barnehage",
          "description": "Familievennlig barnehage med store uteområder i Fyllingsdalen. Fokus på natur og uteliv.",
          "location": "Bergen",
          "capacity": 60
        }
      },
      {
        "id": "kg_003",
        "fields": {
          "title": "Solstrålen Barnehage Oslo",
          "description": "Internasjonal barnehage i Oslo sentrum. Vi tilbyr tospråklig opplæring og kulturell mangfold.",
          "location": "Oslo",
          "capacity": 50
        }
      },
      {
        "id": "kg_004",
        "fields": {
          "title": "Eventyrskogen Barnehage",
          "description": "Naturbarnehage i Stavanger med mye tid utendørs. Vi følger årstidene og lærer om norsk natur.",
          "location": "Stavanger",
          "capacity": 40
        }
      },
      {
        "id": "kg_005",
        "fields": {
          "title": "Blå Himmel Barnehage Bergen",
          "description": "Nyoppstartet barnehage i Åsane med moderne pedagogikk. Små grupper og personlig oppfølging.",
          "location": "Bergen",
          "capacity": 35
        }
      }
    ]
  }' | jq '.'
echo ""

# List indices
echo "4️⃣  Listing all indices..."
curl -s "$BASE_URL/indices" | jq '.'
echo ""

# Search example 1
echo "5️⃣  Searching for 'barnehage bergen'..."
curl -s -X POST "$BASE_URL/indices/kindergartens/search" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "barnehage bergen",
    "limit": 5,
    "fields": ["title", "description", "location"]
  }' | jq '.'
echo ""

# Search example 2
echo "6️⃣  Searching for 'natur' (nature-focused kindergartens)..."
curl -s -X POST "$BASE_URL/indices/kindergartens/search" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "natur",
    "limit": 3
  }' | jq '.'
echo ""

# Search example 3
echo "7️⃣  Searching with field boost (prioritize title matches)..."
curl -s -X POST "$BASE_URL/indices/kindergartens/search" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "oslo",
    "limit": 5,
    "boost": {
      "title": 2.0
    }
  }' | jq '.'
echo ""

# Bulk operations
echo "8️⃣  Performing bulk operations..."
curl -s -X POST "$BASE_URL/indices/kindergartens/bulk" \
  -H "Content-Type: application/json" \
  -d '{
    "operations": [
      {
        "operation": "index",
        "document": {
          "id": "kg_006",
          "fields": {
            "title": "Regnbuen Barnehage",
            "description": "Inkluderende barnehage i Trondheim",
            "location": "Trondheim",
            "capacity": 55
          }
        }
      },
      {
        "operation": "delete",
        "id": "kg_001"
      }
    ]
  }' | jq '.'
echo ""

echo "✅ Demo completed!"
echo ""
echo "Try your own searches:"
echo "curl -X POST $BASE_URL/indices/kindergartens/search \\"
echo "  -H 'Content-Type: application/json' \\"
echo "  -d '{\"query\": \"your search here\", \"limit\": 10}' | jq '.'"
