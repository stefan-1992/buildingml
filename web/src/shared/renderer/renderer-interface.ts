/**
 * Common interface for 3D renderers (WebGL and Three.js).
 * Both WebGLRenderer and ThreeJSRenderer implement this interface.
 */
import type { CameraState } from './orbit-camera.ts';
import type { SectionCutState } from './section-cut.ts';
import type { SceneGraph } from '../types.ts';

/** Result of a pick/raycast operation. */
export interface PickResult {
  id: string;
  elementType: string;
  label: string;
  properties: [string, string][];
  /**
   * Material of the part actually under the cursor. An element's `properties`
   * list every material it uses; this narrows it to the one that was clicked
   * (a window's glazing rather than its frame). For an element without a
   * material of its own this is the palette look of its type (e.g. "Plaster").
   */
  materialName?: string;
  /** RGBA the part is drawn with, for a swatch in the inspector. */
  color?: [number, number, number, number];
}

/** One element in the loaded scene, for visibility filters and element lists. */
export interface ElementSummary {
  id: string;
  elementType: string;
  label: string;
  properties: [string, string][];
}

/** Callback when a 3D transform gizmo moves an element. */
export type TransformCallback = (id: string, delta: [number, number, number]) => void;

export interface IRenderer {
  camera: CameraState;
  wireframe: boolean;
  sectionCut: SectionCutState;
  lateralCut: SectionCutState;

  /** Currently highlighted (selected) element ID, or null. */
  highlightedId: string | null;

  /**
   * Element IDs to hide. Hidden meshes are skipped when drawing and when
   * picking, so you cannot select something you cannot see. Assign a new Set
   * to apply a change; the renderer does not observe mutation in place.
   */
  hiddenIds: Set<string>;

  /**
   * Per-element metadata for building visibility UIs, one entry per element
   * (sub-meshes sharing an id are collapsed). Valid after `loadScene`.
   */
  listElements(): ElementSummary[];

  /** Called when a transform gizmo moves an element (delta in world units). */
  onTransform: TransformCallback | null;

  /** Whether the gizmo is currently being dragged (suppresses orbit camera). */
  isDraggingGizmo: boolean;

  loadScene(sceneData: SceneGraph): void;
  start(): void;
  stop(): void;
  dispose(): void;

  /** Pick the element at canvas coordinates (x, y). Returns null if nothing hit. */
  pick(canvasX: number, canvasY: number): PickResult | null;

  /** Attach translate gizmo to element. Pass null to detach. */
  attachGizmo(elementId: string | null): void;
}
