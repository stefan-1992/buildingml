/** All drawing functions for the 2D floorplan canvas. */

import type { FloorplanElement, WallElement, DoorElement, WindowElement, ColumnElement, SlabElement, FurnitureElement, DetectedSpace, Point2D } from './element-model.ts';
import { wallLength } from './element-model.ts';
import { worldToScreen, screenToWorld, type CanvasCamera } from './canvas-camera.ts';

export interface DrawState {
  elements: FloorplanElement[];
  detectedSpaces: DetectedSpace[];
  selectedId: string | null;
  hoveredId: string | null;
  currentTool: string;
  showGrid: boolean;
  snapEnabled: boolean;
  wallStart: Point2D | null;
  slabPoints: Point2D[];
  ghostPos: Point2D | null;
  camera: CanvasCamera;
  perpDragMode: boolean;
  perpDragPolygon: Point2D[] | null;
}

function w2s(cam: CanvasCamera, wx: number, wy: number, cw: number, ch: number): Point2D {
  return worldToScreen(cam, wx, wy, cw, ch);
}

export function drawAll(ctx: CanvasRenderingContext2D, state: DrawState, canvasWidth: number, canvasHeight: number): void {
  const { camera, elements, detectedSpaces, showGrid } = state;
  const w = canvasWidth;
  const h = canvasHeight;

  // Background
  ctx.fillStyle = '#15151f';
  ctx.fillRect(0, 0, w, h);

  // Grid
  if (showGrid) drawGrid(ctx, camera, w, h);

  // Draw spaces (below everything)
  detectedSpaces.forEach(s => {
    if (state.perpDragMode && state.perpDragPolygon && state.selectedId === s.id) {
      drawSpace(ctx, { ...s, polygon: state.perpDragPolygon }, state, w, h);
    } else {
      drawSpace(ctx, s, state, w, h);
    }
  });

  // Draw slabs
  elements.filter((e): e is SlabElement => e.type === 'Slab').forEach(e => drawSlab(ctx, e, state, w, h));

  // Draw walls
  elements.filter((e): e is WallElement => e.type === 'Wall').forEach(e => drawWall(ctx, e, state, w, h));

  // Draw openings
  elements.filter((e): e is DoorElement | WindowElement => e.type === 'Door' || e.type === 'Window')
    .forEach(e => drawOpening(ctx, e, state, w, h));

  // Draw columns
  elements.filter((e): e is ColumnElement => e.type === 'Column').forEach(e => drawColumn(ctx, e, state, w, h));

  // Draw furniture
  elements.filter((e): e is FurnitureElement => e.type === 'Furniture').forEach(e => drawFurniture(ctx, e, state, w, h));

  // Ghost preview
  drawGhost(ctx, state, w, h);

  // Selection highlight
  if (state.selectedId && !state.selectedId.startsWith('space-')) {
    const sel = elements.find(e => e.id === state.selectedId);
    if (sel) drawSelectionHighlight(ctx, sel, state.camera, w, h);
  }
}

function drawGrid(ctx: CanvasRenderingContext2D, cam: CanvasCamera, w: number, h: number): void {
  const topLeft = screenToWorld(cam, 0, 0, w, h);
  const bottomRight = screenToWorld(cam, w, h, w, h);

  let gridStep = 1;
  if (cam.zoom < 20) gridStep = 5;
  if (cam.zoom < 8) gridStep = 10;
  if (cam.zoom > 100) gridStep = 0.5;
  if (cam.zoom > 200) gridStep = 0.25;

  const minX = Math.floor(Math.min(topLeft.x, bottomRight.x) / gridStep) * gridStep;
  const maxX = Math.ceil(Math.max(topLeft.x, bottomRight.x) / gridStep) * gridStep;
  const minY = Math.floor(Math.min(topLeft.y, bottomRight.y) / gridStep) * gridStep;
  const maxY = Math.ceil(Math.max(topLeft.y, bottomRight.y) / gridStep) * gridStep;

  ctx.lineWidth = 1;

  for (let x = minX; x <= maxX; x += gridStep) {
    const isMajor = Math.abs(x % (gridStep * 5)) < 0.001 || Math.abs(x) < 0.001;
    ctx.strokeStyle = Math.abs(x) < 0.001 ? 'rgba(137,180,250,0.3)' : (isMajor ? 'rgba(100,100,140,0.3)' : 'rgba(100,100,140,0.12)');
    const s = w2s(cam, x, 0, w, h);
    ctx.beginPath();
    ctx.moveTo(s.x, 0);
    ctx.lineTo(s.x, h);
    ctx.stroke();
  }

  for (let y = minY; y <= maxY; y += gridStep) {
    const isMajor = Math.abs(y % (gridStep * 5)) < 0.001 || Math.abs(y) < 0.001;
    ctx.strokeStyle = Math.abs(y) < 0.001 ? 'rgba(137,180,250,0.3)' : (isMajor ? 'rgba(100,100,140,0.3)' : 'rgba(100,100,140,0.12)');
    const s = w2s(cam, 0, y, w, h);
    ctx.beginPath();
    ctx.moveTo(0, s.y);
    ctx.lineTo(w, s.y);
    ctx.stroke();
  }

  // Origin marker
  const o = w2s(cam, 0, 0, w, h);
  ctx.fillStyle = 'rgba(137,180,250,0.5)';
  ctx.beginPath();
  ctx.arc(o.x, o.y, 4, 0, Math.PI * 2);
  ctx.fill();
}

