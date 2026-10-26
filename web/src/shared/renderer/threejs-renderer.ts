/**
 * Three.js-based renderer for BuildingML scene graphs.
 * Drop-in alternative to WebGLRenderer — same public API surface.
 * Supports raycasting-based element picking and selection highlight.
 */
import * as THREE from 'three';
import { TransformControls } from 'three/addons/controls/TransformControls.js';
import type { CameraState } from './orbit-camera.ts';
import type { SectionCutState } from './section-cut.ts';
import type { ElementSummary, PickResult, TransformCallback } from './renderer-interface.ts';
import { DEFAULT_CAMERA } from './orbit-camera.ts';
import { DEFAULT_SECTION_CUT, computeSectionCutBounds, computeLateralCutBounds } from './section-cut.ts';
import { fitCameraToScene, meshArrays } from './scene-loader.ts';
import { FRAGMENT_SHADER } from './shaders.ts';
import type { SceneGraph, SceneNode } from '../types.ts';

/** Highlight color for selection outline (cyan-blue). */
const HIGHLIGHT_COLOR = new THREE.Color(0.35, 0.7, 1.0);

/**
 * Custom shader material that supports section cut clipping,
 * matching the lighting model of the existing WebGL renderer.
 * Adds a uHighlight uniform for selection glow.
 */
function createBmlMaterial(
  color: number[],
  specular = 0.0,
  shininess = 16.0,
): THREE.ShaderMaterial {
  return new THREE.ShaderMaterial({
    uniforms: {
      uColor: { value: new THREE.Vector4(color[0], color[1], color[2], color[3]) },
      uLightDir: { value: new THREE.Vector3(0.5, 0.3, 0.8).normalize() },
      uCameraPos: { value: new THREE.Vector3() },
      uClipEnabled: { value: false },
      uClipZ: { value: 10.0 },
      uClipYEnabled: { value: false },
      uClipY: { value: 10.0 },
      uHighlight: { value: 0.0 },
      uHighlightColor: { value: HIGHLIGHT_COLOR },
      uSpecular: { value: specular },
      uShininess: { value: shininess },
    },
    vertexShader: `
      varying vec3 vNormal;
      varying vec3 vPosition;
      void main() {
        vec4 worldPos = modelMatrix * vec4(position, 1.0);
        vPosition = worldPos.xyz;
        vNormal = normalMatrix * normal;
        gl_Position = projectionMatrix * viewMatrix * worldPos;
      }
    `,
    // Same lighting as the WebGL renderer; only the vertex stage differs.
    fragmentShader: FRAGMENT_SHADER,
    transparent: color[3] < 1.0,
    side: THREE.DoubleSide,
    depthWrite: color[3] >= 1.0,
  });
}

/**
 * Three.js renderer — drop-in replacement for WebGLRenderer.
 * Supports element selection via raycasting.
 */
export class ThreeJSRenderer {
  private renderer: THREE.WebGLRenderer;
  private threeScene: THREE.Scene;
  private threeCamera: THREE.PerspectiveCamera;
  private canvas: HTMLCanvasElement;
  private meshGroup: THREE.Group;
  private materials: THREE.ShaderMaterial[] = [];
  private animFrameId: number | null = null;
  private raycaster = new THREE.Raycaster();
  private gizmo: TransformControls | null = null;
  private gizmoTarget: THREE.Mesh | null = null;
  private gizmoStartPos = new THREE.Vector3();

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

  /**
   * `Object3D.visible` removes a mesh from rendering, but three's Raycaster
   * only tests layers — it happily hits invisible geometry — so `pick()` has to
   * filter as well.
   */
  set hiddenIds(ids: Set<string>) {
    this._hiddenIds = ids;
    for (const child of this.meshGroup.children) {
      child.visible = !ids.has(child.name);
    }
  }

  /** One entry per element; sub-meshes sharing an id collapse into the first. */
  listElements(): ElementSummary[] {
    const seen = new Set<string>();
    const out: ElementSummary[] = [];
    for (const child of this.meshGroup.children) {
      if (seen.has(child.name) || !child.userData?.elementType) continue;
      seen.add(child.name);
      out.push({
        id: child.name,
        elementType: child.userData.elementType,
        label: child.userData.label,
        properties: child.userData.properties ?? [],
      });
    }
    return out;
  }

