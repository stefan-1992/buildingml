/** Floorplan element types and factory functions. */

export interface Point2D {
  x: number;
  y: number;
}

export interface WallElement {
  type: 'Wall';
  id: string;
  from: Point2D;
  to: Point2D;
  height: number;
  thickness: number;
  anchored: boolean;
  children: string[]; // child element IDs (doors/windows)
}

export interface DoorElement {
  type: 'Door';
  id: string;
  parentWall: string;
  offset: number;
  width: number;
  height: number;
}

export interface WindowElement {
  type: 'Window';
  id: string;
  parentWall: string;
  offset: number;
  width: number;
  height: number;
  sill_height: number;
}

export interface ColumnElement {
  type: 'Column';
  id: string;
  x: number;
  y: number;
  height: number;
  profileW: number;
  profileD: number;
}

export interface SlabElement {
  type: 'Slab';
  id: string;
  boundary: Point2D[];
  thickness: number;
}

export interface FurnitureElement {
  type: 'Furniture';
  id: string;
  asset: string;
  x: number;
  y: number;
  rotation: number;
  width: number;
  depth: number;
}

export type FloorplanElement = WallElement | DoorElement | WindowElement | ColumnElement | SlabElement | FurnitureElement;

export interface DetectedSpace {
  id: string;
  name: string;
  polygon: Point2D[];
  wallIds: string[];
  area: number;
}

export interface IdCounters {
  Wall: number;
  Door: number;
  Window: number;
  Column: number;
  Slab: number;
  Beam: number;
  Furniture: number;
}

export function createIdCounters(): IdCounters {
  return { Wall: 1, Door: 1, Window: 1, Column: 1, Slab: 1, Beam: 1, Furniture: 1 };
}

export function genId(type: string, counters: IdCounters): string {
  const prefix = type.charAt(0).toUpperCase();
  const key = type as keyof IdCounters;
  const n = counters[key] || 1;
  counters[key] = n + 1;
  return `${prefix}-${String(n).padStart(2, '0')}`;
}

export function createWall(from: Point2D, to: Point2D, height: number, counters: IdCounters): WallElement {
  return {
    type: 'Wall',
    id: genId('Wall', counters),
    from: { x: from.x, y: from.y },
    to: { x: to.x, y: to.y },
    height,
    thickness: 0.24,
    anchored: false,
    children: [],
  };
}

export function createDoor(wallId: string, offset: number, counters: IdCounters): DoorElement {
  return {
    type: 'Door',
    id: genId('Door', counters),
    parentWall: wallId,
    offset,
    width: 0.9,
    height: 2.1,
  };
}

export function createWindow(wallId: string, offset: number, counters: IdCounters): WindowElement {
  return {
    type: 'Window',
    id: genId('Window', counters),
    parentWall: wallId,
    offset,
    width: 1.2,
    height: 1.4,
    sill_height: 0.9,
  };
}

export function createColumn(pos: Point2D, height: number, counters: IdCounters): ColumnElement {
  return {
    type: 'Column',
    id: genId('Column', counters),
    x: pos.x,
    y: pos.y,
    height,
    profileW: 0.3,
    profileD: 0.3,
  };
}

export function createSlab(points: Point2D[], counters: IdCounters): SlabElement {
  return {
    type: 'Slab',
    id: genId('Slab', counters),
    boundary: points.map(p => ({ x: p.x, y: p.y })),
    thickness: 0.22,
  };
}

export function createFurniture(pos: Point2D, asset: string, width: number, depth: number, counters: IdCounters): FurnitureElement {
  return {
    type: 'Furniture',
    id: genId('Furniture', counters),
    asset,
    x: pos.x,
    y: pos.y,
    rotation: 0,
    width,
    depth,
  };
}

export function wallLength(wall: WallElement): number {
  const dx = wall.to.x - wall.from.x;
  const dy = wall.to.y - wall.from.y;
  return Math.sqrt(dx * dx + dy * dy);
}

export function wallAngle(wall: WallElement): number {
  return Math.atan2(wall.to.y - wall.from.y, wall.to.x - wall.from.x);
}
