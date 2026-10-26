import type { DataProvider, ChatRequest, ChatResponse } from '../data-provider.ts';
import type { SceneGraph, SemanticModel, Diagnostic } from '../types.ts';

/**
 * DataProvider that talks to the buildingml preview server via REST API.
 * Used when running `buildingml preview` on localhost.
 */
export class RestProvider implements DataProvider {
  private baseUrl: string;

  constructor(baseUrl: string = '') {
    // Strip trailing slash
    this.baseUrl = baseUrl.replace(/\/$/, '');
  }

  async generateScene(source: string): Promise<SceneGraph> {
    const res = await fetch(`${this.baseUrl}/api/scene`, {
      method: 'POST',
      headers: { 'Content-Type': 'text/plain' },
      body: source,
    });
    if (!res.ok) throw new Error(`Scene generation failed: ${res.statusText}`);
    return res.json();
  }

  async generateModel(source: string): Promise<SemanticModel> {
    const res = await fetch(`${this.baseUrl}/api/model`, {
      method: 'POST',
      headers: { 'Content-Type': 'text/plain' },
      body: source,
    });
    if (!res.ok) throw new Error(`Model generation failed: ${res.statusText}`);
    return res.json();
  }

  async lint(source: string): Promise<Diagnostic[]> {
    const res = await fetch(`${this.baseUrl}/api/lint`, {
      method: 'POST',
      headers: { 'Content-Type': 'text/plain' },
      body: source,
    });
    if (!res.ok) throw new Error(`Lint failed: ${res.statusText}`);
    return res.json();
  }

  async compile(source: string): Promise<string> {
    const res = await fetch(`${this.baseUrl}/api/compile`, {
      method: 'POST',
      headers: { 'Content-Type': 'text/plain' },
      body: source,
    });
    if (!res.ok) throw new Error(`Compile failed: ${res.statusText}`);
    return res.text();
  }

  async getSource(): Promise<string> {
    const res = await fetch(`${this.baseUrl}/api/source`);
    if (!res.ok) throw new Error(`Failed to get source: ${res.statusText}`);
    return res.text();
  }

  async saveSource(source: string): Promise<void> {
    const res = await fetch(`${this.baseUrl}/api/source`, {
      method: 'POST',
      headers: { 'Content-Type': 'text/plain' },
      body: source,
    });
    if (!res.ok) throw new Error(`Failed to save source: ${res.statusText}`);
  }

  async chat(
    request: ChatRequest,
    onChunk: (chunk: string) => void,
  ): Promise<ChatResponse> {
    const res = await fetch(`${this.baseUrl}/api/chat`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(request),
    });
    if (!res.ok) {
      const text = await res.text();
      throw new Error(`Chat failed: ${text || res.statusText}`);
    }

    // Stream the response using SSE-style chunks
    const reader = res.body?.getReader();
    if (!reader) {
      const json = await res.json();
      return { content: json.content };
    }

    const decoder = new TextDecoder();
    let fullContent = '';
    let buffer = '';

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });

      // Parse SSE lines
      const lines = buffer.split('\n');
      buffer = lines.pop() || '';

      for (const line of lines) {
        if (line.startsWith('data: ')) {
          const data = line.slice(6);
          if (data === '[DONE]') continue;
          try {
            const parsed = JSON.parse(data);
            const delta = parsed.choices?.[0]?.delta?.content;
            if (delta) {
              fullContent += delta;
              onChunk(fullContent);
            }
          } catch {
            // Not JSON — might be raw text
          }
        }
      }
    }

    return { content: fullContent };
  }
}
