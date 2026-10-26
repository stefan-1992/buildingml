/** Space detection using planar face enumeration from wall topology. */

import type { FloorplanElement, DetectedSpace, Point2D, WallElement } from './element-model.ts';

const CONNECT_TOL = 0.05; // meters

export function pointInPolygon(p: Point2D, polygon: Point2D[]): boolean {
  let inside = false;
  for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
    const xi = polygon[i].x, yi = polygon[i].y;
    const xj = polygon[j].x, yj = polygon[j].y;
    if (((yi > p.y) !== (yj > p.y)) && (p.x < (xj - xi) * (p.y - yi) / (yj - yi) + xi)) {
      inside = !inside;
    }
  }
  return inside;
}

export function detectSpaces(
  elements: FloorplanElement[],
  previousSpaces: DetectedSpace[],
): DetectedSpace[] {
  const walls = elements.filter((e): e is WallElement => e.type === 'Wall');
  if (walls.length < 3) return [];

  // Step 1: Cluster endpoints into unique vertices
  const verts: Point2D[] = [];
  const vmap = new Map<string, number>();

  function vkey(x: number, y: number): string {
    const rx = Math.round(x / CONNECT_TOL) * CONNECT_TOL;
    const ry = Math.round(y / CONNECT_TOL) * CONNECT_TOL;
    return rx.toFixed(3) + ',' + ry.toFixed(3);
  }

  function getVert(x: number, y: number): number {
    const k = vkey(x, y);
    if (vmap.has(k)) return vmap.get(k)!;
    const i = verts.length;
    verts.push({ x, y });
    vmap.set(k, i);
    return i;
  }

  // Step 2: Build directed half-edges
  interface HalfEdge {
    from: number;
    to: number;
    wallId: string;
  }

  const hedges: HalfEdge[] = [];
  for (const w of walls) {
    const fi = getVert(w.from.x, w.from.y);
    const ti = getVert(w.to.x, w.to.y);
    if (fi === ti) continue;
    hedges.push({ from: fi, to: ti, wallId: w.id });
    hedges.push({ from: ti, to: fi, wallId: w.id });
  }

  // Step 3: Build adjacency with sorted outgoing edges at each vertex
  interface AdjEntry {
    to: number;
    wallId: string;
    idx: number;
    angle: number;
  }

  const adj = new Map<number, AdjEntry[]>();
  for (let i = 0; i < hedges.length; i++) {
    const h = hedges[i];
    if (!adj.has(h.from)) adj.set(h.from, []);
    adj.get(h.from)!.push({
      to: h.to,
      wallId: h.wallId,
      idx: i,
      angle: Math.atan2(verts[h.to].y - verts[h.from].y, verts[h.to].x - verts[h.from].x),
    });
  }
  for (const [, list] of adj) {
    list.sort((a, b) => a.angle - b.angle);
  }

  // Step 4: For each half-edge u→v, find the next half-edge in face traversal.
  const nxt = new Array<number>(hedges.length).fill(-1);
  for (let i = 0; i < hedges.length; i++) {
    const h = hedges[i];
    const outgoing = adj.get(h.to);
    if (!outgoing || outgoing.length === 0) continue;

    // Find twin (v→u) in outgoing edges of v, prefer same wallId
    let twinIdx = -1;
    for (let j = 0; j < outgoing.length; j++) {
      if (outgoing[j].to === h.from && outgoing[j].wallId === h.wallId) {
        twinIdx = j;
        break;
      }
    }
    if (twinIdx < 0) {
      for (let j = 0; j < outgoing.length; j++) {
        if (outgoing[j].to === h.from) {
          twinIdx = j;
          break;
        }
      }
    }
    if (twinIdx >= 0) {
      const prevIdx = (twinIdx - 1 + outgoing.length) % outgoing.length;
      nxt[i] = outgoing[prevIdx].idx;
    }
  }

  // Step 5: Trace face cycles
  const visited = new Set<number>();
  const faces: { polygon: Point2D[]; wallIds: string[]; area: number }[] = [];

  for (let i = 0; i < hedges.length; i++) {
    if (visited.has(i) || nxt[i] < 0) continue;
    const cycle: number[] = [];
    let cur = i;
    let safe = 0;
    while (!visited.has(cur) && nxt[cur] >= 0 && safe < 500) {
      visited.add(cur);
      cycle.push(cur);
      cur = nxt[cur];
      safe++;
    }
    if (cycle.length >= 3 && cur === i) {
      const poly = cycle.map(ci => ({ ...verts[hedges[ci].from] }));
      const wids = [...new Set(cycle.map(ci => hedges[ci].wallId))];
      // Signed area: positive = CCW = interior face
      let area = 0;
      for (let j = 0; j < poly.length; j++) {
        const p1 = poly[j], p2 = poly[(j + 1) % poly.length];
        area += p1.x * p2.y - p2.x * p1.y;
      }
      area /= 2;
      if (area > 0.1) {
        faces.push({ polygon: poly, wallIds: wids, area });
      }
    }
  }

  faces.sort((a, b) => a.area - b.area);

  // Preserve user-given names across re-detection by matching wall ID sets
  const oldNames = new Map<string, string>();
  for (const s of previousSpaces) {
    oldNames.set(s.wallIds.slice().sort().join(','), s.name);
  }

  return faces.map((f, i) => {
    const key = f.wallIds.slice().sort().join(',');
    return {
      id: 'space-' + i,
      name: oldNames.get(key) || 'Space ' + (i + 1),
      polygon: f.polygon,
      wallIds: f.wallIds,
      area: f.area,
    };
  });
}

