/**
 * IFC-to-SceneGraph converter using web-ifc.
 *
 * Parses compiled IFC STEP strings via the web-ifc WASM engine,
 * extracts triangle geometry, and converts to SceneGraph format
 * for rendering with the existing WebGLRenderer.
 */
import type { IfcAPI, FlatMesh, PlacedGeometry, IfcGeometry } from 'web-ifc';
import type { SceneGraph, SceneNode, Vertex } from '../types.ts';

let ifcApiInstance: IfcAPI | null = null;
let initPromise: Promise<IfcAPI> | null = null;

/**
 * Lazily initialize the web-ifc WASM API (singleton).
 * @param wasmPath Directory containing web-ifc.wasm (must end with '/')
 */
export async function initIfcApi(wasmPath?: string): Promise<IfcAPI> {
  if (ifcApiInstance) return ifcApiInstance;
  if (initPromise) return initPromise;

  initPromise = (async () => {
    // Dynamic import so web-ifc is only loaded when needed
    const WebIFC = await import('web-ifc');
    const api = new WebIFC.IfcAPI();

    if (wasmPath) {
      api.SetWasmPath(wasmPath, true);
    }

    await api.Init();
    ifcApiInstance = api;
    return api;
  })();

  return initPromise;
}

/**
 * Apply a 4x4 transformation matrix (column-major) to a position or normal vector.
 */
function transformPoint(
  x: number, y: number, z: number,
  m: number[],
  isNormal: boolean,
): [number, number, number] {
  if (isNormal) {
    // Normals: rotate only (upper-left 3x3), then normalize
    const nx = m[0] * x + m[4] * y + m[8] * z;
    const ny = m[1] * x + m[5] * y + m[9] * z;
    const nz = m[2] * x + m[6] * y + m[10] * z;
    const len = Math.hypot(nx, ny, nz) || 1;
    return [nx / len, ny / len, nz / len];
  }
  // Positions: full affine transform
  const px = m[0] * x + m[4] * y + m[8] * z + m[12];
  const py = m[1] * x + m[5] * y + m[9] * z + m[13];
  const pz = m[2] * x + m[6] * y + m[10] * z + m[14];
  return [px, py, pz];
}

/**
 * Map an IFC type code to a human-readable element type string.
 */
function ifcTypeLabel(api: IfcAPI, modelID: number, expressID: number): string {
  try {
    const typeCode = api.GetLineType(modelID, expressID);
    const name = api.GetNameFromTypeCode(typeCode);
    // Strip "Ifc" prefix for display, e.g. "IfcWall" → "Wall"
    return name.startsWith('Ifc') ? name.slice(3) : name;
  } catch {
    return 'Element';
  }
}

/**
 * Element-type color mapping matching the BML scene graph colors.
 * Used as fallback when IFC elements lack IfcStyledItem presentation styles.
 */
const ELEMENT_TYPE_COLORS: Record<string, [number, number, number, number]> = {
  Wall:                [0.85, 0.82, 0.78, 1.0],
  WallStandardCase:    [0.85, 0.82, 0.78, 1.0],
  Slab:                [0.75, 0.75, 0.73, 1.0],
  Column:              [0.70, 0.70, 0.68, 1.0],
  Beam:                [0.65, 0.65, 0.63, 1.0],
  Roof:                [0.72, 0.33, 0.26, 1.0],
  Stair:               [0.80, 0.78, 0.75, 1.0],
  StairFlight:         [0.80, 0.78, 0.75, 1.0],
  Railing:             [0.40, 0.40, 0.38, 1.0],
  Ramp:                [0.78, 0.76, 0.72, 1.0],
  RampFlight:          [0.78, 0.76, 0.72, 1.0],
  FurnishingElement:   [0.60, 0.45, 0.30, 1.0],
  Door:                [0.55, 0.35, 0.15, 1.0],
  Window:              [0.50, 0.70, 0.90, 0.5],
  TransportElement:    [0.50, 0.50, 0.55, 0.7],
  Space:               [0.70, 0.85, 0.95, 0.15],
  OpeningElement:      [0.0, 0.0, 0.0, 0.0], // invisible
};

/** Get color for an element: prefer type-based color, fall back to web-ifc color. */
function resolveColor(
  elementType: string,
  ifcColor: { x: number; y: number; z: number; w: number },
): [number, number, number, number] {
  return ELEMENT_TYPE_COLORS[elementType] ?? [ifcColor.x, ifcColor.y, ifcColor.z, ifcColor.w];
}

/**
 * Convert an IFC STEP string into a SceneGraph for rendering.
 *
 * @param api Initialized IfcAPI instance (from initIfcApi)
 * @param ifcContent IFC file content as a string
 * @returns SceneGraph compatible with loadSceneToGpu / WebGLRenderer
 */
export function ifcToSceneGraph(api: IfcAPI, ifcContent: string): SceneGraph {
  const encoder = new TextEncoder();
  const data = encoder.encode(ifcContent);
  const modelID = api.OpenModel(data);

  if (modelID < 0) {
    throw new Error('Failed to open IFC model');
  }

  const nodes: SceneNode[] = [];

  try {
    api.StreamAllMeshes(modelID, (flatMesh: FlatMesh) => {
      const expressID = flatMesh.expressID;
      const elementType = ifcTypeLabel(api, modelID, expressID);

      // Skip opening elements — they are boolean subtraction volumes, not visible
      if (elementType === 'OpeningElement') return;

      const geometries = flatMesh.geometries;
      for (let i = 0; i < geometries.size(); i++) {
        const placed: PlacedGeometry = geometries.get(i);
        const geom: IfcGeometry = api.GetGeometry(modelID, placed.geometryExpressID);

        const vertexDataPtr = geom.GetVertexData();
        const vertexDataSize = geom.GetVertexDataSize();
        const indexDataPtr = geom.GetIndexData();
        const indexDataSize = geom.GetIndexDataSize();

        const rawVertices = api.GetVertexArray(vertexDataPtr, vertexDataSize);
        const rawIndices = api.GetIndexArray(indexDataPtr, indexDataSize);

        const transform = placed.flatTransformation as unknown as number[];
        const color = resolveColor(elementType, placed.color);

        // Skip fully transparent elements
        if (color[3] <= 0) {
          geom.delete();
          continue;
        }

        // web-ifc vertex layout: [x, y, z, nx, ny, nz] per vertex (6 floats)
        // web-ifc bakes a Z-up→Y-up rotation into flatTransformation (for Three.js).
        // Our renderer uses Z-up. Convert back: (x, y, z)_yup → (x, -z, y)_zup
        const vertexCount = rawVertices.length / 6;
        const vertices: Vertex[] = new Array(vertexCount);

        for (let v = 0; v < vertexCount; v++) {
          const off = v * 6;
          const pos = transformPoint(
            rawVertices[off], rawVertices[off + 1], rawVertices[off + 2],
            transform, false,
          );
          const norm = transformPoint(
            rawVertices[off + 3], rawVertices[off + 4], rawVertices[off + 5],
            transform, true,
          );
          vertices[v] = {
            position: [pos[0], (-pos[2]) || 0, pos[1]],
            normal: [norm[0], (-norm[2]) || 0, norm[1]],
          };
        }

        const indices = Array.from(rawIndices);

        const node: SceneNode = {
          id: `ifc-${expressID}-${i}`,
          element_type: elementType,
          label: `${elementType} #${expressID}`,
          mesh: {
            vertices,
            indices,
            color,
          },
          children: [],
        };

        nodes.push(node);
        geom.delete();
      }
    });
  } finally {
    api.CloseModel(modelID);
  }

  return { nodes };
}