  constructor(canvas: HTMLCanvasElement) {
    this.canvas = canvas;

    this.renderer = new THREE.WebGLRenderer({
      canvas,
      antialias: true,
      alpha: false,
    });
    this.renderer.setClearColor(new THREE.Color(0.08, 0.08, 0.12));
    this.renderer.setPixelRatio(globalThis.devicePixelRatio || 1);
    // Enable sorting for transparency
    this.renderer.sortObjects = true;

    this.threeScene = new THREE.Scene();
    this.threeCamera = new THREE.PerspectiveCamera(45, 1, 0.1, 500);
    this.meshGroup = new THREE.Group();
    this.threeScene.add(this.meshGroup);

    this.camera = { ...DEFAULT_CAMERA, target: [...DEFAULT_CAMERA.target] };
    this.sectionCut = { ...DEFAULT_SECTION_CUT };
    this.lateralCut = { ...DEFAULT_SECTION_CUT };
  }

  /** Load a scene graph and auto-fit camera. */
  loadScene(sceneData: SceneGraph): void {
    // Clear existing meshes and materials
    this.meshGroup.clear();
    for (const mat of this.materials) {
      mat.dispose();
    }
    this.materials = [];
    this.highlightedId = null;

    // Build Three.js meshes from scene graph
    const loadNode = (node: SceneNode, inherited: [string, string][]): void => {
      const { positions, normals, indices } = meshArrays(node.mesh);

      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
      geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
      geometry.setIndex(new THREE.BufferAttribute(indices, 1));
      geometry.computeBoundingSphere();

      const sceneMaterial =
        node.material !== undefined ? sceneData.materials?.[node.material] : undefined;
      const material = createBmlMaterial(
        node.mesh.color,
        sceneMaterial?.specular,
        sceneMaterial?.shininess,
      );
      this.materials.push(material);

      const mesh = new THREE.Mesh(geometry, material);
      mesh.name = node.id;
      mesh.userData = {
        elementType: node.element_type,
        label: node.label,
        // Sub-meshes of one element carry no properties of their own.
        properties: node.properties?.length ? node.properties : inherited,
        originalColor: [...node.mesh.color],
        materialName: sceneMaterial?.name ?? '',
      };
      // Transparent objects render after opaque
      mesh.renderOrder = node.mesh.color[3] < 1.0 ? 1 : 0;
      this.meshGroup.add(mesh);

      if (node.children) {
        for (const child of node.children) {
          loadNode(child, mesh.userData.properties);
        }
      }
    };

    for (const node of sceneData.nodes) {
      loadNode(node, []);
    }

    // Fit camera to scene (reuses existing logic)
    fitCameraToScene(sceneData, this.camera);
    this.sectionCut = computeSectionCutBounds(sceneData);
    this.lateralCut = computeLateralCutBounds(sceneData);
    // Re-apply through the setter so a filter set before the reload survives it.
    this.hiddenIds = this._hiddenIds;
  }

