# Generative Answers with SSE (JavaScript Example)

This guide shows how to call the `/indices/:name/answer` endpoint with SSE streaming and how to decide when to use it.

## SSE streaming example (JavaScript)

```javascript
const BASE_URL = 'http://localhost:3000';
const INDEX = 'kindergartens';

async function streamAnswer(query) {
  const response = await fetch(`${BASE_URL}/indices/${INDEX}/answer`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      query,
      search_limit: 5,
      fields: ['title', 'description', 'location'],
      fuzzy: true,
      stream: true,
      temperature: 0.2
    })
  });

  if (!response.ok || !response.body) {
    const errorText = await response.text();
    throw new Error(`Request failed: ${response.status} ${errorText}`);
  }

  const reader = response.body.getReader();
  const decoder = new TextDecoder('utf-8');
  let buffer = '';
  let answerText = '';
  let meta = null;

  while (true) {
    const { value, done } = await reader.read();
    if (done) break;

    buffer += decoder.decode(value, { stream: true });

    let idx;
    while ((idx = buffer.indexOf('\n')) !== -1) {
      const line = buffer.slice(0, idx).trimEnd();
      buffer = buffer.slice(idx + 1);

      if (!line) continue;

      if (line.startsWith('event:')) {
        // You can capture the event type if needed.
        continue;
      }

      if (line.startsWith('data:')) {
        const data = line.slice('data:'.length).trim();
        if (!data) continue;

        // Server sends `event: meta` with JSON data
        // and plain `data:` chunks for text.
        try {
          const maybeJson = JSON.parse(data);
          meta = maybeJson;
          console.log('meta', meta);
        } catch {
          answerText += data;
          // Render progressively in UI
          console.log('chunk', data);
        }
      }
    }
  }

  return { answerText, meta };
}

streamAnswer('hvor er familievennlig barnehage')
  .then(({ answerText, meta }) => {
    console.log('final answer', answerText);
    console.log('meta', meta);
  })
  .catch(console.error);
```

## Detecting questions vs. keyword searches

A simple heuristic is enough for most UX:

```javascript
const QUESTION_WORDS = [
  'hvor', 'hva', 'hvem', 'hvilken', 'hvilke', 'hvordan', 'når', 'hvorfor',
  'where', 'what', 'who', 'which', 'how', 'when', 'why'
];

function looksLikeQuestion(input) {
  const text = input.trim().toLowerCase();
  if (!text) return false;

  // Ends with a question mark
  if (text.endsWith('?')) return true;

  // Starts with a common question word
  return QUESTION_WORDS.some(word => text.startsWith(`${word} `));
}

function chooseEndpoint(query) {
  return looksLikeQuestion(query)
    ? 'answer'
    : 'search';
}
```

## Notes

- The endpoint returns `event: meta` containing `model`, `search_took_ms`, and `sources`.
- Text chunks arrive as normal `data:` lines. Concatenate them to build the final answer.
- If the LLM can’t answer from sources, it will respond with “I don’t know” (per system prompt).
