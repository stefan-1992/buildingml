import { mat4Perspective, mat4LookAt, mat4Identity } from './math.ts';
import { createShaderProgram, type ShaderLocations } from './shaders.ts';
import { getCameraPosition, type CameraState, DEFAULT_CAMERA } from './orbit-camera.ts';
import { loadSceneToGpu, fitCameraToScene, type GpuMesh } from './scene-loader.ts';
import { computeSectionCutBounds, computeLateralCutBounds, type SectionCutState, DEFAULT_SECTION_CUT } from './section-cut.ts';
import type { ElementSummary, PickResult, TransformCallback } from './renderer-interface.ts';
import type { SceneGraph } from '../types.ts';

export interface RendererState {
  camera: CameraState;
  wireframe: boolean;
  sectionCut: SectionCutState;
  lateralCut: SectionCutState;
}

/** Highlight color for selection (cyan-blue), matching Three.js renderer. */
const HIGHLIGHT_COLOR = [0.35, 0.7, 1.0];

// Pick shader: renders each mesh with a flat color encoding its index.
const PICK_VS = `
  attribute vec3 aPosition;
  uniform mat4 uProjection;
  uniform mat4 uView;
  void main() {
    gl_Position = uProjection * uView * vec4(aPosition, 1.0);
  }
`;
const PICK_FS = `
  precision mediump float;
  uniform vec4 uPickColor;
  void main() {
    gl_FragColor = uPickColor;
  }
`;

interface PickShader {
  program: WebGLProgram;
  aPosition: number;
  uProjection: WebGLUniformLocation | null;
  uView: WebGLUniformLocation | null;
  uPickColor: WebGLUniformLocation | null;
}

function createPickShader(gl: WebGLRenderingContext): PickShader | null {
  const vs = gl.createShader(gl.VERTEX_SHADER);
  const fs = gl.createShader(gl.FRAGMENT_SHADER);
  if (!vs || !fs) return null;
  gl.shaderSource(vs, PICK_VS);
  gl.compileShader(vs);
  gl.shaderSource(fs, PICK_FS);
  gl.compileShader(fs);
  if (!gl.getShaderParameter(vs, gl.COMPILE_STATUS) || !gl.getShaderParameter(fs, gl.COMPILE_STATUS)) return null;
  const program = gl.createProgram();
  if (!program) return null;
  gl.attachShader(program, vs);
  gl.attachShader(program, fs);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) return null;
  return {
    program,
    aPosition: gl.getAttribLocation(program, 'aPosition'),
    uProjection: gl.getUniformLocation(program, 'uProjection'),
    uView: gl.getUniformLocation(program, 'uView'),
    uPickColor: gl.getUniformLocation(program, 'uPickColor'),
  };
}

/** Encode a mesh index as RGBA color (index 0..16M). */
function indexToColor(index: number): [number, number, number, number] {
  return [
    ((index + 1) & 0xFF) / 255,
    (((index + 1) >> 8) & 0xFF) / 255,
    (((index + 1) >> 16) & 0xFF) / 255,
    1.0,
  ];
}

/** Decode an RGBA pixel back to mesh index. Returns -1 for background. */
function colorToIndex(r: number, g: number, b: number): number {
  const val = r + (g << 8) + (b << 16);
  return val === 0 ? -1 : val - 1;
}

/**
 * Framework-agnostic WebGL renderer.
 * Receives a canvas and manages the GL context, shader program, and render loop.
 * Supports color-buffer picking for element selection and highlight.
 */
export class WebGLRenderer {
  private gl: WebGLRenderingContext;
  private canvas: HTMLCanvasElement;
  private program: WebGLProgram;
  private locs: ShaderLocations;
  private meshes: GpuMesh[] = [];
  private animFrameId: number | null = null;
  private pickShader: PickShader | null = null;
  private pickFBO: WebGLFramebuffer | null = null;
  private pickTexture: WebGLTexture | null = null;
  private pickRBO: WebGLRenderbuffer | null = null;
  private pickWidth = 0;
  private pickHeight = 0;