function drawWall(ctx: CanvasRenderingContext2D, wall: WallElement, state: DrawState, w: number, h: number): void {
  const cam = state.camera;
  const f = w2s(cam, wall.from.x, wall.from.y, w, h);
  const t = w2s(cam, wall.to.x, wall.to.y, w, h);
  const thickPx = wall.thickness * cam.zoom;
  const angle = Math.atan2(t.y - f.y, t.x - f.x);
  const nx = Math.sin(angle) * thickPx / 2;
  const ny = -Math.cos(angle) * thickPx / 2;

  const isHovered = state.hoveredId === wall.id;
  const isSelected = state.selectedId === wall.id;

  ctx.fillStyle = isHovered ? '#d5cfc5' : '#c8c0b8';
  ctx.strokeStyle = isSelected ? '#89b4fa' : '#8a8278';
  ctx.lineWidth = isSelected ? 2 : 1;

  ctx.beginPath();
  ctx.moveTo(f.x - nx, f.y - ny);
  ctx.lineTo(t.x - nx, t.y - ny);
  ctx.lineTo(t.x + nx, t.y + ny);
  ctx.lineTo(f.x + nx, f.y + ny);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();

  // Wall endpoints
  ctx.fillStyle = isSelected ? '#89b4fa' : '#8a8278';
  ctx.beginPath();
  ctx.arc(f.x, f.y, 3, 0, Math.PI * 2);
  ctx.fill();
  ctx.beginPath();
  ctx.arc(t.x, t.y, 3, 0, Math.PI * 2);
  ctx.fill();

  // Dimension label
  const len = wallLength(wall);
  if (cam.zoom > 15) {
    const mid = { x: (f.x + t.x) / 2, y: (f.y + t.y) / 2 };
    ctx.save();
    ctx.translate(mid.x, mid.y);
    let textAngle = angle;
    if (textAngle > Math.PI / 2 || textAngle < -Math.PI / 2) textAngle += Math.PI;
    ctx.rotate(textAngle);
    ctx.fillStyle = 'rgba(255,255,255,0.7)';
    ctx.font = '10px monospace';
    ctx.textAlign = 'center';
    ctx.fillText(len.toFixed(2) + 'm', 0, -thickPx / 2 - 4);
    ctx.restore();
  }

  // Wall ID label
  if (cam.zoom > 25) {
    const mid = { x: (f.x + t.x) / 2, y: (f.y + t.y) / 2 };
    ctx.fillStyle = 'rgba(200,200,220,0.5)';
    ctx.font = '9px monospace';
    ctx.textAlign = 'center';
    ctx.fillText(wall.id, mid.x, mid.y + thickPx / 2 + 12);
  }

  // Anchor indicator
  if (wall.anchored) {
    const mid = { x: (f.x + t.x) / 2, y: (f.y + t.y) / 2 };
    const r = 6;
    ctx.fillStyle = '#f38ba8';
    ctx.beginPath();
    ctx.moveTo(mid.x, mid.y - r);
    ctx.lineTo(mid.x + r, mid.y);
    ctx.lineTo(mid.x, mid.y + r);
    ctx.lineTo(mid.x - r, mid.y);
    ctx.closePath();
    ctx.fill();
    ctx.fillStyle = '#1e1e2e';
    ctx.font = 'bold 8px monospace';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText('A', mid.x, mid.y);
    ctx.textBaseline = 'alphabetic';
  }
}

