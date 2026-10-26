import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ifcToSceneGraph } from '../ifc-loader.ts';
import type { SceneGraph } from '../../types.ts';

/**
 * Mock IfcAPI that simulates web-ifc's geometry extraction.
 * Returns a single flat mesh with a simple triangle.
 */
function createMockIfcApi() {
  const mockGeometry = {
    GetVertexData: () => 100, // pointer (unused in mock)
    GetVertexDataSize: () => 18, // 3 vertices * 6 floats
    GetIndexData: () => 200, // pointer (unused in mock)
    GetIndexDataSize: () => 3, // 1 triangle
    delete: vi.fn(),
  };

  // 3 vertices: position + normal (6 floats each)
  const vertexArray = new Float32Array([
    // vertex 0: pos(0,0,0) normal(0,0,1)
    0, 0, 0, 0, 0, 1,
    // vertex 1: pos(1,0,0) normal(0,0,1)
    1, 0, 0, 0, 0, 1,
    // vertex 2: pos(0,1,0) normal(0,0,1)
    0, 1, 0, 0, 0, 1,
  ]);

  const indexArray = new Uint32Array([0, 1, 2]);

  // Identity matrix (column-major)
  const identityMatrix = [
    1, 0, 0, 0,
    0, 1, 0, 0,
    0, 0, 1, 0,
    0, 0, 0, 1,
  ];

  const placedGeometry = {
    color: { x: 0.8, y: 0.2, z: 0.1, w: 1.0 },
    geometryExpressID: 42,
    flatTransformation: identityMatrix,
  };

  const geometriesVector = {
    size: () => 1,
    get: (idx: number) => {
      if (idx === 0) return placedGeometry;
      throw new Error('out of bounds');
    },
  };

  const api = {
    OpenModel: vi.fn().mockReturnValue(0),
    CloseModel: vi.fn(),
    GetGeometry: vi.fn().mockReturnValue(mockGeometry),
    GetVertexArray: vi.fn().mockReturnValue(vertexArray),
    GetIndexArray: vi.fn().mockReturnValue(indexArray),
    GetLineType: vi.fn().mockReturnValue(1000),
    GetNameFromTypeCode: vi.fn().mockReturnValue('IfcWall'),
    StreamAllMeshes: vi.fn((_modelID: number, callback: (mesh: any) => void) => {
      callback({
        expressID: 101,
        geometries: geometriesVector,
      });
    }),
  };

  return api;
}

