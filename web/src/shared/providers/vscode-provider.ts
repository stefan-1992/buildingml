import type { DataProvider, ChatRequest, ChatResponse } from '../data-provider.ts';
import type { SceneGraph, SemanticModel, Diagnostic } from '../types.ts';

/**
 * DataProvider for VS Code webview that uses postMessage bridge.
 * The extension handles compilation via CLI subprocess and sends results back.
 */
export class VsCodeProvider implements DataProvider {
  private vscode: { postMessage(msg: unknown): void };
  private pendingRequests = new Map<string, {
    resolve: (value: unknown) => void;
    reject: (reason: unknown) => void;
  }>();
  private requestId = 0;

  constructor() {
    // acquireVsCodeApi is available in VS Code webviews
    this.vscode = (window as unknown as { acquireVsCodeApi(): { postMessage(msg: unknown): void } }).acquireVsCodeApi();

    window.addEventListener('message', (event) => {
      const msg = event.data;
      if (msg.requestId && this.pendingRequests.has(msg.requestId)) {
        const { resolve, reject } = this.pendingRequests.get(msg.requestId)!;
        this.pendingRequests.delete(msg.requestId);
        if (msg.error) {
          reject(new Error(msg.error));
        } else {
          resolve(msg.result);
        }
      }
    });
  }

  private request(type: string, payload?: unknown): Promise<unknown> {
    const id = `req-${++this.requestId}`;
    return new Promise((resolve, reject) => {
      this.pendingRequests.set(id, { resolve, reject });
      this.vscode.postMessage({ type, requestId: id, ...payload as object });
      // Timeout after 15s
      setTimeout(() => {
        if (this.pendingRequests.has(id)) {
          this.pendingRequests.delete(id);
          reject(new Error(`Request ${type} timed out`));
        }
      }, 15000);
    });
  }

  async generateScene(source: string): Promise<SceneGraph> {
    return this.request('generateScene', { source }) as Promise<SceneGraph>;
  }

  async generateModel(source: string): Promise<SemanticModel> {
    return this.request('generateModel', { source }) as Promise<SemanticModel>;
  }

  async lint(source: string): Promise<Diagnostic[]> {
    return this.request('lint', { source }) as Promise<Diagnostic[]>;
  }

  async compile(source: string): Promise<string> {
    return this.request('compile', { source }) as Promise<string>;
  }

  async getSource(): Promise<string> {
    return this.request('getSource') as Promise<string>;
  }

  async saveSource(source: string): Promise<void> {
    await this.request('saveSource', { source });
  }

  async chat(
    _request: ChatRequest,
    _onChunk: (chunk: string) => void,
  ): Promise<ChatResponse> {
    // VS Code extension does not support chat yet
    throw new Error('Chat is not available in VS Code extension mode.');
  }
}