function drawOpening(ctx: CanvasRenderingContext2D, elem: DoorElement | WindowElement, state: DrawState, w: number, h: number): void {
  const cam = state.camera;
  const wall = state.elements.find(e => e.id === elem.parentWall) as WallElement | undefined;
  if (!wall) return;

  const f = w2s(cam, wall.from.x, wall.from.y, w, h);
  const t = w2s(cam, wall.to.x, wall.to.y, w, h);
  const angle = Math.atan2(t.y - f.y, t.x - f.x);
  const thickPx = wall.thickness * cam.zoom;
  const offsetPx = elem.offset * cam.zoom;
  const widthPx = elem.width * cam.zoom;

  const isHovered = state.hoveredId === elem.id;
  const isSelected = state.selectedId === elem.id;

  ctx.save();
  ctx.translate(f.x, f.y);
  ctx.rotate(angle);

  if (elem.type === 'Door') {
    ctx.fillStyle = '#15151f';
    ctx.fillRect(offsetPx, -thickPx / 2 - 1, widthPx, thickPx + 2);

    ctx.strokeStyle = isSelected ? '#89b4fa' : (isHovered ? '#c08040' : '#a06020');
    ctx.lineWidth = isSelected ? 2.5 : 2;
    ctx.beginPath();
    ctx.moveTo(offsetPx, -thickPx / 2);
    ctx.lineTo(offsetPx, -thickPx / 2 - widthPx * 0.8);
    ctx.stroke();

    ctx.strokeStyle = isSelected ? 'rgba(137,180,250,0.4)' : 'rgba(160,96,32,0.3)';
    ctx.lineWidth = 1;
    ctx.setLineDash([3, 3]);
    ctx.beginPath();
    ctx.arc(offsetPx, -thickPx / 2, widthPx * 0.8, -Math.PI / 2, 0);
    ctx.stroke();
    ctx.setLineDash([]);

    if (cam.zoom > 25) {
      ctx.fillStyle = 'rgba(200,170,120,0.7)';
      ctx.font = '9px monospace';
      ctx.textAlign = 'center';
      ctx.fillText(elem.id, offsetPx + widthPx / 2, thickPx / 2 + 12);
    }
  } else if (elem.type === 'Window') {
    ctx.fillStyle = '#15151f';
    ctx.fillRect(offsetPx, -thickPx / 2 - 1, widthPx, thickPx + 2);

    ctx.strokeStyle = isSelected ? '#89b4fa' : (isHovered ? '#77aaee' : '#5588cc');
    ctx.lineWidth = isSelected ? 2.5 : 2;

    const gap = thickPx * 0.15;
    ctx.beginPath();
    ctx.moveTo(offsetPx, -gap);
    ctx.lineTo(offsetPx + widthPx, -gap);
    ctx.moveTo(offsetPx, gap);
    ctx.lineTo(offsetPx + widthPx, gap);
    ctx.stroke();

    ctx.strokeStyle = isSelected ? 'rgba(137,180,250,0.3)' : 'rgba(85,136,204,0.3)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(offsetPx + widthPx / 2, -thickPx / 2);
    ctx.lineTo(offsetPx + widthPx / 2, thickPx / 2);
    ctx.stroke();

    if (cam.zoom > 25) {
      ctx.fillStyle = 'rgba(120,170,220,0.7)';
      ctx.font = '9px monospace';
      ctx.textAlign = 'center';
      ctx.fillText(elem.id, offsetPx + widthPx / 2, thickPx / 2 + 12);
    }
  }

  ctx.restore();
}