describe('ifcToSceneGraph', () => {
  let mockApi: ReturnType<typeof createMockIfcApi>;

  beforeEach(() => {
    mockApi = createMockIfcApi();
  });

  it('converts IFC geometry into a SceneGraph with correct structure', () => {
    const scene: SceneGraph = ifcToSceneGraph(mockApi as any, 'ISO-10303-21; ...');

    expect(scene.nodes).toHaveLength(1);
    const node = scene.nodes[0];
    expect(node.id).toBe('ifc-101-0');
    expect(node.element_type).toBe('Wall');
    expect(node.label).toBe('Wall #101');
    expect(node.children).toEqual([]);
  });

  it('converts Y-up to Z-up coordinates', () => {
    const scene = ifcToSceneGraph(mockApi as any, 'test');
    const mesh = scene.nodes[0].mesh;

    expect(mesh.vertices).toHaveLength(3);
    // Identity transform, then Y-up→Z-up: (x,y,z) → (x,-z,y)
    expect(mesh.vertices[0].position).toEqual([0, 0, 0]);
    expect(mesh.vertices[1].position).toEqual([1, 0, 0]);
    expect(mesh.vertices[2].position).toEqual([0, 0, 1]);
    // Normal (0,0,1) → (0,-1,0) after Y-up→Z-up
    expect(mesh.vertices[0].normal).toEqual([0, -1, 0]);
  });

  it('extracts correct indices', () => {
    const scene = ifcToSceneGraph(mockApi as any, 'test');
    expect(scene.nodes[0].mesh.indices).toEqual([0, 1, 2]);
  });

  it('uses Wall element-type color for IfcWall', () => {
    const scene = ifcToSceneGraph(mockApi as any, 'test');
    const color = scene.nodes[0].mesh.color;
    // Wall color: [0.85, 0.82, 0.78, 1.0]
    expect(color[0]).toBeCloseTo(0.85);
    expect(color[1]).toBeCloseTo(0.82);
    expect(color[2]).toBeCloseTo(0.78);
    expect(color[3]).toBeCloseTo(1.0);
  });

  it('applies a non-identity transform to vertices', () => {
    // Translation by (5, 10, 15) — column-major 4x4
    const translationMatrix = [
      1, 0, 0, 0,
      0, 1, 0, 0,
      0, 0, 1, 0,
      5, 10, 15, 1,
    ];
    mockApi.StreamAllMeshes.mockImplementation((_modelID: number, callback: (mesh: any) => void) => {
      callback({
        expressID: 200,
        geometries: {
          size: () => 1,
          get: () => ({
            color: { x: 0.5, y: 0.5, z: 0.5, w: 1.0 },
            geometryExpressID: 42,
            flatTransformation: translationMatrix,
          }),
        },
      });
    });

    const scene = ifcToSceneGraph(mockApi as any, 'test');
    const v = scene.nodes[0].mesh.vertices;

    // After transform then Y-up→Z-up: (x,y,z) → (x,-z,y)
    expect(v[0].position).toEqual([5, -15, 10]);
    expect(v[1].position).toEqual([6, -15, 10]);
    expect(v[2].position).toEqual([5, -15, 11]);
    // Normal (0,0,1) → (0,-1,0) after Y-up→Z-up
    expect(v[0].normal).toEqual([0, -1, 0]);
  });

  it('handles multiple placed geometries per element', () => {
    mockApi.StreamAllMeshes.mockImplementation((_modelID: number, callback: (mesh: any) => void) => {
      callback({
        expressID: 300,
        geometries: {
          size: () => 2,
          get: (idx: number) => ({
            color: { x: idx === 0 ? 1 : 0, y: 0, z: idx === 1 ? 1 : 0, w: 1.0 },
            geometryExpressID: 42,
            flatTransformation: [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1],
          }),
        },
      });
    });

    const scene = ifcToSceneGraph(mockApi as any, 'test');
    expect(scene.nodes).toHaveLength(2);
    expect(scene.nodes[0].id).toBe('ifc-300-0');
    expect(scene.nodes[1].id).toBe('ifc-300-1');
    // Both get Wall color (element type from mock)
    expect(scene.nodes[0].mesh.color[0]).toBeCloseTo(0.85);
    expect(scene.nodes[1].mesh.color[0]).toBeCloseTo(0.85);
  });

  it('closes the model even on error', () => {
    mockApi.StreamAllMeshes.mockImplementation(() => {
      throw new Error('parse error');
    });

    expect(() => ifcToSceneGraph(mockApi as any, 'bad data')).toThrow('parse error');
    expect(mockApi.CloseModel).toHaveBeenCalledTimes(1);
  });

  it('returns empty nodes array for empty model', () => {
    mockApi.StreamAllMeshes.mockImplementation(() => {
      // No callback invocations — empty model
    });

    const scene = ifcToSceneGraph(mockApi as any, 'test');
    expect(scene.nodes).toEqual([]);
  });

  it('strips "Ifc" prefix from element type labels', () => {
    mockApi.GetNameFromTypeCode.mockReturnValue('IfcBeamStandardCase');
    const scene = ifcToSceneGraph(mockApi as any, 'test');
    expect(scene.nodes[0].element_type).toBe('BeamStandardCase');
  });

  it('handles GetLineType failure gracefully', () => {
    mockApi.GetLineType.mockImplementation(() => { throw new Error('unknown'); });
    const scene = ifcToSceneGraph(mockApi as any, 'test');
    expect(scene.nodes[0].element_type).toBe('Element');
    expect(scene.nodes[0].label).toBe('Element #101');
  });

  it('throws if model fails to open', () => {
    mockApi.OpenModel.mockReturnValue(-1);
    expect(() => ifcToSceneGraph(mockApi as any, 'test')).toThrow('Failed to open IFC model');
  });

  it('uses element-type color for known IFC types', () => {
    mockApi.StreamAllMeshes.mockImplementation((_modelID: number, callback: (mesh: any) => void) => {
      callback({
        expressID: 500,
        geometries: {
          size: () => 1,
          get: () => ({
            color: { x: 0.8, y: 0.8, z: 0.8, w: 1.0 },
            geometryExpressID: 42,
            flatTransformation: [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1],
          }),
        },
      });
    });
    mockApi.GetNameFromTypeCode.mockReturnValue('IfcRoof');

    const scene = ifcToSceneGraph(mockApi as any, 'test');
    const color = scene.nodes[0].mesh.color;
    // Should use Roof color: [0.72, 0.33, 0.26, 1.0]
    expect(color[0]).toBeCloseTo(0.72);
    expect(color[1]).toBeCloseTo(0.33);
    expect(color[2]).toBeCloseTo(0.26);
    expect(color[3]).toBeCloseTo(1.0);
  });

  it('falls back to web-ifc color for unknown element types', () => {
    mockApi.StreamAllMeshes.mockImplementation((_modelID: number, callback: (mesh: any) => void) => {
      callback({
        expressID: 600,
        geometries: {
          size: () => 1,
          get: () => ({
            color: { x: 0.2, y: 0.6, z: 0.9, w: 0.8 },
            geometryExpressID: 42,
            flatTransformation: [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1],
          }),
        },
      });
    });
    // Unknown type not in ELEMENT_TYPE_COLORS
    mockApi.GetNameFromTypeCode.mockReturnValue('IfcBuildingElementProxy');

    const scene = ifcToSceneGraph(mockApi as any, 'test');
    const color = scene.nodes[0].mesh.color;
    // Falls back to web-ifc color
    expect(color[0]).toBeCloseTo(0.2);
    expect(color[1]).toBeCloseTo(0.6);
    expect(color[2]).toBeCloseTo(0.9);
    expect(color[3]).toBeCloseTo(0.8);
  });

  it('skips IfcOpeningElement geometry', () => {
    mockApi.StreamAllMeshes.mockImplementation((_modelID: number, callback: (mesh: any) => void) => {
      // First: a wall
      callback({
        expressID: 700,
        geometries: {
          size: () => 1,
          get: () => ({
            color: { x: 0.8, y: 0.2, z: 0.1, w: 1.0 },
            geometryExpressID: 42,
            flatTransformation: [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1],
          }),
        },
      });
      // Second: an opening element (should be skipped)
      callback({
        expressID: 701,
        geometries: {
          size: () => 1,
          get: () => ({
            color: { x: 0.5, y: 0.5, z: 0.5, w: 1.0 },
            geometryExpressID: 43,
            flatTransformation: [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1],
          }),
        },
      });
    });

    let callCount = 0;
    mockApi.GetNameFromTypeCode.mockImplementation(() => {
      callCount++;
      return callCount === 1 ? 'IfcWall' : 'IfcOpeningElement';
    });

    const scene = ifcToSceneGraph(mockApi as any, 'test');
    // Only the wall should be in the scene, opening element filtered out
    expect(scene.nodes).toHaveLength(1);
    expect(scene.nodes[0].element_type).toBe('Wall');
  });
});
