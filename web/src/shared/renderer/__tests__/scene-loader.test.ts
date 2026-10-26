import { describe, it, expect } from 'vitest';
import { meshArrays, fitCameraToScene } from '../scene-loader.ts';
import { computeSectionCutBounds, computeLateralCutBounds } from '../section-cut.ts';
import { DEFAULT_CAMERA } from '../orbit-camera.ts';
import type { SceneGraph, SceneMesh } from '../../types.ts';

const objectMesh: SceneMesh = {
  vertices: [
    { position: [0, 1, 2], normal: [0, 0, 1] },
    { position: [3, 4, 5], normal: [1, 0, 0] },
  ],
  indices: [0, 1, 0],
  color: [1, 1, 1, 1],
};

// The form the VS Code extension posts.
const packedMesh: SceneMesh = {
  vertices: [],
  positions: new Float32Array([0, 1, 2, 3, 4, 5]),
  normals: new Float32Array([0, 0, 1, 1, 0, 0]),
  indices: new Uint32Array([0, 1, 0]),
  color: [1, 1, 1, 1],
};

function sceneOf(mesh: SceneMesh): SceneGraph {
  return { nodes: [{ id: 'n1', element_type: 'Wall', label: 'n1', mesh, children: [] }] };
}

describe('meshArrays', () => {
  it('flattens vertex objects', () => {
    const a = meshArrays(objectMesh);
    expect(Array.from(a.positions)).toEqual([0, 1, 2, 3, 4, 5]);
    expect(Array.from(a.normals)).toEqual([0, 0, 1, 1, 0, 0]);
    expect(Array.from(a.indices)).toEqual([0, 1, 0]);
  });

  it('passes packed arrays through without copying', () => {
    const a = meshArrays(packedMesh);
    expect(a.positions).toBe(packedMesh.positions);
    expect(a.normals).toBe(packedMesh.normals);
    expect(a.indices).toBe(packedMesh.indices);
  });
});

describe('bounds from packed meshes', () => {
  it('match the bounds of the same mesh as vertex objects', () => {
    for (const compute of [computeSectionCutBounds, computeLateralCutBounds]) {
      expect(compute(sceneOf(packedMesh))).toEqual(compute(sceneOf(objectMesh)));
    }
    const a = { ...DEFAULT_CAMERA, target: [...DEFAULT_CAMERA.target] as [number, number, number] };
    const b = { ...DEFAULT_CAMERA, target: [...DEFAULT_CAMERA.target] as [number, number, number] };
    fitCameraToScene(sceneOf(packedMesh), a);
    fitCameraToScene(sceneOf(objectMesh), b);
    expect(a).toEqual(b);
    expect(a.target).toEqual([1.5, 2.5, 3.5]);
  });
});