function drawColumn(ctx: CanvasRenderingContext2D, col: ColumnElement, state: DrawState, w: number, h: number): void {
  const cam = state.camera;
  const s = w2s(cam, col.x, col.y, w, h);
  const hw = (col.profileW / 2) * cam.zoom;
  const hd = (col.profileD / 2) * cam.zoom;

  const isHovered = state.hoveredId === col.id;
  const isSelected = state.selectedId === col.id;

  ctx.fillStyle = isHovered ? '#999' : '#888886';
  ctx.strokeStyle = isSelected ? '#89b4fa' : '#666';
  ctx.lineWidth = isSelected ? 2 : 1;

  ctx.fillRect(s.x - hw, s.y - hd, hw * 2, hd * 2);
  ctx.strokeRect(s.x - hw, s.y - hd, hw * 2, hd * 2);

  ctx.strokeStyle = 'rgba(255,255,255,0.3)';
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(s.x - hw, s.y - hd);
  ctx.lineTo(s.x + hw, s.y + hd);
  ctx.moveTo(s.x + hw, s.y - hd);
  ctx.lineTo(s.x - hw, s.y + hd);
  ctx.stroke();

  if (cam.zoom > 25) {
    ctx.fillStyle = 'rgba(180,180,180,0.6)';
    ctx.font = '9px monospace';
    ctx.textAlign = 'center';
    ctx.fillText(col.id, s.x, s.y + hd + 12);
  }
}

function drawFurniture(ctx: CanvasRenderingContext2D, furn: FurnitureElement, state: DrawState, w: number, h: number): void {
  const cam = state.camera;
  // furn.x/y is the bottom-left corner (consistent with 3D scene graph)
  const corner = w2s(cam, furn.x, furn.y, w, h);

  const wPx = furn.width * cam.zoom;
  const dPx = furn.depth * cam.zoom;
  const hw = wPx / 2;
  const hd = dPx / 2;

  // Rotate around center of the furniture.
  // Y is flipped (world Y+ = up, screen Y+ = down), so center is ABOVE the corner on screen.
  const cx = corner.x + hw;
  const cy = corner.y - hd;

  const isHovered = state.hoveredId === furn.id;
  const isSelected = state.selectedId === furn.id;

  ctx.save();
  ctx.translate(cx, cy);
  ctx.rotate((furn.rotation * Math.PI) / 180);

  ctx.fillStyle = isSelected ? 'rgba(137, 180, 250, 0.25)' : (isHovered ? 'rgba(200, 180, 150, 0.35)' : 'rgba(160, 150, 120, 0.25)');
  ctx.strokeStyle = isSelected ? '#89b4fa' : (isHovered ? '#c8a060' : '#b09870');
  ctx.lineWidth = isSelected ? 2 : 1.5;
  ctx.setLineDash([5, 3]);

  ctx.fillRect(-hw, -hd, wPx, dPx);
  ctx.strokeRect(-hw, -hd, wPx, dPx);

  ctx.setLineDash([]);

  // Front indicator at top edge
  ctx.beginPath();
  ctx.moveTo(0, -hd);
  ctx.lineTo(0, -hd + 5);
  ctx.stroke();

  if (cam.zoom > 20) {
    ctx.fillStyle = isSelected ? 'rgba(137, 180, 250, 0.9)' : 'rgba(200, 180, 150, 0.7)';
    ctx.font = '9px sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(furn.asset, 0, 0);
  }

  ctx.restore();
}

function drawSlab(ctx: CanvasRenderingContext2D, slab: SlabElement, state: DrawState, w: number, h: number): void {
  if (slab.boundary.length < 3) return;
  const cam = state.camera;

  const isHovered = state.hoveredId === slab.id;
  const isSelected = state.selectedId === slab.id;

  ctx.fillStyle = isHovered ? 'rgba(220,220,215,0.15)' : 'rgba(208,208,206,0.1)';
  ctx.strokeStyle = isSelected ? '#89b4fa' : 'rgba(208,208,206,0.4)';
  ctx.lineWidth = isSelected ? 2 : 1;
  ctx.setLineDash(isSelected ? [] : [4, 4]);

  ctx.beginPath();
  const first = w2s(cam, slab.boundary[0].x, slab.boundary[0].y, w, h);
  ctx.moveTo(first.x, first.y);
  for (let i = 1; i < slab.boundary.length; i++) {
    const p = w2s(cam, slab.boundary[i].x, slab.boundary[i].y, w, h);
    ctx.lineTo(p.x, p.y);
  }
  ctx.closePath();
  ctx.fill();
  ctx.stroke();
  ctx.setLineDash([]);

  if (cam.zoom > 15) {
    let cx = 0, cy = 0;
    for (const p of slab.boundary) { cx += p.x; cy += p.y; }
    cx /= slab.boundary.length;
    cy /= slab.boundary.length;
    const s = w2s(cam, cx, cy, w, h);
    ctx.fillStyle = 'rgba(200,200,200,0.5)';
    ctx.font = '10px monospace';
    ctx.textAlign = 'center';
    ctx.fillText(slab.id, s.x, s.y);
  }
}