  camera: CameraState;
  wireframe = false;
  sectionCut: SectionCutState;
  lateralCut: SectionCutState;
  highlightedId: string | null = null;
  onTransform: TransformCallback | null = null;
  isDraggingGizmo = false;

  private _hiddenIds: Set<string> = new Set();

  get hiddenIds(): Set<string> {
    return this._hiddenIds;
  }

  /** Applying visibility once here keeps the draw and pick loops branch-free. */
  set hiddenIds(ids: Set<string>) {
    this._hiddenIds = ids;
    for (const mesh of this.meshes) {
      mesh.visible = !ids.has(mesh.id);
    }
  }

  constructor(canvas: HTMLCanvasElement) {
    this.canvas = canvas;
    const gl = canvas.getContext('webgl', { antialias: true, alpha: false, preserveDrawingBuffer: true });
    if (!gl) throw new Error('WebGL not available');
    this.gl = gl;

    // Enable 32-bit index support (required for IFC geometry with >65535 vertices)
    gl.getExtension('OES_element_index_uint');

    const result = createShaderProgram(gl);
    if (!result) throw new Error('Failed to create shader program');
    this.program = result.program;
    this.locs = result.locs;

    this.pickShader = createPickShader(gl);

    this.camera = { ...DEFAULT_CAMERA, target: [...DEFAULT_CAMERA.target] };
    this.sectionCut = { ...DEFAULT_SECTION_CUT };
    this.lateralCut = { ...DEFAULT_SECTION_CUT };
  }

  /** Load a scene graph into GPU buffers and auto-fit camera. */
  loadScene(sceneData: SceneGraph): void {
    this.meshes = loadSceneToGpu(this.gl, sceneData, this.meshes);
    fitCameraToScene(sceneData, this.camera);
    this.sectionCut = computeSectionCutBounds(sceneData);
    this.lateralCut = computeLateralCutBounds(sceneData);
    this.highlightedId = null;
    // Re-apply through the setter so a filter set before the reload survives it.
    this.hiddenIds = this._hiddenIds;
  }

  /** One entry per element; sub-meshes sharing an id collapse into the first. */
  listElements(): ElementSummary[] {
    const seen = new Set<string>();
    const out: ElementSummary[] = [];
    for (const mesh of this.meshes) {
      if (seen.has(mesh.id)) continue;
      seen.add(mesh.id);
      out.push({
        id: mesh.id,
        elementType: mesh.elementType,
        label: mesh.label,
        properties: mesh.properties,
      });
    }
    return out;
  }

  /** Pick the element at canvas coordinates. Returns null if nothing hit. */
  pick(canvasX: number, canvasY: number): PickResult | null {
    const gl = this.gl;
    if (!this.pickShader) return null;

    const rect = this.canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const maxSize = gl.getParameter(gl.MAX_RENDERBUFFER_SIZE) as number;
    const w = Math.min(Math.floor(rect.width * dpr), maxSize);
    const h = Math.min(Math.floor(rect.height * dpr), maxSize);
    if (w <= 0 || h <= 0) return null;
    const scaleX = w / (rect.width * dpr);
    const scaleY = h / (rect.height * dpr);
    const px = Math.floor((canvasX - rect.left) * dpr * scaleX);
    const py = h - Math.floor((canvasY - rect.top) * dpr * scaleY); // flip Y

    // Ensure FBO is sized correctly
    this.ensurePickFBO(w, h);
    if (!this.pickFBO) return null;

    // Render pick pass to FBO
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.pickFBO);
    gl.viewport(0, 0, w, h);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.enable(gl.DEPTH_TEST);
    gl.disable(gl.BLEND);

    const ps = this.pickShader;
    gl.useProgram(ps.program);

    const aspect = w / h;
    const projection = mat4Perspective(Math.PI / 4, aspect, 0.1, 500);
    const eye = getCameraPosition(this.camera);
    const view = mat4LookAt(eye, this.camera.target, [0, 0, 1]);
    gl.uniformMatrix4fv(ps.uProjection, false, projection);
    gl.uniformMatrix4fv(ps.uView, false, view);

