import type { SceneGraph, SceneMesh, SceneNode } from '../types.ts';
import type { CameraState } from './orbit-camera.ts';

/** Shading defaults for meshes without a resolved IFC material. */
const DEFAULT_SPECULAR = 0.0;
const DEFAULT_SHININESS = 16.0;

export interface GpuMesh {
  id: string;
  elementType: string;
  label: string;
  vbo: WebGLBuffer;
  ibo: WebGLBuffer;
  indexCount: number;
  color: Float32Array;
  properties: [string, string][];
  /** Resolved material name, empty when the mesh is coloured by element type. */
  materialName: string;
  specular: number;
  shininess: number;
  /** Cleared by `hiddenIds` — hidden meshes are skipped when drawing and picking. */
  visible: boolean;
}

/** A mesh's geometry as flat typed arrays, whether it arrived packed or as vertex objects. */
export function meshArrays(mesh: SceneMesh): {
  positions: Float32Array;
  normals: Float32Array;
  indices: Uint32Array;
} {
  const indices = mesh.indices instanceof Uint32Array ? mesh.indices : new Uint32Array(mesh.indices);
  if (mesh.positions && mesh.normals) {
    return { positions: mesh.positions, normals: mesh.normals, indices };
  }
  const verts = mesh.vertices;
  const positions = new Float32Array(verts.length * 3);
  const normals = new Float32Array(verts.length * 3);
  for (let i = 0, j = 0; i < verts.length; i++, j += 3) {
    const p = verts[i].position;
    const n = verts[i].normal;
    positions[j] = p[0]; positions[j + 1] = p[1]; positions[j + 2] = p[2];
    normals[j] = n[0]; normals[j + 1] = n[1]; normals[j + 2] = n[2];
  }
  return { positions, normals, indices };
}

/** Call `visit` with every vertex position of a mesh, packed or not, without copying. */
export function forEachPosition(mesh: SceneMesh, visit: (x: number, y: number, z: number) => void): void {
  const p = mesh.positions;
  if (p) {
    for (let i = 0; i < p.length; i += 3) visit(p[i], p[i + 1], p[i + 2]);
  } else {
    for (const v of mesh.vertices) visit(v.position[0], v.position[1], v.position[2]);
  }
}

/** Load a SceneGraph into GPU buffers and return mesh list. Disposes old meshes. */
export function loadSceneToGpu(
  gl: WebGLRenderingContext,
  sceneData: SceneGraph,
  existingMeshes: GpuMesh[],
): GpuMesh[] {
  // Clean up old buffers
  for (const mesh of existingMeshes) {
    gl.deleteBuffer(mesh.vbo);
    gl.deleteBuffer(mesh.ibo);
  }

  const meshes: GpuMesh[] = [];

  function loadNode(node: SceneNode, inherited: [string, string][]): void {
    const { positions, normals, indices: idxData } = meshArrays(node.mesh);

    // Interleaved buffer: [px, py, pz, nx, ny, nz]
    const vertCount = positions.length / 3;
    const vertData = new Float32Array(vertCount * 6);
    for (let i = 0; i < vertCount; i++) {
      vertData[i * 6 + 0] = positions[i * 3 + 0];
      vertData[i * 6 + 1] = positions[i * 3 + 1];
      vertData[i * 6 + 2] = positions[i * 3 + 2];
      vertData[i * 6 + 3] = normals[i * 3 + 0];
      vertData[i * 6 + 4] = normals[i * 3 + 1];
      vertData[i * 6 + 5] = normals[i * 3 + 2];
    }

    const vbo = gl.createBuffer()!;
    gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
    gl.bufferData(gl.ARRAY_BUFFER, vertData, gl.STATIC_DRAW);

    const ibo = gl.createBuffer()!;
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, ibo);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, idxData, gl.STATIC_DRAW);

    const material =
      node.material !== undefined ? sceneData.materials?.[node.material] : undefined;

    // Sub-meshes of one IFC element share its id and carry no properties of
    // their own, so they inherit the element's rather than repeating them.
    const properties = node.properties?.length ? node.properties : inherited;

    meshes.push({
      id: node.id,
      elementType: node.element_type,
      label: node.label,
      vbo,
      ibo,
      indexCount: idxData.length,
      color: new Float32Array(node.mesh.color),
      properties,
      materialName: material?.name ?? '',
      specular: material?.specular ?? DEFAULT_SPECULAR,
      shininess: material?.shininess ?? DEFAULT_SHININESS,
      visible: true,
    });

    if (node.children) {
      for (const child of node.children) {
        loadNode(child, properties);
      }
    }
  }

  for (const node of sceneData.nodes) {
    loadNode(node, []);
  }

  return meshes;
}

/** Compute scene bounds and set camera to fit. */
export function fitCameraToScene(sceneData: SceneGraph, camera: CameraState): void {
  let minX = Infinity, minY = Infinity, minZ = Infinity;
  let maxX = -Infinity, maxY = -Infinity, maxZ = -Infinity;

  function scanNode(node: SceneNode): void {
    forEachPosition(node.mesh, (x, y, z) => {
      minX = Math.min(minX, x);
      minY = Math.min(minY, y);
      minZ = Math.min(minZ, z);
      maxX = Math.max(maxX, x);
      maxY = Math.max(maxY, y);
      maxZ = Math.max(maxZ, z);
    });
    if (node.children) node.children.forEach(scanNode);
  }
  sceneData.nodes.forEach(scanNode);

  if (minX === Infinity) return;

  camera.target = [
    (minX + maxX) / 2,
    (minY + maxY) / 2,
    (minZ + maxZ) / 2,
  ];

  const dx = maxX - minX, dy = maxY - minY, dz = maxZ - minZ;
  camera.distance = Math.max(dx, dy, dz) * 1.5;
  camera.rotX = 0.5;
  camera.rotY = 0.8;
}

/** Count scene statistics. */
export function getSceneStats(sceneData: SceneGraph): { nodeCount: number; triCount: number } {
  let nodeCount = 0;
  let triCount = 0;

  function countNode(node: SceneNode): void {
    nodeCount++;
    triCount += node.mesh.indices.length / 3;
    if (node.children) node.children.forEach(countNode);
  }
  sceneData.nodes.forEach(countNode);

  return { nodeCount, triCount };
}
