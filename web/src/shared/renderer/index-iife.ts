/**
 * IIFE entry point for VS Code extension webview.
 * Excludes ThreeJSRenderer and web-ifc (ifc-loader) to avoid CSP violations
 * from Emscripten's eval/new Function() in the VS Code webview sandbox.
 * The web app uses index.ts which includes all exports.
 */
export { mat4Perspective, mat4LookAt, mat4Identity } from './math.ts';
export { VERTEX_SHADER, FRAGMENT_SHADER, createShaderProgram, type ShaderLocations } from './shaders.ts';
export { OrbitCameraController, getCameraPosition, DEFAULT_CAMERA, type CameraState } from './orbit-camera.ts';
export { computeSectionCutBounds, computeLateralCutBounds, DEFAULT_SECTION_CUT, cutFraction, applyCutFraction, type SectionCutState } from './section-cut.ts';
export { loadSceneToGpu, fitCameraToScene, getSceneStats, type GpuMesh } from './scene-loader.ts';
export { WebGLRenderer, type RendererState } from './webgl-renderer.ts';
export type { IRenderer, PickResult, TransformCallback } from './renderer-interface.ts';

// Floorplan 2D modules (used by VS Code 2D preview)
export { drawAll, type DrawState } from '../floorplan/drawing.ts';
export { importStorey, getStoreys, type ImportedStorey } from '../floorplan/model-importer.ts';
export { detectSpaces, pointInPolygon } from '../floorplan/space-detection.ts';
export {
  worldToScreen, screenToWorld, applyZoom, createDefaultCamera,
  type CanvasCamera,
} from '../floorplan/canvas-camera.ts';
export type {
  FloorplanElement, WallElement, DoorElement, WindowElement,
  ColumnElement, SlabElement, DetectedSpace, Point2D,
} from '../floorplan/element-model.ts';
export { wallLength } from '../floorplan/element-model.ts';
