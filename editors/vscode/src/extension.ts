import * as path from "path";
import * as vscode from "vscode";
import * as cp from "child_process";
import * as fs from "fs";
import * as os from "os";
import * as crypto from "crypto";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export function activate(context: vscode.ExtensionContext) {
  const config = vscode.workspace.getConfiguration("buildingml");
  const serverPath = config.get<string>("server.path", "buildingml");

  // Server options — run the BuildingML binary with 'lsp' subcommand
  const serverOptions: ServerOptions = {
    command: serverPath,
    args: ["lsp"],
    transport: TransportKind.stdio,
  };

  // Client options
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "buildingml" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.bml"),
    },
  };

  // Create the language client
  client = new LanguageClient(
    "buildingml",
    "BuildingML Language Server",
    serverOptions,
    clientOptions
  );

  // Start the client (also starts the server)
  client.start();

  // Register commands
  context.subscriptions.push(
    vscode.commands.registerCommand("buildingml.restartServer", async () => {
      if (client) {
        await client.restart();
        vscode.window.showInformationMessage(
          "BuildingML language server restarted"
        );
      }
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("buildingml.preview", () => {
      PreviewPanel.createOrShow(context.extensionUri, serverPath);
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("buildingml.exportIfc", async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || editor.document.languageId !== "buildingml") {
        vscode.window.showWarningMessage("No BuildingML file is open");
        return;
      }

      const bmlUri = editor.document.uri;
      const defaultName = path.basename(bmlUri.fsPath, ".bml") + ".ifc";
      const defaultUri = vscode.Uri.file(
        path.join(path.dirname(bmlUri.fsPath), defaultName)
      );

      const saveUri = await vscode.window.showSaveDialog({
        defaultUri,
        filters: { "IFC Files": ["ifc"], "All Files": ["*"] },
        title: "Export IFC File",
      });
      if (!saveUri) { return; }

      const source = editor.document.getText();
      const tmpFile = path.join(
        require("os").tmpdir(),
        `bml-export-${Date.now()}.bml`
      );

      try {
        require("fs").writeFileSync(tmpFile, source);
        cp.execSync(
          `${serverPath} compile "${tmpFile}" -o "${saveUri.fsPath}"`,
          { encoding: "utf-8", timeout: 30000 }
        );
        vscode.window.showInformationMessage(
          `IFC exported to ${path.basename(saveUri.fsPath)}`
        );
      } catch (err: any) {
        const message = err.stderr
          ? err.stderr.toString()
          : String(err.message || err);
        vscode.window.showErrorMessage(`IFC export failed: ${message}`);
      } finally {
        try { require("fs").unlinkSync(tmpFile); } catch (_) {}
      }
    })
  );
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}

/**
 * Preview Panel — renders BuildingML files in a 3D WebGL viewer or 2D floorplan.
 *
 * Uses the `buildingml` CLI to generate scene JSON (3D) and model JSON (2D),
 * which are sent to a webview. The 2D view tracks cursor position to auto-select
 * the active storey.
 */
/** A document the preview can render: BuildingML or an .ifc file. */
type PreviewKind = "bml" | "ifc" | null;
function previewKind(doc: vscode.TextDocument): PreviewKind {
  if (doc.languageId === "buildingml") { return "bml"; }
  const p = doc.uri.fsPath;
  if (p.endsWith(".ifc")) { return "ifc"; }
  return null;
}

/**
 * The .ifc file in a focused tab. Read from the tabs rather than the text
 * editor: VS Code does not hand files above 50 MB to extensions, so a large IFC
 * never becomes `activeTextEditor`.
 */
function activeIfcTab(anyGroup: boolean): vscode.Uri | undefined {
  const { activeTabGroup, all } = vscode.window.tabGroups;
  for (const group of anyGroup ? [activeTabGroup, ...all] : [activeTabGroup]) {
    const input = group.activeTab?.input;
    if (input instanceof vscode.TabInputText && input.uri.scheme === "file" && input.uri.fsPath.endsWith(".ifc")) {
      return input.uri;
    }
  }
  return undefined;
}

/** Render timings and webview errors, for diagnosing large models. */
const log = vscode.window.createOutputChannel("BuildingML Preview");

/** Geometry bytes per webview message when posting a large scene. */
const CHUNK_BYTES = 32 * 1024 * 1024;

/** 4-byte words a node and its children take once packed. */
function packedWords(node: any): number {
  let words = node.mesh.vertices.length * 6 + node.mesh.indices.length;
  for (const child of node.children ?? []) {
    words += packedWords(child);
  }
  return words;
}

/**
 * Replace a scene node's vertex objects with typed-array views into `buf`,
 * starting at word `at`, recursively. Returns the word after the last view.
 */
function packNode(node: any, buf: ArrayBuffer, at: number): number {
  const mesh = node.mesh;
  const verts = mesh.vertices;
  const positions = new Float32Array(buf, at * 4, verts.length * 3);
  const normals = new Float32Array(buf, (at + verts.length * 3) * 4, verts.length * 3);
  at += verts.length * 6;
  for (let i = 0; i < verts.length; i++) {
    const p = verts[i].position;
    const n = verts[i].normal;
    const j = i * 3;
    positions[j] = p[0]; positions[j + 1] = p[1]; positions[j + 2] = p[2];
    normals[j] = n[0]; normals[j + 1] = n[1]; normals[j + 2] = n[2];
  }
  const indices = new Uint32Array(buf, at * 4, mesh.indices.length);
  indices.set(mesh.indices);
  at += indices.length;
  mesh.positions = positions;
  mesh.normals = normals;
  mesh.vertices = [];
  mesh.indices = indices;
  for (const child of node.children ?? []) {
    at = packNode(child, buf, at);
  }
  return at;
}

/** Pack nodes into views of a single ArrayBuffer. */
function packNodes(nodes: any[], words: number): void {
  const buf = new ArrayBuffer(words * 4);
  let at = 0;
  for (const node of nodes) {
    at = packNode(node, buf, at);
  }
}

class PreviewPanel {
  public static currentPanel: PreviewPanel | undefined;
  private static readonly viewType = "buildingml.preview";

  private readonly panel: vscode.WebviewPanel;
  private readonly serverPath: string;
  private disposables: vscode.Disposable[] = [];

  // Cached data for cursor-only changes (avoid re-running CLI)
  private cachedModel: any = null;
  private cachedSource: string = "";

  // Bumped on every update. An async render compares its own token before
  // posting, so a save that lands mid-render discards the older result rather
  // than letting the two race to the webview.
  private renderToken = 0;
  // Document being rendered; the webview keeps its filters while this is unchanged.
  private renderSource = "";

  // Inputs of whatever the webview is showing or rendering. Switching tabs fires
  // an update even when nothing changed, and on a large model that is a full
  // rebuild for a scene already on screen or on its way.
  private lastRenderKey: string | null = null;

  public static createOrShow(extensionUri: vscode.Uri, serverPath: string) {
    const column = vscode.ViewColumn.Beside;

    if (PreviewPanel.currentPanel) {
      PreviewPanel.currentPanel.panel.reveal(column);
      // Explicit command: render even if the inputs look unchanged, so the
      // button is also the way to force a refresh.
      PreviewPanel.currentPanel.update(true);
      return;
    }

    const panel = vscode.window.createWebviewPanel(
      PreviewPanel.viewType,
      "BuildingML Preview",
      column,
      {
        enableScripts: true,
        retainContextWhenHidden: true,
        localResourceRoots: [vscode.Uri.joinPath(extensionUri, "media")],
      }
    );

    PreviewPanel.currentPanel = new PreviewPanel(panel, serverPath, extensionUri);
  }

  private constructor(panel: vscode.WebviewPanel, serverPath: string, extensionUri: vscode.Uri) {
    this.panel = panel;
    this.serverPath = serverPath;

    this.panel.webview.html = this.getHtml(extensionUri);

    // Update when active editor changes
    this.disposables.push(
      vscode.window.onDidChangeActiveTextEditor((editor) => {
        if (editor && previewKind(editor.document)) {
          this.update();
        }
      })
    );

    // A large IFC opens as a tab without a text editor
    this.disposables.push(
      vscode.window.tabGroups.onDidChangeTabs(() => {
        if (activeIfcTab(false)) { this.update(); }
      })
    );

    // Update on document save
    this.disposables.push(
      vscode.workspace.onDidSaveTextDocument((doc) => {
        if (previewKind(doc)) { this.update(); }
      })
    );

    // Track cursor position for 2D storey detection
    this.disposables.push(
      vscode.window.onDidChangeTextEditorSelection((event) => {
        if (event.textEditor.document.languageId === "buildingml") {
          this.updateCursorStorey(event.textEditor);
        }
      })
    );

    // Handle panel disposal
    this.panel.onDidDispose(() => {
      PreviewPanel.currentPanel = undefined;
      for (const d of this.disposables) {
        d.dispose();
      }
    });

    // Handle messages from webview
    this.panel.webview.onDidReceiveMessage(
      (message) => {
        if (message?.type === "log") { log.appendLine(`[webview] ${message.text}`); }
      },
      null,
      this.disposables
    );

    // Initial render; the panel itself is now the focused tab
    this.update(true);
  }

  /**
   * Determine which storey the cursor is in using span data from the cached model.
   * Only sends a lightweight message (no CLI call) when just the cursor moves.
   */
  private updateCursorStorey(editor: vscode.TextEditor) {
    if (!this.cachedModel) { return; }

    // VS Code lines are 0-based, tree-sitter spans are 0-based
    const cursorLine = editor.selection.active.line;

    this.panel.webview.postMessage({ type: "updateCursorStorey", cursorLine });
  }

  /**
   * A fingerprint of everything a render reads, so an update that would produce
   * the identical scene can be skipped.
   *
   * Saved files are keyed by mtime and size rather than by content: hashing a
   * 10 MB IFC on every tab switch would give back
   * some of what the skip is meant to save. Unsaved buffers have no mtime worth
   * trusting, so those are hashed.
   */
  private inputKey(uri: vscode.Uri, kind: PreviewKind, doc?: vscode.TextDocument): string {
    const file = uri.fsPath;
    if (doc && uri.scheme !== "file") {
      return `${kind}:${uri.toString()}:${doc.version}`;
    }
    if (doc?.isDirty) {
      const h = crypto.createHash("sha1").update(doc.getText()).digest("hex");
      return `${kind}:${file}:dirty:${h}`;
    }
    const stamp = (f: string): string => {
      try {
        const st = fs.statSync(f);
        return `${st.mtimeMs}:${st.size}`;
      } catch {
        return "-";
      }
    };
    return `${kind}:${file}|${stamp(file)}`;
  }

  /**
   * Run a command without blocking the extension host. The synchronous form
   * freezes the whole window for the duration, which on a large model is tens
   * of seconds of unresponsive UI.
   */
  private run(command: string, args: string[]): Promise<string> {
    return new Promise((resolve, reject) => {
      cp.execFile(
        command,
        args,
        { encoding: "utf-8", timeout: 180000, maxBuffer: 512 * 1024 * 1024 },
        (err, stdout, stderr) => {
          if (err) {
            const detail = (stderr || "").toString().trim();
            reject(new Error(detail || String(err.message || err)));
            return;
          }
          resolve(stdout);
        }
      );
    });
  }

  /** Show a cancellable-looking status while a render runs. */
  private withProgress<T>(title: string, work: () => Promise<T>): Thenable<T> {
    return vscode.window.withProgress(
      { location: vscode.ProgressLocation.Window, title },
      work
    );
  }

  /**
   * Post a finished scene payload, unless a newer render has started.
   *
   * Meshes go out packed into typed arrays (sent as binary) and split over
   * several `sceneChunk` messages. As one JSON message, a large IFC (260 MB
   * for the West Riverside Hospital architecture model) never reaches the
   * webview. Each message carries one ArrayBuffer: VS Code's RPC writes the
   * buffer count of a message as a uint8, so a message with 255+ buffers
   * arrives truncated or fails.
   */
  private postScene(payload: any, token: number) {
    if (token !== this.renderToken) { return; }
    const scene = payload.scene ?? payload;
    let batch: any[] = [];
    let words = 0;
    for (const node of scene.nodes) {
      batch.push(node);
      words += packedWords(node);
      if (words * 4 >= CHUNK_BYTES) {
        packNodes(batch, words);
        this.panel.webview.postMessage({ type: "sceneChunk", nodes: batch });
        batch = [];
        words = 0;
      }
    }
    packNodes(batch, words);
    this.panel.webview.postMessage({
      type: "updateScene",
      scene: { ...scene, nodes: batch },
      source: this.renderSource,
    });
    if (payload.model) {
      this.cachedModel = payload.model;
      this.panel.webview.postMessage({
        type: "updateModel",
        model: payload.model,
        cursorLine: -1,
      });
    }
  }

  private postError(err: any, token: number) {
    if (token !== this.renderToken) { return; }
    // Leave no key behind, so the next update retries instead of skipping.
    this.lastRenderKey = null;
    this.panel.webview.postMessage({
      type: "error",
      message: String(err?.message || err),
    });
  }

  private update(force = false) {
    const editor = vscode.window.activeTextEditor;
    const doc = editor && previewKind(editor.document) ? editor.document : undefined;
    // Only an explicit render looks past the focused tab group, so focusing
    // another file never swaps the preview to an IFC open elsewhere
    const uri = doc?.uri ?? activeIfcTab(force);
    if (!uri) { return; }
    const kind = doc ? previewKind(doc) : "ifc";

    // The webview is already showing exactly this. Re-rendering it would cost a
    // full rebuild to arrive at the same picture.
    const key = this.inputKey(uri, kind, doc);
    if (!force && key === this.lastRenderKey) { return; }
    this.lastRenderKey = key;

    this.renderToken++;
    this.renderSource = uri.toString();
    if (kind === "ifc") {
      this.updateFromIfc(uri);
    } else if (editor) {
      this.updateFromBml(editor);
    }
  }

  private updateFromBml(editor: vscode.TextEditor) {
    const source = editor.document.getText();
    const docUri = editor.document.uri;

    // Use the original file path when saved so relative imports resolve correctly.
    // Fall back to a temp file only for unsaved buffers, placed next to the original
    // so imports still resolve from the correct directory.
    const originalPath = docUri.scheme === "file" ? docUri.fsPath : undefined;
    const tmpFile =
      !originalPath || editor.document.isDirty
        ? path.join(
            originalPath ? path.dirname(originalPath) : os.tmpdir(),
            `.bml-preview-${Date.now()}.bml`
          )
        : undefined;
    const targetFile = tmpFile ?? originalPath!;
    const token = this.renderToken;

    (async () => {
      try {
        if (tmpFile) {
          fs.writeFileSync(tmpFile, source);
        }

        // Generate scene (3D)
        const scene = JSON.parse(await this.run(this.serverPath, ["scene", targetFile]));
        if (token !== this.renderToken) { return; }
        this.panel.webview.postMessage({ type: "updateScene", scene, source: this.renderSource });

        // Generate model (2D) — cache it for cursor-only updates
        try {
          const model = JSON.parse(await this.run(this.serverPath, ["model", targetFile]));
          if (token !== this.renderToken) { return; }
          this.cachedModel = model;
          this.cachedSource = source;

          this.panel.webview.postMessage({
            type: "updateModel",
            model: model,
            cursorLine: editor.selection.active.line,
          });
        } catch (modelErr: any) {
          // Model generation failure is non-fatal; 3D still works
          console.error("Model generation failed:", modelErr.message);
        }
      } catch (err) {
        this.postError(err, token);
      } finally {
        if (tmpFile) {
          try { fs.unlinkSync(tmpFile); } catch { /* already gone */ }
        }
      }
    })();
  }

  /**
   * Render an .ifc file directly via the native `buildingml ifc-scene` reader
   * (ifc-lite).
   */
  private updateFromIfc(docUri: vscode.Uri) {
    if (docUri.scheme !== "file") {
      this.panel.webview.postMessage({ type: "error", message: "Save the file before previewing." });
      return;
    }
    const token = this.renderToken;
    this.withProgress(`Reading ${path.basename(docUri.fsPath)}`, async () => {
      try {
        const t0 = Date.now();
        let out = await this.run(this.serverPath, ["ifc-scene", docUri.fsPath]);
        const t1 = Date.now();
        const mb = (out.length / 1e6).toFixed(0);
        const payload = JSON.parse(out);
        out = ""; // let the 100s of MB of JSON text go before packing
        const t2 = Date.now();
        this.postScene(payload, token);
        log.appendLine(`${path.basename(docUri.fsPath)}: ifc-scene ${t1 - t0} ms (${mb} MB), parse ${t2 - t1} ms, pack+post ${Date.now() - t2} ms`);
      } catch (err) {
        log.appendLine(`${path.basename(docUri.fsPath)}: ${String((err as any)?.message || err)}`);
        this.postError(err, token);
      }
    });
  }

  private getHtml(extensionUri: vscode.Uri): string {
    const scriptUri = this.panel.webview.asWebviewUri(
      vscode.Uri.joinPath(extensionUri, "media", "shared-renderer.js")
    );

    return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src ${this.panel.webview.cspSource} 'unsafe-inline'; style-src 'unsafe-inline';">
<style>
  body { margin: 0; background: #1a1a2e; overflow: hidden; }
  canvas { width: 100vw; height: 100vh; display: block; }
  .hidden { display: none !important; }
  #status {
    position: fixed; top: 8px; left: 8px;
    color: rgba(255,255,255,0.6); font-family: monospace; font-size: 11px;
    pointer-events: none; z-index: 10;
  }
  #controls {
    position: fixed; bottom: 8px; left: 8px;
    color: rgba(255,255,255,0.4); font-family: monospace; font-size: 10px;
    pointer-events: none; z-index: 10;
  }
  #view-toggle {
    position: fixed; top: 8px; right: 8px;
    display: flex; gap: 8px; z-index: 20;
    font-family: monospace; font-size: 11px;
  }
  /* The 3D/2D pair reads as one segmented control; Filter is separate, so
     hiding it in 2D must not leave the 2D button with square corners. */
  #mode-group { display: flex; gap: 0; }
  #view-toggle button {
    background: rgba(0,0,0,0.55); color: rgba(255,255,255,0.6);
    border: 1px solid rgba(255,255,255,0.15); padding: 4px 12px;
    cursor: pointer; outline: none;
  }
  #mode-group button:first-child { border-radius: 4px 0 0 4px; }
  #mode-group button:last-child { border-radius: 0 4px 4px 0; }
  #btn-filters { border-radius: 4px; }
  #view-toggle button.active {
    background: rgba(137,180,250,0.25); color: rgba(255,255,255,0.9);
    border-color: rgba(137,180,250,0.5);
  }
  #storey-label {
    position: fixed; top: 36px; right: 8px;
    color: rgba(137,180,250,0.8); font-family: monospace; font-size: 11px;
    z-index: 10; pointer-events: none;
  }
  .cut-panel {
    display: none; align-items: center; gap: 6px;
    background: rgba(0,0,0,0.55); padding: 6px 10px;
    border-radius: 6px; font-family: monospace; font-size: 11px;
    color: rgba(255,255,255,0.75);
  }
  .cut-panel label { cursor: pointer; user-select: none; }
  .cut-panel input[type=checkbox] { margin: 0; cursor: pointer; }
  .cut-panel input[type=range] { cursor: pointer; }
  .cut-label { min-width: 48px; text-align: right; }
  #elevation-cut {
    position: fixed; right: 12px; top: 50%; transform: translateY(-50%);
    flex-direction: column;
  }
  #elevation-cut input[type=range] {
    writing-mode: vertical-lr; direction: rtl; height: 120px;
  }
  #lateral-cut {
    position: fixed; bottom: 12px; right: 12px;
  }
  #lateral-cut input[type=range] { width: 120px; }
  .panel {
    position: fixed; z-index: 30; overflow: auto;
    background: rgba(0,0,0,0.78); border: 1px solid rgba(255,255,255,0.15);
    border-radius: 6px; padding: 8px 10px;
    font-family: monospace; font-size: 11px; color: rgba(255,255,255,0.85);
  }
  .panel-title {
    display: flex; justify-content: space-between; align-items: center;
    gap: 8px; margin-bottom: 6px;
    font-weight: bold; color: rgba(137,180,250,0.95);
  }
  .panel-close { cursor: pointer; color: rgba(255,255,255,0.5); flex-shrink: 0; }
  .panel-close:hover { color: rgba(255,255,255,0.9); }
  #inspector { left: 8px; bottom: 26px; width: 300px; max-height: 52vh; }
  #filters { left: 8px; top: 30px; width: 240px; max-height: 60vh; }
  .insp-row { display: flex; gap: 8px; margin-bottom: 3px; }
  .insp-key { color: rgba(255,255,255,0.45); min-width: 96px; flex-shrink: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .insp-val {
    flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .insp-group {
    margin: 7px 0 3px; padding-top: 5px; font-weight: bold;
    color: rgba(166,227,161,0.85);
    border-top: 1px solid rgba(255,255,255,0.12);
  }
  .insp-swatch {
    display: inline-block; width: 9px; height: 9px; margin-right: 5px;
    border: 1px solid rgba(255,255,255,0.35); vertical-align: middle;
  }
  .insp-actions { display: flex; gap: 6px; margin-bottom: 7px; }
  .btn {
    background: rgba(255,255,255,0.08); color: rgba(255,255,255,0.75);
    border: 1px solid rgba(255,255,255,0.18); border-radius: 4px;
    padding: 3px 9px; cursor: pointer; font-family: monospace; font-size: 11px;
  }
  .btn:hover { background: rgba(137,180,250,0.25); color: #fff; }
  .filter-head {
    margin: 8px 0 4px; color: rgba(255,255,255,0.45);
    text-transform: uppercase; font-size: 10px; letter-spacing: 0.5px;
  }
  .filter-row {
    display: flex; align-items: center; gap: 6px; margin-bottom: 2px;
    cursor: pointer; user-select: none;
  }
  .filter-row input { margin: 0; cursor: pointer; flex-shrink: 0; }
  .filter-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .filter-count { color: rgba(255,255,255,0.35); flex-shrink: 0; }
  #filter-status { margin-top: 8px; color: rgba(249,226,175,0.8); }
</style>
</head>
<body>
<canvas id="canvas"></canvas>
<canvas id="canvas2d" class="hidden"></canvas>
<div id="status">Waiting for BuildingML source...</div>
<div id="controls">Drag: rotate | Scroll: zoom | Shift+drag: pan</div>
<div id="view-toggle">
  <div id="mode-group">
    <button id="btn-3d" class="active">3D</button>
    <button id="btn-2d">2D</button>
  </div>
  <button id="btn-filters" title="Show/hide elements by category or storey">Filter</button>
</div>
<div id="storey-label" class="hidden"></div>
<select id="storey-select" class="hidden" title="Storey"></select>
<div id="inspector" class="panel hidden">
  <div class="panel-title"><span id="inspector-name"></span><span id="inspector-close" class="panel-close">✕</span></div>
  <div class="insp-actions">
    <button id="btn-hide" class="btn" title="Hide this element">Hide</button>
    <button id="btn-isolate" class="btn" title="Hide everything else">Isolate</button>
  </div>
  <div id="inspector-body"></div>
</div>
<div id="filters" class="panel hidden">
  <div class="panel-title"><span>Visibility</span><span id="filters-close" class="panel-close">✕</span></div>
  <div class="insp-actions">
    <button id="btn-show-all" class="btn" title="Clear every filter">Show all</button>
  </div>
  <div id="filters-body"></div>
  <div id="filter-status"></div>
</div>
<div id="elevation-cut" class="cut-panel">
  <label><input type="checkbox" id="elev-toggle"> Plan</label>
  <input type="range" id="elev-slider" min="0" max="10" step="0.01" value="10" disabled>
  <span id="elev-label" class="cut-label">10.0 m</span>
</div>
<div id="lateral-cut" class="cut-panel">
  <label><input type="checkbox" id="lat-toggle"> Section</label>
  <input type="range" id="lat-slider" min="0" max="10" step="0.01" value="10" disabled>
  <span id="lat-label" class="cut-label">10.0 m</span>
</div>
<div id="error-overlay" style="display:none;position:absolute;top:8px;left:8px;right:8px;max-height:60%;overflow:auto;z-index:50;background:rgba(40,10,10,0.95);color:#ffb4b4;border:1px solid #a33;border-radius:4px;padding:8px 28px 8px 10px;font-family:monospace;font-size:12px;white-space:pre-wrap;"></div>
<div id="error-close" style="display:none;position:absolute;top:12px;right:14px;z-index:51;color:#ffb4b4;cursor:pointer;font-family:monospace;">✕</div>
<script src="${scriptUri}"></script>
<script>
const {
  WebGLRenderer, OrbitCameraController,
  drawAll, importStorey, getStoreys, detectSpaces,
  applyZoom, createDefaultCamera,
  cutFraction, applyCutFraction,
} = BuildingMLRenderer;
const vscode = acquireVsCodeApi();

// ============================================================
// 3D setup
// ============================================================
const canvas3d = document.getElementById('canvas');
const canvas2d = document.getElementById('canvas2d');
const statusEl = document.getElementById('status');
const controlsEl = document.getElementById('controls');
const storeyLabelEl = document.getElementById('storey-label');
const storeySelect = document.getElementById('storey-select');

const renderer = new WebGLRenderer(canvas3d);
const camController = new OrbitCameraController(canvas3d);
renderer.camera = camController.camera;
renderer.start();

// ============================================================
// View mode toggle: '3d' | '2d'
// ============================================================
let viewMode = '3d';

const saved = vscode.getState() || {};
if (saved.viewMode === '3d' || saved.viewMode === '2d') viewMode = saved.viewMode;

// Cuts are persisted as a fraction of the model's extent, never as metres: the
// saved state outlives the model it came from, and restoring "8 m" into a
// taller building is how a reopened preview ends up sliced through the middle.
function cutMemo(cut) {
  return { enabled: cut.enabled, fraction: cutFraction(cut) };
}

function saveState() {
  vscode.setState({
    viewMode,
    camera3d: { ...camController.camera, target: [...camController.camera.target] },
    sectionCut: cutMemo(renderer.sectionCut),
    lateralCut: cutMemo(renderer.lateralCut),
    camera2d: camera2d ? { ...camera2d } : null,
  });
}

// Restore 3D camera state
if (saved.camera3d) {
  Object.assign(camController.camera, saved.camera3d);
  camController.camera.target = [...saved.camera3d.target];
}

canvas3d.addEventListener('mouseup', saveState);
canvas3d.addEventListener('wheel', () => requestAnimationFrame(saveState), { passive: true });

// ============================================================
// Element inspector — click a mesh to see what it is made of
// ============================================================
const inspectorEl = document.getElementById('inspector');
const inspectorNameEl = document.getElementById('inspector-name');
const inspectorBodyEl = document.getElementById('inspector-body');

function hideInspector() {
  inspectorEl.classList.add('hidden');
  renderer.highlightedId = null;
  selectedId = null;
}
document.getElementById('inspector-close').addEventListener('click', hideInspector);

// Property-set rows arrive as "Pset_WallCommon · IsExternal"; the separator is
// three characters wide so it cannot appear inside an IFC property name.
const GROUP_SEP = ' · ';

function row(key, value, swatch) {
  const r = document.createElement('div');
  r.className = 'insp-row';
  const k = document.createElement('span');
  k.className = 'insp-key';
  k.textContent = key;
  k.title = key;
  const v = document.createElement('span');
  v.className = 'insp-val';
  v.title = value;
  if (swatch) {
    const s = document.createElement('span');
    s.className = 'insp-swatch';
    s.style.background = swatch;
    v.appendChild(s);
  }
  v.appendChild(document.createTextNode(value));
  r.appendChild(k);
  r.appendChild(v);
  return r;
}

function groupHeading(text) {
  const h = document.createElement('div');
  h.className = 'insp-group';
  h.textContent = text;
  return h;
}

let selectedId = null;

function showInspector(hit) {
  selectedId = hit.id;
  inspectorNameEl.textContent = hit.label || hit.elementType;
  inspectorNameEl.title = hit.label || '';
  inspectorBodyEl.textContent = '';
  // The picked part's own material is more specific than the element-level
  // list in properties, so it leads.
  if (hit.materialName) {
    const c = hit.color;
    const swatch = c
      ? 'rgb(' + Math.round(c[0]*255) + ',' + Math.round(c[1]*255) + ',' + Math.round(c[2]*255) + ')'
      : null;
    inspectorBodyEl.appendChild(row('Surface', hit.materialName, swatch));
  }
  let currentGroup = null;
  for (const [key, value] of (hit.properties || [])) {
    const at = key.indexOf(GROUP_SEP);
    if (at < 0) {
      inspectorBodyEl.appendChild(row(key, value));
      continue;
    }
    const group = key.slice(0, at);
    if (group !== currentGroup) {
      inspectorBodyEl.appendChild(groupHeading(group));
      currentGroup = group;
    }
    inspectorBodyEl.appendChild(row(key.slice(at + GROUP_SEP.length), value));
  }
  inspectorEl.classList.remove('hidden');
}

// Distinguish a click from an orbit drag.
let pressPos = null;
canvas3d.addEventListener('mousedown', (e) => { pressPos = [e.clientX, e.clientY]; });
canvas3d.addEventListener('mouseup', (e) => {
  if (!pressPos) { return; }
  const moved = Math.abs(e.clientX - pressPos[0]) + Math.abs(e.clientY - pressPos[1]);
  pressPos = null;
  if (moved > 4 || !sceneLoaded) { return; }
  const hit = renderer.pick(e.clientX, e.clientY);
  if (hit) {
    renderer.highlightedId = hit.id;
    showInspector(hit);
  } else {
    hideInspector();
  }
});

// ============================================================
// Visibility filters — by category, by storey, and per element
// ============================================================
const filtersEl = document.getElementById('filters');
const filtersBodyEl = document.getElementById('filters-body');
const filterStatusEl = document.getElementById('filter-status');

// Elements the user hid one at a time, kept apart from the category/storey
// checkboxes so toggling a category back on does not resurrect them.
let manuallyHidden = new Set();
let hiddenCategories = new Set();
let hiddenStoreys = new Set();
let elements = [];

function storeyOf(element) {
  for (const [key, value] of element.properties) {
    if (key === 'Storey') { return value; }
  }
  return '';
}

/** Recompute the hidden set from all three filters and push it to the renderer. */
function applyFilters() {
  const hidden = new Set(manuallyHidden);
  for (const el of elements) {
    if (hiddenCategories.has(el.elementType) || hiddenStoreys.has(storeyOf(el))) {
      hidden.add(el.id);
    }
  }
  renderer.hiddenIds = hidden;

  const n = hidden.size;
  filterStatusEl.textContent = n === 0
    ? ''
    : n + ' of ' + elements.length + ' elements hidden';
  // A hidden element cannot be picked, so its inspector entry is stale.
  if (selectedId !== null && hidden.has(selectedId)) { hideInspector(); }
}

function filterRow(name, count, checked, onToggle) {
  const label = document.createElement('label');
  label.className = 'filter-row';
  const box = document.createElement('input');
  box.type = 'checkbox';
  box.checked = checked;
  box.addEventListener('change', () => onToggle(box.checked));
  const text = document.createElement('span');
  text.className = 'filter-name';
  text.textContent = name;
  text.title = name;
  const num = document.createElement('span');
  num.className = 'filter-count';
  num.textContent = String(count);
  label.appendChild(box);
  label.appendChild(text);
  label.appendChild(num);
  return label;
}

function section(title) {
  const h = document.createElement('div');
  h.className = 'filter-head';
  h.textContent = title;
  return h;
}

/** Rebuild the category/storey checkbox lists from the loaded scene. */
function buildFilters() {
  elements = renderer.listElements ? renderer.listElements() : [];
  filtersBodyEl.textContent = '';

  const categories = new Map();
  const storeys = new Map();
  for (const el of elements) {
    categories.set(el.elementType, (categories.get(el.elementType) || 0) + 1);
    const s = storeyOf(el);
    if (s) { storeys.set(s, (storeys.get(s) || 0) + 1); }
  }

  if (categories.size > 0) {
    filtersBodyEl.appendChild(section('Category'));
    const names = [...categories.keys()].sort();
    for (const name of names) {
      filtersBodyEl.appendChild(filterRow(name, categories.get(name), !hiddenCategories.has(name), (on) => {
        if (on) { hiddenCategories.delete(name); } else { hiddenCategories.add(name); }
        applyFilters();
      }));
    }
  }

  // Storeys keep the model's own bottom-up order rather than being sorted.
  if (storeys.size > 1) {
    filtersBodyEl.appendChild(section('Storey'));
    for (const name of storeys.keys()) {
      filtersBodyEl.appendChild(filterRow(name, storeys.get(name), !hiddenStoreys.has(name), (on) => {
        if (on) { hiddenStoreys.delete(name); } else { hiddenStoreys.add(name); }
        applyFilters();
      }));
    }
  }

  if (elements.length === 0) {
    filtersBodyEl.appendChild(section('No elements loaded'));
  }
}

function resetFilters() {
  manuallyHidden = new Set();
  hiddenCategories = new Set();
  hiddenStoreys = new Set();
  buildFilters();
  applyFilters();
}

document.getElementById('btn-hide').addEventListener('click', () => {
  if (selectedId === null) { return; }
  manuallyHidden.add(selectedId);
  hideInspector();
  applyFilters();
});

document.getElementById('btn-isolate').addEventListener('click', () => {
  if (selectedId === null) { return; }
  const keep = selectedId;
  manuallyHidden = new Set(elements.filter(el => el.id !== keep).map(el => el.id));
  hiddenCategories = new Set();
  hiddenStoreys = new Set();
  buildFilters();
  applyFilters();
});

document.getElementById('btn-show-all').addEventListener('click', resetFilters);

const btnFilters = document.getElementById('btn-filters');
btnFilters.addEventListener('click', () => {
  const showing = filtersEl.classList.toggle('hidden');
  btnFilters.classList.toggle('active', !showing);
});
document.getElementById('filters-close').addEventListener('click', () => {
  filtersEl.classList.add('hidden');
  btnFilters.classList.remove('active');
});

const btn3d = document.getElementById('btn-3d');
const btn2d = document.getElementById('btn-2d');

// Cached scene data for view mode switching
let cachedBmlScene = null;  // BML scene graph (from buildingml scene)

function setViewMode(mode) {
  viewMode = mode;
  const is3d = mode === '3d';

  canvas3d.classList.toggle('hidden', !is3d);
  canvas2d.classList.toggle('hidden', is3d);

  btn3d.classList.toggle('active', mode === '3d');
  btn2d.classList.toggle('active', mode === '2d');

  // Show/hide 3D-only controls
  const show3dControls = is3d && sceneLoaded;
  document.getElementById('elevation-cut').style.display = show3dControls ? 'flex' : 'none';
  document.getElementById('lateral-cut').style.display = show3dControls ? 'flex' : 'none';

  // Show/hide 2D-only controls
  storeyLabelEl.classList.toggle('hidden', is3d);
  storeySelect.classList.toggle('hidden', is3d || storeySelect.options.length === 0);

  // Selection and visibility filters are 3D-only.
  btnFilters.classList.toggle('hidden', !is3d);
  if (!is3d) {
    hideInspector();
    filtersEl.classList.add('hidden');
    btnFilters.classList.remove('active');
  }

  controlsEl.textContent = is3d
    ? 'Drag: rotate | Scroll: zoom | Shift+drag: pan'
    : 'Scroll: zoom | Middle/Shift+drag: pan';

  if (mode === '2d') {
    resize2dCanvas();
    requestDraw2d();
  } else if (mode === '3d' && cachedBmlScene) {
    renderer.loadScene(cachedBmlScene);
  }

  saveState();
}

btn3d.addEventListener('click', () => setViewMode('3d'));
btn2d.addEventListener('click', () => setViewMode('2d'));

// ============================================================
// 3D UI elements
// ============================================================
let sceneLoaded = false;
const elevSlider = document.getElementById('elev-slider');
const elevLabel = document.getElementById('elev-label');
const elevToggle = document.getElementById('elev-toggle');
const latSlider = document.getElementById('lat-slider');
const latLabel = document.getElementById('lat-label');
const latToggle = document.getElementById('lat-toggle');

function syncElevUI() {
  elevSlider.value = renderer.sectionCut.value.toFixed(2);
  elevLabel.textContent = renderer.sectionCut.value.toFixed(1) + ' m';
  elevToggle.checked = renderer.sectionCut.enabled;
  // Cuts are off by default, so the slider has to read as inert rather than
  // look draggable while dragging it changes nothing.
  elevSlider.disabled = !renderer.sectionCut.enabled;
}
function syncLatUI() {
  latSlider.value = renderer.lateralCut.value.toFixed(2);
  latLabel.textContent = renderer.lateralCut.value.toFixed(1) + ' m';
  latToggle.checked = renderer.lateralCut.enabled;
  latSlider.disabled = !renderer.lateralCut.enabled;
}

elevSlider.addEventListener('input', () => {
  renderer.sectionCut.value = parseFloat(elevSlider.value);
  elevLabel.textContent = parseFloat(elevSlider.value).toFixed(1) + ' m';
  saveState();
});
elevToggle.addEventListener('change', () => {
  renderer.sectionCut.enabled = elevToggle.checked;
  elevSlider.disabled = !elevToggle.checked;
  saveState();
});
latSlider.addEventListener('input', () => {
  renderer.lateralCut.value = parseFloat(latSlider.value);
  latLabel.textContent = parseFloat(latSlider.value).toFixed(1) + ' m';
  saveState();
});
latToggle.addEventListener('change', () => {
  renderer.lateralCut.enabled = latToggle.checked;
  latSlider.disabled = !latToggle.checked;
  saveState();
});

// ============================================================
// 2D floorplan state
// ============================================================
let camera2d = saved.camera2d || createDefaultCamera();
let floorplanElements = [];
let floorplanSpaces = [];
let currentStoreyIndex = -1;
let currentStoreyName = '';
let modelData = null;

const drawState = {
  elements: [],
  detectedSpaces: [],
  selectedId: null,
  hoveredId: null,
  currentTool: 'select',
  showGrid: true,
  snapEnabled: true,
  wallStart: null,
  slabPoints: [],
  ghostPos: null,
  camera: camera2d,
  perpDragMode: false,
  perpDragPolygon: null,
};

function resize2dCanvas() {
  const dpr = window.devicePixelRatio || 1;
  canvas2d.width = window.innerWidth * dpr;
  canvas2d.height = window.innerHeight * dpr;
  canvas2d.style.width = window.innerWidth + 'px';
  canvas2d.style.height = window.innerHeight + 'px';
}

function requestDraw2d() {
  if (viewMode !== '2d') return;
  requestAnimationFrame(draw2d);
}

function draw2d() {
  if (viewMode !== '2d') return;
  const ctx = canvas2d.getContext('2d');
  if (!ctx) return;

  const dpr = window.devicePixelRatio || 1;
  const w = canvas2d.width;
  const h = canvas2d.height;

  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

  drawState.elements = floorplanElements;
  drawState.detectedSpaces = floorplanSpaces;
  drawState.camera = camera2d;

  drawAll(ctx, drawState, w / dpr, h / dpr);
}

/** Load a storey from the model and render 2D. */
// Index of the plan (getStoreys order) whose heading or spaces contain the
// cursor line; 0 if none
function planAtLine(model, line) {
  const within = (span) => span && line >= span.start.line && line <= span.end.line;
  const index = getStoreys(model).findIndex(
    (p) => within(p.span) || p.spaces.some((s) => within(s.span)));
  return Math.max(0, index);
}

function loadStorey(model, storeyIndex) {
  const result = importStorey(model, storeyIndex);
  if (!result) {
    floorplanElements = [];
    floorplanSpaces = [];
    currentStoreyName = '';
    storeyLabelEl.textContent = '';
    requestDraw2d();
    return;
  }

  floorplanElements = result.elements;
  floorplanSpaces = detectSpaces(result.elements, []);
  currentStoreyName = result.name;
  storeyLabelEl.textContent = result.name;

  requestDraw2d();
}

// Storey dropdown (used when there is no editor cursor to drive selection, e.g. .ifc)
function populateStoreys() {
  if (!modelData) { return; }
  const storeys = getStoreys(modelData);
  storeySelect.innerHTML = '';
  storeys.forEach((s, i) => {
    const opt = document.createElement('option');
    opt.value = String(i);
    opt.textContent = s.name || ('Storey ' + i);
    storeySelect.appendChild(opt);
  });
  if (currentStoreyIndex >= 0 && currentStoreyIndex < storeys.length) {
    storeySelect.value = String(currentStoreyIndex);
  }
  storeySelect.classList.toggle('hidden', viewMode !== '2d' || storeys.length === 0);
}
storeySelect.addEventListener('change', () => {
  currentStoreyIndex = parseInt(storeySelect.value, 10);
  loadStorey(modelData, currentStoreyIndex);
  fitCamera2d();
  requestDraw2d();
});

/** Auto-fit camera to show all content with padding. */
function fitCamera2d() {
  if (floorplanElements.length === 0) return;

  let minX = Infinity, maxX = -Infinity;
  let minY = Infinity, maxY = -Infinity;

  for (const el of floorplanElements) {
    if (el.type === 'Wall') {
      minX = Math.min(minX, el.from.x, el.to.x);
      maxX = Math.max(maxX, el.from.x, el.to.x);
      minY = Math.min(minY, el.from.y, el.to.y);
      maxY = Math.max(maxY, el.from.y, el.to.y);
    } else if (el.type === 'Column') {
      minX = Math.min(minX, el.x - el.profileW / 2);
      maxX = Math.max(maxX, el.x + el.profileW / 2);
      minY = Math.min(minY, el.y - el.profileD / 2);
      maxY = Math.max(maxY, el.y + el.profileD / 2);
    } else if (el.type === 'Slab') {
      for (const p of el.boundary) {
        minX = Math.min(minX, p.x);
        maxX = Math.max(maxX, p.x);
        minY = Math.min(minY, p.y);
        maxY = Math.max(maxY, p.y);
      }
    }
  }

  if (!isFinite(minX)) return;

  const dpr = window.devicePixelRatio || 1;
  const canvasW = canvas2d.width / dpr;
  const canvasH = canvas2d.height / dpr;
  const padding = 40; // pixels

  const spanX = maxX - minX || 1;
  const spanY = maxY - minY || 1;

  const zoomX = (canvasW - padding * 2) / spanX;
  const zoomY = (canvasH - padding * 2) / spanY;

  camera2d.zoom = Math.min(zoomX, zoomY);
  camera2d.zoom = Math.max(5, Math.min(500, camera2d.zoom));
  camera2d.viewX = (minX + maxX) / 2;
  camera2d.viewY = (minY + maxY) / 2;
}

// ============================================================
// 2D mouse interaction (read-only: pan & zoom)
// ============================================================
let isPanning2d = false;
let panStart2d = { x: 0, y: 0 };

canvas2d.addEventListener('mousedown', (e) => {
  // Middle button or shift+left for pan
  if (e.button === 1 || (e.button === 0 && e.shiftKey)) {
    isPanning2d = true;
    panStart2d = { x: e.clientX, y: e.clientY };
    e.preventDefault();
  }
});

canvas2d.addEventListener('mousemove', (e) => {
  if (isPanning2d) {
    const dx = e.clientX - panStart2d.x;
    const dy = e.clientY - panStart2d.y;
    camera2d.viewX -= dx / camera2d.zoom;
    camera2d.viewY += dy / camera2d.zoom; // flip Y
    panStart2d = { x: e.clientX, y: e.clientY };
    requestDraw2d();
  }
});

canvas2d.addEventListener('mouseup', () => {
  if (isPanning2d) {
    isPanning2d = false;
    saveState();
  }
});

canvas2d.addEventListener('wheel', (e) => {
  e.preventDefault();
  const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1;
  const dpr = window.devicePixelRatio || 1;
  applyZoom(camera2d, factor, e.clientX, e.clientY, canvas2d.width / dpr, canvas2d.height / dpr);
  requestDraw2d();
  saveState();
}, { passive: false });

// Prevent context menu on canvas
canvas2d.addEventListener('contextmenu', (e) => e.preventDefault());

// ============================================================
// Window resize
// ============================================================
window.addEventListener('resize', () => {
  if (viewMode === '2d') {
    resize2dCanvas();
    requestDraw2d();
  }
});

// ============================================================
// Message handling
// ============================================================
let firstLoad = true;
let lastSource = null;
// Nodes of a large scene arrive in 'sceneChunk' messages ahead of its 'updateScene'.
let pendingNodes = [];
function logToHost(text) { vscode.postMessage({ type: 'log', text }); }
window.addEventListener('error', e => logToHost('error: ' + e.message));
window.addEventListener('unhandledrejection', e => logToHost('error: ' + (e.reason && e.reason.message || e.reason)));
window.addEventListener('message', event => {
  const msg = event.data;
  const tMsg = performance.now();

  if (msg.type === 'sceneChunk') {
    for (const node of msg.nodes) pendingNodes.push(node);

  } else if (msg.type === 'updateScene') {
    if (pendingNodes.length) {
      msg.scene.nodes = pendingNodes.concat(msg.scene.nodes);
      pendingNodes = [];
    }
    hideError();
    sceneLoaded = true;
    cachedBmlScene = msg.scene;

    // Remember where each cut sat, as a fraction, before loadScene recomputes
    // the bounds. A fresh open with no saved view means no clipping at all:
    // fraction 1 with the toggle off, so the whole model is visible however
    // the next model is proportioned. Pre-fraction saved states (which carried
    // metres) read as fraction 1 here, which is the safe direction to fail.
    function restoreMemo(memo) {
      return {
        enabled: !!(memo && memo.enabled),
        fraction: memo && typeof memo.fraction === 'number' ? memo.fraction : 1,
      };
    }
    const prevSectionCut = firstLoad
      ? restoreMemo(saved && saved.sectionCut)
      : cutMemo(renderer.sectionCut);
    const prevLateralCut = firstLoad
      ? restoreMemo(saved && saved.lateralCut)
      : cutMemo(renderer.lateralCut);
    firstLoad = false;

    renderer.loadScene(msg.scene);
    // Mesh indices are rebuilt, so a stale selection no longer identifies
    // anything meaningful.
    hideInspector();
    // A re-render of the same document keeps the filters; ids from another file
    // could name unrelated elements, so a new document starts unfiltered.
    if (msg.source !== lastSource) {
      lastSource = msg.source;
      resetFilters();
    } else {
      buildFilters();
      applyFilters();
    }

    // Re-place each cut at the same fraction of the new scene's extent.
    renderer.sectionCut.enabled = prevSectionCut.enabled;
    applyCutFraction(renderer.sectionCut, prevSectionCut.fraction);
    renderer.lateralCut.enabled = prevLateralCut.enabled;
    applyCutFraction(renderer.lateralCut, prevLateralCut.fraction);

    const sc = renderer.sectionCut;
    elevSlider.min = sc.min.toFixed(2);
    elevSlider.max = sc.max.toFixed(2);
    elevSlider.step = sc.step.toFixed(4);
    syncElevUI();
    if (viewMode === '3d') document.getElementById('elevation-cut').style.display = 'flex';

    const lc = renderer.lateralCut;
    latSlider.min = lc.min.toFixed(2);
    latSlider.max = lc.max.toFixed(2);
    latSlider.step = lc.step.toFixed(4);
    syncLatUI();
    if (viewMode === '3d') document.getElementById('lateral-cut').style.display = 'flex';

    saveState();
    statusEl.textContent = msg.scene.nodes.length + ' elements';
    logToHost(msg.scene.nodes.length + ' elements loaded in ' + Math.round(performance.now() - tMsg) + ' ms');

  } else if (msg.type === 'updateModel') {
    modelData = msg.model;
    const newStoreyIndex = planAtLine(modelData, msg.cursorLine);

    // If storey changed, reload and fit camera
    if (newStoreyIndex !== currentStoreyIndex) {
      currentStoreyIndex = newStoreyIndex;
      loadStorey(modelData, currentStoreyIndex);
      fitCamera2d();
      requestDraw2d();
    } else {
      loadStorey(modelData, currentStoreyIndex);
      requestDraw2d();
    }
    populateStoreys();

  } else if (msg.type === 'updateCursorStorey') {
    const newStoreyIndex = modelData ? planAtLine(modelData, msg.cursorLine) : currentStoreyIndex;
    if (newStoreyIndex !== currentStoreyIndex) {
      currentStoreyIndex = newStoreyIndex;
      loadStorey(modelData, currentStoreyIndex);
      fitCamera2d();
      requestDraw2d();
    }

  } else if (msg.type === 'error') {
    pendingNodes = [];
    showError(msg.message);
  }
});

const errorOverlay = document.getElementById('error-overlay');
const errorClose = document.getElementById('error-close');
function showError(message) {
  errorOverlay.textContent = message;
  errorOverlay.style.display = 'block';
  errorClose.style.display = 'block';
  statusEl.textContent = 'Error';
}
function hideError() {
  errorOverlay.style.display = 'none';
  errorClose.style.display = 'none';
}
errorClose.addEventListener('click', hideError);

// Apply initial view mode from saved state
setViewMode(viewMode);
statusEl.textContent = 'Ready';
</script>
</body>
</html>`;
  }
}
