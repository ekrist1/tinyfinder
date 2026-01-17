<?php

namespace App\Services;

use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\Log;

/**
 * Simple Search Service Client for Laravel
 * 
 * Usage:
 * $search = new SearchService();
 * $results = $search->search('products', 'laptop gaming');
 */
class SearchService
{
    private string $baseUrl;
    private int $timeout;

    public function __construct()
    {
        $this->baseUrl = config('services.search.url', 'http://localhost:3000');
        $this->timeout = config('services.search.timeout', 10);
    }

    /**
     * Create a new search index
     */
    public function createIndex(string $name, array $fields): array
    {
        $response = Http::timeout($this->timeout)
            ->post("{$this->baseUrl}/indices", [
                'name' => $name,
                'fields' => $fields,
            ]);

        if ($response->failed()) {
            Log::error('Failed to create search index', [
                'index' => $name,
                'status' => $response->status(),
                'body' => $response->body(),
            ]);
            throw new \Exception('Failed to create search index');
        }

        return $response->json();
    }

    /**
     * Add documents to an index
     */
    public function addDocuments(string $index, array $documents): array
    {
        $response = Http::timeout($this->timeout)
            ->post("{$this->baseUrl}/indices/{$index}/documents", [
                'documents' => $documents,
            ]);

        if ($response->failed()) {
            Log::error('Failed to add documents', [
                'index' => $index,
                'count' => count($documents),
                'status' => $response->status(),
            ]);
            throw new \Exception('Failed to add documents');
        }

        return $response->json();
    }

    /**
     * Search an index
     */
    public function search(
        string $index,
        string $query,
        int $limit = 10,
        array $fields = [],
        array $boost = []
    ): array {
        $payload = [
            'query' => $query,
            'limit' => $limit,
        ];

        if (!empty($fields)) {
            $payload['fields'] = $fields;
        }

        if (!empty($boost)) {
            $payload['boost'] = $boost;
        }

        $response = Http::timeout($this->timeout)
            ->post("{$this->baseUrl}/indices/{$index}/search", $payload);

        if ($response->failed()) {
            Log::error('Search failed', [
                'index' => $index,
                'query' => $query,
                'status' => $response->status(),
            ]);
            return [
                'success' => false,
                'data' => [
                    'took_ms' => 0,
                    'total' => 0,
                    'hits' => [],
                ],
            ];
        }

        return $response->json();
    }

    /**
     * Delete a document
     */
    public function deleteDocument(string $index, string $documentId): array
    {
        $response = Http::timeout($this->timeout)
            ->delete("{$this->baseUrl}/indices/{$index}/documents/{$documentId}");

        if ($response->failed()) {
            Log::error('Failed to delete document', [
                'index' => $index,
                'document_id' => $documentId,
                'status' => $response->status(),
            ]);
            throw new \Exception('Failed to delete document');
        }

        return $response->json();
    }

    /**
     * Bulk operations
     */
    public function bulk(string $index, array $operations): array
    {
        $response = Http::timeout($this->timeout)
            ->post("{$this->baseUrl}/indices/{$index}/bulk", [
                'operations' => $operations,
            ]);

        if ($response->failed()) {
            Log::error('Bulk operation failed', [
                'index' => $index,
                'operations_count' => count($operations),
                'status' => $response->status(),
            ]);
            throw new \Exception('Bulk operation failed');
        }

        return $response->json();
    }

    /**
     * List all indices
     */
    public function listIndices(): array
    {
        $response = Http::timeout($this->timeout)
            ->get("{$this->baseUrl}/indices");

        if ($response->failed()) {
            Log::error('Failed to list indices', [
                'status' => $response->status(),
            ]);
            return [
                'success' => false,
                'data' => [],
            ];
        }

        return $response->json();
    }

    /**
     * Delete an index
     */
    public function deleteIndex(string $index): array
    {
        $response = Http::timeout($this->timeout)
            ->delete("{$this->baseUrl}/indices/{$index}");

        if ($response->failed()) {
            Log::error('Failed to delete index', [
                'index' => $index,
                'status' => $response->status(),
            ]);
            throw new \Exception('Failed to delete index');
        }

        return $response->json();
    }

    /**
     * Health check
     */
    public function health(): array
    {
        $response = Http::timeout(5)
            ->get("{$this->baseUrl}/health");

        return $response->json();
    }
}

// Example Usage in a Controller:

namespace App\Http\Controllers;

use App\Services\SearchService;
use Illuminate\Http\Request;

class KindergartenSearchController extends Controller
{
    private SearchService $search;

    public function __construct(SearchService $search)
    {
        $this->search = $search;
    }

    public function search(Request $request)
    {
        $query = $request->input('q', '');
        $limit = $request->input('limit', 20);

        $results = $this->search->search(
            'kindergartens',
            $query,
            $limit,
            ['title', 'description', 'location'],
            ['title' => 2.0] // Boost title matches
        );

        if ($results['success']) {
            return response()->json([
                'query' => $query,
                'took_ms' => $results['data']['took_ms'],
                'results' => $results['data']['hits'],
            ]);
        }

        return response()->json([
            'error' => 'Search failed',
        ], 500);
    }

    public function sync()
    {
        // Example: Sync Eloquent models to search index
        $kindergartens = \App\Models\Kindergarten::all();

        $documents = $kindergartens->map(function ($kg) {
            return [
                'id' => "kg_{$kg->id}",
                'fields' => [
                    'title' => $kg->name,
                    'description' => $kg->description,
                    'location' => $kg->city,
                    'capacity' => $kg->capacity,
                ],
            ];
        })->toArray();

        $this->search->addDocuments('kindergartens', $documents);

        return response()->json([
            'message' => 'Synced ' . count($documents) . ' kindergartens',
        ]);
    }
}

// Config file: config/services.php
/*
return [
    'search' => [
        'url' => env('SEARCH_SERVICE_URL', 'http://localhost:3000'),
        'timeout' => env('SEARCH_SERVICE_TIMEOUT', 10),
    ],
];
*/

// .env
/*
SEARCH_SERVICE_URL=http://localhost:3000
SEARCH_SERVICE_TIMEOUT=10
*/
