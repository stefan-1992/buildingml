import type { SceneGraph, SemanticModel, Diagnostic } from './types.ts';

export interface ChatRequest {
  messages: Array<{ role: 'system' | 'user' | 'assistant'; content: string }>;
}

export interface ChatResponse {
  content: string;
}

export interface DataProvider {
  /** Generate 3D scene graph from BML source */
  generateScene(source: string): Promise<SceneGraph>;

  /** Generate semantic model from BML source (for floorplan editor) */
  generateModel(source: string): Promise<SemanticModel>;

  /** Lint BML source and return diagnostics */
  lint(source: string): Promise<Diagnostic[]>;

  /** Compile BML source to IFC STEP format */
  compile(source: string): Promise<string>;

  /** Get the current BML source (from file/server) */
  getSource(): Promise<string>;

  /** Save BML source back (single-file mode) */
  saveSource(source: string): Promise<void>;

  /** Send a chat message to the LLM and stream the response */
  chat(
    request: ChatRequest,
    onChunk: (chunk: string) => void,
  ): Promise<ChatResponse>;
}
