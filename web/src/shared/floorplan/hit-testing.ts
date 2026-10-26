/** Hit testing for floorplan elements. */

import type { FloorplanElement, WallElement, ColumnElement, DoorElement, WindowElement, SlabElement, FurnitureElement, DetectedSpace, Point2D } from './element-model.ts';
import { wallAngle } from './element-model.ts';
import { pointInPolygon } from './space-detection.ts';

/** Hit test all elements and spaces. Returns the ID of the hit element/space, or null. */
export function hitTest(
  worldPt: Point2D,
  elements: FloorplanElement[],
  detectedSpaces: DetectedSpace[],
): string | null {
  // Priority order: Openings > Columns > Walls > Spaces > Slabs

  for (let i = elements.length - 1; i >= 0; i--) {
    const e = elements[i];
    if ((e.type === 'Door' || e.type === 'Window') && hitTestOpening(e as DoorElement | WindowElement, worldPt, elements)) return e.id;
  }
  for (let i = elements.length - 1; i >= 0; i--) {
    const e = elements[i];
    if (e.type === 'Column' && hitTestColumn(e as ColumnElement, worldPt)) return e.id;
  }
  for (let i = elements.length - 1; i >= 0; i--) {
    const e = elements[i];
    if (e.type === 'Furniture' && hitTestFurniture(e as FurnitureElement, worldPt)) return e.id;
  }
  for (let i = elements.length - 1; i >= 0; i--) {
    const e = elements[i];
    if (e.type === 'Wall' && hitTestWall(e as WallElement, worldPt)) return e.id;
  }
  for (const space of detectedSpaces) {
    if (pointInPolygon(worldPt, space.polygon)) return space.id;
  }
  for (let i = elements.length - 1; i >= 0; i--) {
    const e = elements[i];
    if (e.type === 'Slab' && hitTestSlab(e as SlabElement, worldPt)) return e.id;
  }
  return null;
}

function hitTestWall(wall: WallElement, p: Point2D): boolean {
  const fx = wall.from.x, fy = wall.from.y;
  const tx = wall.to.x, ty = wall.to.y;
  const dx = tx - fx, dy = ty - fy;
  const len2 = dx * dx + dy * dy;
  if (len2 < 0.0001) return false;
  let t = ((p.x - fx) * dx + (p.y - fy) * dy) / len2;
  t = Math.max(0, Math.min(1, t));
  const closestX = fx + t * dx;
  const closestY = fy + t * dy;
  const dist = Math.sqrt((p.x - closestX) ** 2 + (p.y - closestY) ** 2);
  return dist < wall.thickness / 2 + 0.1;
}

function hitTestColumn(col: ColumnElement, p: Point2D): boolean {
  const hw = col.profileW / 2 + 0.05;
  const hd = col.profileD / 2 + 0.05;
  return Math.abs(p.x - col.x) < hw && Math.abs(p.y - col.y) < hd;
}

function hitTestFurniture(furn: FurnitureElement, p: Point2D): boolean {
  // furn.x/y is bottom-left corner; rotate test around center
  const cx = furn.x + furn.width / 2;
  const cy = furn.y + furn.depth / 2;
  const hw = furn.width / 2 + 0.05;
  const hd = furn.depth / 2 + 0.05;
  const angle = (furn.rotation * Math.PI) / 180;
  const cos = Math.cos(-angle), sin = Math.sin(-angle);
  const dx = p.x - cx;
  const dy = p.y - cy;
  const lx = dx * cos - dy * sin;
  const ly = dx * sin + dy * cos;
  return Math.abs(lx) < hw && Math.abs(ly) < hd;
}

function hitTestOpening(elem: DoorElement | WindowElement, p: Point2D, elements: FloorplanElement[]): boolean {
  const wall = elements.find(e => e.id === elem.parentWall) as WallElement | undefined;
  if (!wall) return false;

  const fx = wall.from.x, fy = wall.from.y;
  const angle = wallAngle(wall);
  const cos = Math.cos(-angle), sin = Math.sin(-angle);
  const lx = (p.x - fx) * cos - (p.y - fy) * sin;
  const ly = (p.x - fx) * sin + (p.y - fy) * cos;

  return lx >= elem.offset - 0.1 && lx <= elem.offset + elem.width + 0.1 &&
         Math.abs(ly) < wall.thickness / 2 + 0.15;
}

function hitTestSlab(slab: SlabElement, p: Point2D): boolean {
  if (slab.boundary.length < 3) return false;
  return pointInPolygon(p, slab.boundary);
}

/** Find closest wall and offset along it. */
export function findWallAtPoint(
  worldPt: Point2D,
  elements: FloorplanElement[],
): { wall: WallElement; offset: number } | null {
  let bestWall: WallElement | null = null;
  let bestDist = Infinity;
  let bestOffset = 0;

  for (const e of elements) {
    if (e.type !== 'Wall') continue;
    const wall = e as WallElement;
    const fx = wall.from.x, fy = wall.from.y;
    const tx = wall.to.x, ty = wall.to.y;
    const dx = tx - fx, dy = ty - fy;
    const len2 = dx * dx + dy * dy;
    if (len2 < 0.001) continue;
    let t = ((worldPt.x - fx) * dx + (worldPt.y - fy) * dy) / len2;
    t = Math.max(0, Math.min(1, t));
    const cx = fx + t * dx, cy = fy + t * dy;
    const dist = Math.sqrt((worldPt.x - cx) ** 2 + (worldPt.y - cy) ** 2);
    if (dist < bestDist) {
      bestDist = dist;
      bestWall = wall;
      bestOffset = t * Math.sqrt(len2);
    }
  }

  if (bestDist > 0.5 || !bestWall) return null;
  return { wall: bestWall, offset: bestOffset };
}