function drawSpace(ctx: CanvasRenderingContext2D, space: DetectedSpace, state: DrawState, w: number, h: number): void {
  if (space.polygon.length < 3) return;
  const cam = state.camera;
  const isHovered = state.hoveredId === space.id;
  const isSelected = state.selectedId === space.id;

  ctx.fillStyle = isSelected ? 'rgba(137,180,250,0.15)' :
                  isHovered ? 'rgba(137,180,250,0.10)' :
                              'rgba(137,180,250,0.05)';
  ctx.beginPath();
  const first = w2s(cam, space.polygon[0].x, space.polygon[0].y, w, h);
  ctx.moveTo(first.x, first.y);
  for (let i = 1; i < space.polygon.length; i++) {
    const p = w2s(cam, space.polygon[i].x, space.polygon[i].y, w, h);
    ctx.lineTo(p.x, p.y);
  }
  ctx.closePath();
  ctx.fill();

  if (isSelected) {
    ctx.strokeStyle = '#89b4fa';
    ctx.lineWidth = 2;
    ctx.setLineDash([6, 4]);
    ctx.stroke();
    ctx.setLineDash([]);
  }

  if (cam.zoom > 10) {
    let cx = 0, cy = 0;
    for (const p of space.polygon) { cx += p.x; cy += p.y; }
    cx /= space.polygon.length;
    cy /= space.polygon.length;
    const s = w2s(cam, cx, cy, w, h);
    ctx.fillStyle = isSelected ? 'rgba(137,180,250,0.8)' : 'rgba(137,180,250,0.4)';
    ctx.font = '11px monospace';
    ctx.textAlign = 'center';
    ctx.fillText(space.name, s.x, s.y);
    if (cam.zoom > 20) {
      ctx.fillStyle = isSelected ? 'rgba(137,180,250,0.6)' : 'rgba(137,180,250,0.3)';
      ctx.font = '9px monospace';
      ctx.fillText(space.area.toFixed(1) + ' m\u00B2', s.x, s.y + 14);
    }
  }
}

function drawSelectionHighlight(ctx: CanvasRenderingContext2D, elem: FloorplanElement, cam: CanvasCamera, w: number, h: number): void {
  ctx.setLineDash([4, 4]);
  ctx.strokeStyle = '#89b4fa';
  ctx.lineWidth = 1;

  if (elem.type === 'Wall') {
    const wall = elem as WallElement;
    const f = w2s(cam, wall.from.x, wall.from.y, w, h);
    const t = w2s(cam, wall.to.x, wall.to.y, w, h);
    const pad = 8;
    const angle = Math.atan2(t.y - f.y, t.x - f.x);
    const nx = Math.sin(angle) * (wall.thickness * cam.zoom / 2 + pad);
    const ny = -Math.cos(angle) * (wall.thickness * cam.zoom / 2 + pad);
    ctx.beginPath();
    ctx.moveTo(f.x - nx, f.y - ny);
    ctx.lineTo(t.x - nx, t.y - ny);
    ctx.lineTo(t.x + nx, t.y + ny);
    ctx.lineTo(f.x + nx, f.y + ny);
    ctx.closePath();
    ctx.stroke();
  } else if (elem.type === 'Column') {
    const col = elem as ColumnElement;
    const s = w2s(cam, col.x, col.y, w, h);
    const hw = (col.profileW / 2) * cam.zoom + 6;
    const hd = (col.profileD / 2) * cam.zoom + 6;
    ctx.strokeRect(s.x - hw, s.y - hd, hw * 2, hd * 2);
  } else if (elem.type === 'Furniture') {
    const furn = elem as FurnitureElement;
    const corner = w2s(cam, furn.x, furn.y, w, h);
    const hw = (furn.width / 2) * cam.zoom + 6;
    const hd = (furn.depth / 2) * cam.zoom + 6;
    // Center on screen: X += hw, Y -= hd (Y is flipped)
    const cx = corner.x + (furn.width / 2) * cam.zoom;
    const cy = corner.y - (furn.depth / 2) * cam.zoom;
    ctx.save();
    ctx.translate(cx, cy);
    ctx.rotate((furn.rotation * Math.PI) / 180);
    ctx.strokeRect(-hw, -hd, hw * 2, hd * 2);
    ctx.restore();
  } else if (elem.type === 'Slab') {
    const slab = elem as SlabElement;
    if (slab.boundary.length >= 3) {
      ctx.beginPath();
      const first = w2s(cam, slab.boundary[0].x, slab.boundary[0].y, w, h);
      ctx.moveTo(first.x, first.y);
      for (let i = 1; i < slab.boundary.length; i++) {
        const p = w2s(cam, slab.boundary[i].x, slab.boundary[i].y, w, h);
        ctx.lineTo(p.x, p.y);
      }
      ctx.closePath();
      ctx.stroke();
    }
  }

  ctx.setLineDash([]);
}