    for (let i = 0; i < this.meshes.length; i++) {
      const mesh = this.meshes[i];
      // Hidden meshes are not drawn, so they must not be selectable either.
      if (!mesh.visible) continue;
      const color = indexToColor(i);
      gl.uniform4fv(ps.uPickColor, color);

      gl.bindBuffer(gl.ARRAY_BUFFER, mesh.vbo);
      gl.enableVertexAttribArray(ps.aPosition);
      gl.vertexAttribPointer(ps.aPosition, 3, gl.FLOAT, false, 24, 0);

      gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, mesh.ibo);
      gl.drawElements(gl.TRIANGLES, mesh.indexCount, gl.UNSIGNED_INT, 0);
    }

    // Read pixel
    const pixel = new Uint8Array(4);
    gl.readPixels(px, py, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);

    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.enable(gl.BLEND);

    const idx = colorToIndex(pixel[0], pixel[1], pixel[2]);
    if (idx < 0 || idx >= this.meshes.length) return null;

    const mesh = this.meshes[idx];
    return {
      id: mesh.id,
      elementType: mesh.elementType,
      label: mesh.label,
      properties: mesh.properties,
      materialName: mesh.materialName,
      color: [mesh.color[0], mesh.color[1], mesh.color[2], mesh.color[3]],
    };
  }

  /** Attach translate gizmo — not supported in WebGL renderer (Three.js only). */
  attachGizmo(_elementId: string | null): void {
    // No-op: gizmos are only available in Three.js renderer
  }

  /** Start the render loop. */
  start(): void {
    if (this.animFrameId !== null) return;
    const loop = () => {
      this.render();
      this.animFrameId = requestAnimationFrame(loop);
    };
    this.animFrameId = requestAnimationFrame(loop);
  }

  /** Stop the render loop. */
  stop(): void {
    if (this.animFrameId !== null) {
      cancelAnimationFrame(this.animFrameId);
      this.animFrameId = null;
    }
  }

  /** Clean up all GL resources (but keep the context alive for potential reuse). */
  dispose(): void {
    this.stop();
    for (const mesh of this.meshes) {
      this.gl.deleteBuffer(mesh.vbo);
      this.gl.deleteBuffer(mesh.ibo);
    }
    this.gl.deleteProgram(this.program);
    if (this.pickShader) this.gl.deleteProgram(this.pickShader.program);
    this.cleanupPickFBO();
    this.meshes = [];
    // Note: do NOT call WEBGL_lose_context here — it permanently kills the
    // GL context on this canvas, which breaks React StrictMode (dev) and any
    // scenario where the canvas element is reused after remount.
  }

  private ensurePickFBO(w: number, h: number): void {
    // Clamp to GPU max renderbuffer size to avoid GL_INVALID_VALUE
    const maxSize = this.gl.getParameter(this.gl.MAX_RENDERBUFFER_SIZE) as number;
    w = Math.min(w, maxSize);
    h = Math.min(h, maxSize);
    if (w <= 0 || h <= 0) return;

    if (this.pickFBO && this.pickWidth === w && this.pickHeight === h) return;
    this.cleanupPickFBO();

    const gl = this.gl;
    const fbo = gl.createFramebuffer();
    const tex = gl.createTexture();
    const rbo = gl.createRenderbuffer();
    if (!fbo || !tex || !rbo) return;

    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);

    gl.bindRenderbuffer(gl.RENDERBUFFER, rbo);
    gl.renderbufferStorage(gl.RENDERBUFFER, gl.DEPTH_COMPONENT16, w, h);

    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
    gl.framebufferRenderbuffer(gl.FRAMEBUFFER, gl.DEPTH_ATTACHMENT, gl.RENDERBUFFER, rbo);

    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.bindTexture(gl.TEXTURE_2D, null);
    gl.bindRenderbuffer(gl.RENDERBUFFER, null);

    this.pickFBO = fbo;
    this.pickTexture = tex;
    this.pickRBO = rbo;
    this.pickWidth = w;
    this.pickHeight = h;
  }

  private cleanupPickFBO(): void {
    const gl = this.gl;
    if (this.pickFBO) gl.deleteFramebuffer(this.pickFBO);
    if (this.pickTexture) gl.deleteTexture(this.pickTexture);
    if (this.pickRBO) gl.deleteRenderbuffer(this.pickRBO);
    this.pickFBO = null;
    this.pickTexture = null;
    this.pickRBO = null;
    this.pickWidth = 0;
    this.pickHeight = 0;
  }

  private render(): void {
    const { gl, canvas, program, locs } = this;

    // Resize to container (only when dimensions actually change)
    const rect = canvas.parentElement?.getBoundingClientRect();
    if (rect) {
      const dpr = window.devicePixelRatio || 1;
      const w = Math.floor(rect.width * dpr);
      const h = Math.floor(rect.height * dpr);
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
        canvas.style.width = rect.width + 'px';
        canvas.style.height = rect.height + 'px';
      }
    }
    gl.viewport(0, 0, canvas.width, canvas.height);

    gl.clearColor(0.08, 0.08, 0.12, 1.0);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.enable(gl.DEPTH_TEST);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);

    gl.useProgram(program);

    const aspect = canvas.width / canvas.height;
    const projection = mat4Perspective(Math.PI / 4, aspect, 0.1, 500);
    const eye = getCameraPosition(this.camera);
    const view = mat4LookAt(eye, this.camera.target, [0, 0, 1]);
    const model = mat4Identity();

    gl.uniformMatrix4fv(locs.uProjection, false, projection);
    gl.uniformMatrix4fv(locs.uView, false, view);
    gl.uniformMatrix4fv(locs.uModel, false, model);
    gl.uniform3fv(locs.uLightDir, [0.5, 0.3, 0.8]);
    gl.uniform3fv(locs.uCameraPos, eye);
    gl.uniform1i(locs.uClipEnabled, this.sectionCut.enabled ? 1 : 0);
    gl.uniform1f(locs.uClipZ, this.sectionCut.value);
    gl.uniform1i(locs.uClipYEnabled, this.lateralCut.enabled ? 1 : 0);
    gl.uniform1f(locs.uClipY, this.lateralCut.value);
    gl.uniform3fv(locs.uHighlightColor, HIGHLIGHT_COLOR);

    // Draw opaque first, then transparent (two-pass, zero allocations)
    for (let pass = 0; pass < 2; pass++) {
      for (const mesh of this.meshes) {
        if (!mesh.visible) continue;
        const isTransparent = mesh.color[3] < 1.0;
        if (pass === 0 && isTransparent) continue;
        if (pass === 1 && !isTransparent) continue;

        if (isTransparent) gl.depthMask(false);

        gl.uniform4fv(locs.uColor, mesh.color);
        gl.uniform1f(locs.uSpecular, mesh.specular);
        gl.uniform1f(locs.uShininess, mesh.shininess);

        // Set highlight uniform per mesh
        const isHighlighted = this.highlightedId !== null && mesh.id === this.highlightedId;
        gl.uniform1f(locs.uHighlight, isHighlighted ? 1.0 : 0.0);

        gl.bindBuffer(gl.ARRAY_BUFFER, mesh.vbo);
        gl.enableVertexAttribArray(locs.aPosition);
        gl.vertexAttribPointer(locs.aPosition, 3, gl.FLOAT, false, 24, 0);
        gl.enableVertexAttribArray(locs.aNormal);
        gl.vertexAttribPointer(locs.aNormal, 3, gl.FLOAT, false, 24, 12);

        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, mesh.ibo);

        if (this.wireframe) {
          for (let i = 0; i < mesh.indexCount; i += 3) {
            gl.drawElements(gl.LINE_LOOP, 3, gl.UNSIGNED_INT, i * 4);
          }
        } else {
          gl.drawElements(gl.TRIANGLES, mesh.indexCount, gl.UNSIGNED_INT, 0);
        }

        if (isTransparent) gl.depthMask(true);
      }
    }
  }
}
