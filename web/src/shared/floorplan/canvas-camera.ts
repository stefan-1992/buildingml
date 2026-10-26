/** 2D canvas camera: pan, zoom, coordinate transforms, snap. */

import type { Point2D } from './element-model.ts';

export const SNAP_SIZE = 0.25; // meters

export interface CanvasCamera {
  viewX: number;
  viewY: number;
  zoom: number; // pixels per meter
}

export function createDefaultCamera(): CanvasCamera {
  return { viewX: 0, viewY: 0, zoom: 50 };
}

export function worldToScreen(cam: CanvasCamera, wx: number, wy: number, canvasWidth: number, canvasHeight: number): Point2D {
  const cx = canvasWidth / 2;
  const cy = canvasHeight / 2;
  return {
    x: cx + (wx - cam.viewX) * cam.zoom,
    y: cy - (wy - cam.viewY) * cam.zoom, // flip Y: world Y+ is up, screen Y+ is down
  };
}

export function screenToWorld(cam: CanvasCamera, sx: number, sy: number, canvasWidth: number, canvasHeight: number): Point2D {
  const cx = canvasWidth / 2;
  const cy = canvasHeight / 2;
  return {
    x: cam.viewX + (sx - cx) / cam.zoom,
    y: cam.viewY - (sy - cy) / cam.zoom,
  };
}

export function snapToGrid(val: number, enabled: boolean): number {
  if (!enabled) return val;
  return Math.round(val / SNAP_SIZE) * SNAP_SIZE;
}

export function snapPoint(p: Point2D, enabled: boolean): Point2D {
  return { x: snapToGrid(p.x, enabled), y: snapToGrid(p.y, enabled) };
}

export function applyZoom(cam: CanvasCamera, factor: number, mouseScreenX: number, mouseScreenY: number, canvasWidth: number, canvasHeight: number): void {
  const worldBefore = screenToWorld(cam, mouseScreenX, mouseScreenY, canvasWidth, canvasHeight);
  cam.zoom *= factor;
  cam.zoom = Math.max(5, Math.min(500, cam.zoom));
  const worldAfter = screenToWorld(cam, mouseScreenX, mouseScreenY, canvasWidth, canvasHeight);
  cam.viewX += worldBefore.x - worldAfter.x;
  cam.viewY += worldBefore.y - worldAfter.y;
}