function drawGhost(ctx: CanvasRenderingContext2D, state: DrawState, w: number, h: number): void {
  if (!state.ghostPos) return;
  const cam = state.camera;

  if (state.currentTool === 'wall' && state.wallStart) {
    const f = w2s(cam, state.wallStart.x, state.wallStart.y, w, h);
    const t = w2s(cam, state.ghostPos.x, state.ghostPos.y, w, h);
    ctx.strokeStyle = 'rgba(137,180,250,0.6)';
    ctx.lineWidth = 2;
    ctx.setLineDash([6, 4]);
    ctx.beginPath();
    ctx.moveTo(f.x, f.y);
    ctx.lineTo(t.x, t.y);
    ctx.stroke();
    ctx.setLineDash([]);

    const dx = state.ghostPos.x - state.wallStart.x;
    const dy = state.ghostPos.y - state.wallStart.y;
    const len = Math.sqrt(dx * dx + dy * dy);
    if (len > 0.01) {
      const mid = { x: (f.x + t.x) / 2, y: (f.y + t.y) / 2 };
      ctx.fillStyle = 'rgba(137,180,250,0.8)';
      ctx.font = '11px monospace';
      ctx.textAlign = 'center';
      ctx.fillText(len.toFixed(2) + 'm', mid.x, mid.y - 10);
    }

    ctx.fillStyle = '#89b4fa';
    ctx.beginPath();
    ctx.arc(f.x, f.y, 4, 0, Math.PI * 2);
    ctx.fill();
  }

  if (state.currentTool === 'slab' && state.slabPoints.length > 0) {
    ctx.strokeStyle = 'rgba(208,208,206,0.6)';
    ctx.lineWidth = 2;
    ctx.setLineDash([6, 4]);
    ctx.beginPath();
    const first = w2s(cam, state.slabPoints[0].x, state.slabPoints[0].y, w, h);
    ctx.moveTo(first.x, first.y);
    for (let i = 1; i < state.slabPoints.length; i++) {
      const p = w2s(cam, state.slabPoints[i].x, state.slabPoints[i].y, w, h);
      ctx.lineTo(p.x, p.y);
    }
    const g = w2s(cam, state.ghostPos.x, state.ghostPos.y, w, h);
    ctx.lineTo(g.x, g.y);
    ctx.stroke();
    ctx.setLineDash([]);

    for (const p of state.slabPoints) {
      const s = w2s(cam, p.x, p.y, w, h);
      ctx.fillStyle = 'rgba(208,208,206,0.8)';
      ctx.beginPath();
      ctx.arc(s.x, s.y, 4, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  // Crosshair
  if (state.currentTool !== 'select') {
    const g = w2s(cam, state.ghostPos.x, state.ghostPos.y, w, h);
    ctx.strokeStyle = 'rgba(255,255,255,0.3)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(g.x - 10, g.y);
    ctx.lineTo(g.x + 10, g.y);
    ctx.moveTo(g.x, g.y - 10);
    ctx.lineTo(g.x, g.y + 10);
    ctx.stroke();

    if (state.snapEnabled) {
      ctx.fillStyle = '#f5c211';
      ctx.beginPath();
      ctx.arc(g.x, g.y, 3, 0, Math.PI * 2);
      ctx.fill();
    }
  }
}
