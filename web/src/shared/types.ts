// TypeScript interfaces matching Rust serde output for BuildingML

// --- Scene Graph (from bml-core/src/scene/graph.rs) ---

export interface SceneGraph {
  nodes: SceneNode[];
  /** Shared material table; nodes reference entries by index. Absent for BML scenes. */
  materials?: SceneMaterial[];
}

/** A material resolved from IFC presentation styles. */
export interface SceneMaterial {
  name: string;
  color: [number, number, number, number]; // RGBA
  /** Specular intensity 0..1. */
  specular: number;
  /** Blinn-Phong exponent. */
  shininess: number;
}

export interface SceneNode {
  id: string;
  element_type: string;
  label: string;
  mesh: SceneMesh;
  children: SceneNode[];
  /** Key-value property metadata for interactive display (e.g. height, thickness). */
  properties?: [string, string][];
  /** Index into `SceneGraph.materials`; absent means flat shading of `mesh.color`. */
  material?: number;
}

export interface SceneMesh {
  vertices: Vertex[];
  indices: number[] | Uint32Array;
  color: [number, number, number, number]; // RGBA
  /**
   * Packed geometry (xyz per vertex). The VS Code extension posts meshes in
   * this form, with `vertices` empty, because a large IFC as JSON vertex
   * objects exceeds what a webview message can carry. See `meshArrays()`.
   */
  positions?: Float32Array;
  normals?: Float32Array;
}

export interface Vertex {
  position: [number, number, number];
  normal: [number, number, number];
}

// --- Semantic Model IR (from bml-core/src/semantic/model.rs) ---

export interface SemanticModel {
  sites: ResolvedSite[];
  materials: ResolvedMaterial[];
  asset_libraries: ResolvedAssetLibrary[];
}

export interface ResolvedAssetLibrary {
  name: string;
  properties: ResolvedProperty[];
  assets: ResolvedAsset[];
}

export interface ResolvedAsset {
  id: string;
  properties: ResolvedProperty[];
  parts: ResolvedAssetPart[];
}

export interface ResolvedAssetPart {
  name: string;
  position: [number, number, number];
  rotation: number;
  material: string | null;
}

export interface ResolvedSite {
  name: string;
  /** Created for a building without a `# Site` heading (omitted when false). */
  implicit?: boolean;
  properties: ResolvedProperty[];
  buildings: ResolvedBuilding[];
  /** Spaces and elements of the site itself, outside any building (omitted when empty). */
  spaces?: ResolvedSpace[];
  elements?: ResolvedElement[];
  span: Span;
}

export interface ResolvedBuilding {
  name: string;
  properties: ResolvedProperty[];
  storeys: ResolvedStorey[];
  /** Spaces and elements of the building itself, outside any storey (omitted when empty). */
  spaces?: ResolvedSpace[];
  elements?: ResolvedElement[];
  span: Span;
}

export interface ResolvedStorey {
  name: string;
  properties: ResolvedProperty[];
  spaces: ResolvedSpace[];
  elements: ResolvedElement[];
  span: Span;
}

export interface ResolvedSpace {
  name: string;
  properties: ResolvedProperty[];
  elements: ResolvedElement[];
  span: Span;
}

export interface ResolvedElement {
  element_type: ElementType;
  id: string;
  properties: ResolvedProperty[];
  children: ResolvedElement[];
  span: Span;
  /** Properties written with `smooth` / `nurbs` points (theirs is the tessellated polyline) */
  curved?: string[];
}

export type ElementType =
  | 'Wall' | 'Slab' | 'Column' | 'Beam'
  | 'Window' | 'Door' | 'Roof' | 'Stair'
  | 'Railing' | 'Ramp' | 'Furniture' | 'Elevator';

export interface ResolvedProperty {
  key: string;
  value: ResolvedValue;
  span: Span;
}

// Tagged union matching Rust enum serde output
export type ResolvedValue =
  | { Number: number }
  | { String: string }
  | { Boolean: boolean }
  | { Identifier: string }
  | { Point: [number, number] }
  | { Polygon: Array<[number, number]> }
  | { Profile: { profile_type: string; params: number[] } }
  | { Axis: { from: [number, number]; to: [number, number] } }
  | { EnumList: string[] }
  | { NumberList: number[] };

export interface ResolvedMaterial {
  id: string;
  material_type: MaterialType;
  properties: ResolvedProperty[];
  span: Span;
}

export type MaterialType =
  | { Layered: { layers: MaterialLayer[] } }
  | { Homogeneous: { category: string | null; grade: string | null; density: number | null } };

export interface MaterialLayer {
  name: string;
  thickness: number;
}

// --- Diagnostics (ad-hoc projection from Rust) ---

export interface Diagnostic {
  code: string;
  severity: 'error' | 'warning' | 'info';
  message: string;
  line?: number;
  column?: number;
  help: string | null;
}

// --- Span / Position ---

export interface Span {
  start: Position;
  end: Position;
}

export interface Position {
  line: number;
  column: number;
  byte_offset: number;
}

// --- Utility type guards ---

export function getResolvedNumber(v: ResolvedValue): number | undefined {
  return 'Number' in v ? v.Number : undefined;
}

export function getResolvedString(v: ResolvedValue): string | undefined {
  return 'String' in v ? v.String : undefined;
}

export function getResolvedAxis(v: ResolvedValue): { from: [number, number]; to: [number, number] } | undefined {
  return 'Axis' in v ? v.Axis : undefined;
}

export function getResolvedPoint(v: ResolvedValue): [number, number] | undefined {
  return 'Point' in v ? v.Point : undefined;
}

export function getResolvedPolygon(v: ResolvedValue): Array<[number, number]> | undefined {
  return 'Polygon' in v ? v.Polygon : undefined;
}

export function getProperty(props: ResolvedProperty[], key: string): ResolvedValue | undefined {
  const prop = props.find(p => p.key === key);
  return prop?.value;
}