  /** Pick the element at canvas coordinates. Returns null if nothing hit. */
  pick(canvasX: number, canvasY: number): PickResult | null {
    const rect = this.canvas.getBoundingClientRect();
    const ndc = new THREE.Vector2(
      ((canvasX - rect.left) / rect.width) * 2 - 1,
      -((canvasY - rect.top) / rect.height) * 2 + 1,
    );

    // Sync camera for accurate raycast
    this.syncCamera();

    this.raycaster.setFromCamera(ndc, this.threeCamera);
    const intersects = this.raycaster.intersectObjects(this.meshGroup.children, false);

    for (const hit of intersects) {
      const obj = hit.object;
      // The raycaster reports hidden geometry, so skip it here — you should not
      // be able to select something you cannot see.
      if (obj.visible && obj.userData?.elementType) {
        return {
          id: obj.name,
          elementType: obj.userData.elementType,
          label: obj.userData.label,
          properties: obj.userData.properties ?? [],
          materialName: obj.userData.materialName ?? '',
          color: obj.userData.originalColor,
        };
      }
    }
    return null;
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

  /** Attach translate gizmo to element. Pass null to detach. */
  attachGizmo(elementId: string | null): void {
    // Detach existing gizmo
    if (this.gizmo) {
      this.threeScene.remove(this.gizmo.getHelper());
      this.gizmo.dispose();
      this.gizmo = null;
      this.gizmoTarget = null;
    }

    if (!elementId) return;

    // Find the mesh
    const mesh = this.meshGroup.children.find(
      (c): c is THREE.Mesh => c instanceof THREE.Mesh && c.name === elementId,
    );
    if (!mesh) return;

    // Sync camera before creating gizmo
    this.syncCamera();

    const gizmo = new TransformControls(this.threeCamera, this.canvas);
    gizmo.setMode('translate');
    gizmo.setSpace('world');
    gizmo.setSize(0.8);

    // Track drag state to suppress orbit camera
    gizmo.addEventListener('dragging-changed', (event: { value: unknown }) => {
      this.isDraggingGizmo = !!event.value;
      if (event.value) {
        // Store initial position
        this.gizmoStartPos.copy(mesh.position);
      } else {
        // Emit delta on drag end
        if (this.onTransform && this.gizmoTarget) {
          const delta: [number, number, number] = [
            mesh.position.x - this.gizmoStartPos.x,
            mesh.position.y - this.gizmoStartPos.y,
            mesh.position.z - this.gizmoStartPos.z,
          ];
          if (Math.abs(delta[0]) > 0.001 || Math.abs(delta[1]) > 0.001 || Math.abs(delta[2]) > 0.001) {
            this.onTransform(this.gizmoTarget.name, delta);
          }
        }
      }
    });

    gizmo.attach(mesh);
    this.threeScene.add(gizmo.getHelper());
    this.gizmo = gizmo;
    this.gizmoTarget = mesh;
  }

  /** Clean up all resources and release the context. */
  dispose(): void {
    this.stop();
    if (this.gizmo) {
      this.threeScene.remove(this.gizmo.getHelper());
      this.gizmo.dispose();
      this.gizmo = null;
    }
    this.meshGroup.clear();
    for (const mat of this.materials) {
      mat.dispose();
    }
    this.materials = [];
    this.renderer.dispose();
    // Note: do NOT call forceContextLoss — it permanently kills the GL context
    // on this canvas, breaking React StrictMode remount.
  }

  /** Sync orbit camera to Three.js camera position. */
  private syncCamera(): void {
    const cam = this.camera;
    const ex = cam.target[0] + cam.distance * Math.cos(cam.rotX) * Math.sin(cam.rotY);
    const ey = cam.target[1] + cam.distance * Math.cos(cam.rotX) * Math.cos(cam.rotY);
    const ez = cam.target[2] + cam.distance * Math.sin(cam.rotX);
    this.threeCamera.position.set(ex, ey, ez);
    this.threeCamera.up.set(0, 0, 1);
    this.threeCamera.lookAt(cam.target[0], cam.target[1], cam.target[2]);
    this.threeCamera.updateMatrixWorld();
  }

  private render(): void {
    const { canvas, renderer: r } = this;

    // Resize to container
    const rect = canvas.parentElement?.getBoundingClientRect();
    if (rect) {
      const dpr = globalThis.devicePixelRatio || 1;
      const w = rect.width;
      const h = rect.height;
      canvas.style.width = w + 'px';
      canvas.style.height = h + 'px';
      r.setSize(w, h, false);
      r.setPixelRatio(dpr);
      this.threeCamera.aspect = w / h;
      this.threeCamera.updateProjectionMatrix();
    }

    this.syncCamera();

    // Update all material uniforms (highlight, clipping, wireframe)
    for (const child of this.meshGroup.children) {
      if (!(child instanceof THREE.Mesh)) continue;
      const mat = child.material as THREE.ShaderMaterial;
      const cam = this.camera;
      const ex = cam.target[0] + cam.distance * Math.cos(cam.rotX) * Math.sin(cam.rotY);
      const ey = cam.target[1] + cam.distance * Math.cos(cam.rotX) * Math.cos(cam.rotY);
      const ez = cam.target[2] + cam.distance * Math.sin(cam.rotX);
      mat.uniforms.uCameraPos.value.set(ex, ey, ez);
      mat.uniforms.uClipEnabled.value = this.sectionCut.enabled;
      mat.uniforms.uClipZ.value = this.sectionCut.value;
      mat.uniforms.uClipYEnabled.value = this.lateralCut.enabled;
      mat.uniforms.uClipY.value = this.lateralCut.value;
      mat.wireframe = this.wireframe;

      // Highlight selected element
      const isHighlighted = this.highlightedId !== null && child.name === this.highlightedId;
      mat.uniforms.uHighlight.value = isHighlighted ? 1.0 : 0.0;
    }

    r.render(this.threeScene, this.threeCamera);
  }
}