/** Classify walls of a space as shared (in 2+ spaces) or exclusive. */
export function identifySharedWalls(
  spaceId: string,
  detectedSpaces: DetectedSpace[],
  elements: FloorplanElement[],
): { shared: string[]; exclusive: string[] } {
  const idx = parseInt(spaceId.split('-')[1]);
  const space = detectedSpaces[idx];
  if (!space) return { shared: [], exclusive: [] };

  const shared: string[] = [];
  const exclusive: string[] = [];
  for (const wallId of space.wallIds) {
    const isShared = detectedSpaces.some((s, si) => si !== idx && s.wallIds.includes(wallId));
    const wall = elements.find(e => e.id === wallId) as WallElement | undefined;
    const isAnchored = wall?.anchored ?? false;
    if (isShared || isAnchored) {
      shared.push(wallId);
    } else {
      exclusive.push(wallId);
    }
  }
  return { shared, exclusive };
}

/** Check if a space has any anchored walls. */
export function spaceHasAnchoredWalls(
  spaceId: string,
  detectedSpaces: DetectedSpace[],
  elements: FloorplanElement[],
): boolean {
  const idx = parseInt(spaceId.split('-')[1]);
  const space = detectedSpaces[idx];
  if (!space) return false;
  return space.wallIds.some(wid => {
    const w = elements.find(e => e.id === wid) as WallElement | undefined;
    return w?.anchored ?? false;
  });
}

/** Find wall endpoints connected to a given wall (for anchored wall drag). */
export function findConnectedEndpoints(
  wall: WallElement,
  elements: FloorplanElement[],
): { wallId: string; end: 'from' | 'to' }[] {
  const connections: { wallId: string; end: 'from' | 'to' }[] = [];
  for (const other of elements.filter((e): e is WallElement => e.type === 'Wall' && e.id !== wall.id)) {
    if (Math.abs(other.from.x - wall.from.x) < CONNECT_TOL && Math.abs(other.from.y - wall.from.y) < CONNECT_TOL) {
      connections.push({ wallId: other.id, end: 'from' });
    }
    if (Math.abs(other.to.x - wall.from.x) < CONNECT_TOL && Math.abs(other.to.y - wall.from.y) < CONNECT_TOL) {
      connections.push({ wallId: other.id, end: 'to' });
    }
    if (Math.abs(other.from.x - wall.to.x) < CONNECT_TOL && Math.abs(other.from.y - wall.to.y) < CONNECT_TOL) {
      connections.push({ wallId: other.id, end: 'from' });
    }
    if (Math.abs(other.to.x - wall.to.x) < CONNECT_TOL && Math.abs(other.to.y - wall.to.y) < CONNECT_TOL) {
      connections.push({ wallId: other.id, end: 'to' });
    }
  }
  return connections;
}

export { CONNECT_TOL };
