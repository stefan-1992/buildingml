import type { DataProvider, ChatRequest, ChatResponse } from '../data-provider.ts';
import type { SceneGraph, SemanticModel, Diagnostic } from '../types.ts';

/**
 * WASM function signatures expected on the global window object.
 * These are provided by wasm-bindgen JS glue code.
 */
interface WasmBindings {
  bml_lint(source: string): string;
  bml_compile(source: string): string;
  bml_generate_scene(source: string): string;
  bml_generate_model(source: string): string;
}

function getWasm(): WasmBindings {
  const w = window as unknown as Partial<WasmBindings>;
  if (!w.bml_generate_scene) {
    throw new Error('WASM module not loaded');
  }
  return w as WasmBindings;
}

/**
 * DataProvider that calls WASM bindings directly.
 * Used for the hosted web app (client-side, no server needed).
 */
export class WasmProvider implements DataProvider {
  async generateScene(source: string): Promise<SceneGraph> {
    const json = getWasm().bml_generate_scene(source);
    return JSON.parse(json);
  }

  async generateModel(source: string): Promise<SemanticModel> {
    const json = getWasm().bml_generate_model(source);
    return JSON.parse(json);
  }

  async lint(source: string): Promise<Diagnostic[]> {
    const json = getWasm().bml_lint(source);
    return JSON.parse(json);
  }

  async compile(source: string): Promise<string> {
    return getWasm().bml_compile(source);
  }

  async getSource(): Promise<string> {
    // No server — return empty, let the app use default source
    return '';
  }

  async saveSource(_source: string): Promise<void> {
    // No server — save to localStorage as fallback
    try {
      localStorage.setItem('buildingml-source', _source);
    } catch {
      // Ignore storage errors
    }
  }

  async chat(
    _request: ChatRequest,
    _onChunk: (chunk: string) => void,
  ): Promise<ChatResponse> {
    // WASM provider has no server — chat requires a server-side API
    throw new Error('Chat is not available in WASM-only mode. Use the server (buildingml serve) for chat.');
  }
}
