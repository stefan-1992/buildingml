import { describe, it, expect } from 'vitest';
import type { FloorplanElement, WallElement, DoorElement, WindowElement, ColumnElement, SlabElement, DetectedSpace } from '../element-model.ts';

// Import the buildTree function — since it's not exported from the component,
// we'll extract the logic to a shared module for testing.
// For now, we test the tree-building logic inline.

function makeWall(id: string, from: [number, number], to: [number, number]): WallElement {
  return {
    type: 'Wall', id,
    from: { x: from[0], y: from[1] }, to: { x: to[0], y: to[1] },
    height: 2.8, thickness: 0.24, anchored: false, children: [],
  };
}

function makeDoor(id: string, parentWall: string): DoorElement {
  return { type: 'Door', id, parentWall, offset: 1.0, width: 0.9, height: 2.1 };
}

function makeWindow(id: string, parentWall: string): WindowElement {
  return { type: 'Window', id, parentWall, offset: 1.5, width: 1.2, height: 1.4, sill_height: 0.9 };
}

function makeColumn(id: string, x: number, y: number): ColumnElement {
  return { type: 'Column', id, x, y, height: 2.8, profileW: 0.3, profileD: 0.3 };
}

function makeSlab(id: string): SlabElement {
  return {
    type: 'Slab', id,
    boundary: [{ x: 0, y: 0 }, { x: 6, y: 0 }, { x: 6, y: 5 }, { x: 0, y: 5 }],
    thickness: 0.22,
  };
}

interface TreeNode {
  id: string;
  label: string;
  type: string;
  children: TreeNode[];
  depth: number;
  selectable: boolean;
}

// Replicate the buildTree logic for testing (same as in ElementTree.tsx)
function pointInPolygon(point: { x: number; y: number }, polygon: { x: number; y: number }[]): boolean {
  let inside = false;
  const n = polygon.length;
  for (let i = 0, j = n - 1; i < n; j = i++) {
    const xi = polygon[i].x, yi = polygon[i].y;
    const xj = polygon[j].x, yj = polygon[j].y;
    if ((yi > point.y) !== (yj > point.y) && point.x < (xj - xi) * (point.y - yi) / (yj - yi) + xi) {
      inside = !inside;
    }
  }
  return inside;
}

function buildWallNode(wall: WallElement, elements: FloorplanElement[], depth: number): TreeNode {
  const wallNode: TreeNode = {
    id: wall.id, label: wall.id, type: 'Wall',
    children: [], depth, selectable: true,
  };
  const children = elements.filter(e =>
    (e.type === 'Door' || e.type === 'Window') && 'parentWall' in e && e.parentWall === wall.id
  );
  for (const child of children) {
    wallNode.children.push({
      id: child.id, label: child.id, type: child.type,
      children: [], depth: depth + 1, selectable: true,
    });
  }
  return wallNode;
}

function buildTree(
  elements: FloorplanElement[],
  detectedSpaces: DetectedSpace[],
  storeyName: string,
): TreeNode {
  const assignedElements = new Set<string>();
  const storeyNode: TreeNode = {
    id: 'storey-0', label: storeyName, type: 'Storey',
    children: [], depth: 0, selectable: false,
  };

  for (const space of detectedSpaces) {
    const spaceNode: TreeNode = {
      id: space.id, label: space.name, type: 'Space',
      children: [], depth: 1, selectable: true,
    };
    for (const wallId of space.wallIds) {
      const wall = elements.find(e => e.id === wallId) as WallElement | undefined;
      if (!wall || assignedElements.has(wallId)) continue;
      assignedElements.add(wallId);
      spaceNode.children.push(buildWallNode(wall, elements, 2));
    }
    for (const el of elements) {
      if (el.type === 'Column' && !assignedElements.has(el.id)) {
        if (pointInPolygon({ x: el.x, y: el.y }, space.polygon)) {
          assignedElements.add(el.id);
          spaceNode.children.push({
            id: el.id, label: el.id, type: 'Column',
            children: [], depth: 2, selectable: true,
          });
        }
      }
    }
    storeyNode.children.push(spaceNode);
  }

  const unassigned: TreeNode[] = [];
  for (const el of elements) {
    if (assignedElements.has(el.id)) continue;
    if (el.type === 'Door' || el.type === 'Window') continue;
    assignedElements.add(el.id);
    if (el.type === 'Wall') {
      unassigned.push(buildWallNode(el as WallElement, elements, 1));
    } else {
      unassigned.push({
        id: el.id, label: el.id, type: el.type,
        children: [], depth: 1, selectable: true,
      });
    }
  }
  if (unassigned.length > 0) {
    storeyNode.children.push(...unassigned);
  }
  return storeyNode;
}

function flattenIds(node: TreeNode): string[] {
  const ids = [node.id];
  for (const child of node.children) {
    ids.push(...flattenIds(child));
  }
  return ids;
}

describe('buildTree', () => {
  it('creates a storey root node', () => {
    const tree = buildTree([], [], 'Ground Floor');
    expect(tree.type).toBe('Storey');
    expect(tree.label).toBe('Ground Floor');
    expect(tree.selectable).toBe(false);
    expect(tree.children).toHaveLength(0);
  });

  it('groups walls under their detected space', () => {
    const elements: FloorplanElement[] = [
      makeWall('W-01', [0, 0], [6, 0]),
      makeWall('W-02', [6, 0], [6, 5]),
      makeWall('W-03', [6, 5], [0, 5]),
      makeWall('W-04', [0, 5], [0, 0]),
    ];
    const spaces: DetectedSpace[] = [{
      id: 'space-0', name: 'Living Room', area: 30,
      polygon: [{ x: 0, y: 0 }, { x: 6, y: 0 }, { x: 6, y: 5 }, { x: 0, y: 5 }],
      wallIds: ['W-01', 'W-02', 'W-03', 'W-04'],
    }];

    const tree = buildTree(elements, spaces, 'Ground Floor');
    expect(tree.children).toHaveLength(1); // one space
    const spaceNode = tree.children[0];
    expect(spaceNode.type).toBe('Space');
    expect(spaceNode.label).toBe('Living Room');
    expect(spaceNode.children).toHaveLength(4); // four walls
    expect(spaceNode.children.every(c => c.type === 'Wall')).toBe(true);
  });

  it('nests doors and windows under their parent walls', () => {
    const elements: FloorplanElement[] = [
      makeWall('W-01', [0, 0], [6, 0]),
      makeDoor('D-01', 'W-01'),
      makeWindow('WIN-01', 'W-01'),
    ];
    const spaces: DetectedSpace[] = [];

    const tree = buildTree(elements, spaces, 'Ground Floor');
    // W-01 should be a direct child since there are no spaces
    const wallNode = tree.children.find(c => c.id === 'W-01')!;
    expect(wallNode).toBeDefined();
    expect(wallNode.children).toHaveLength(2);
    expect(wallNode.children[0].id).toBe('D-01');
    expect(wallNode.children[0].type).toBe('Door');
    expect(wallNode.children[1].id).toBe('WIN-01');
    expect(wallNode.children[1].type).toBe('Window');
  });

  it('places columns inside spaces', () => {
    const elements: FloorplanElement[] = [
      makeWall('W-01', [0, 0], [6, 0]),
      makeWall('W-02', [6, 0], [6, 5]),
      makeWall('W-03', [6, 5], [0, 5]),
      makeWall('W-04', [0, 5], [0, 0]),
      makeColumn('C-01', 3, 2.5),
    ];
    const spaces: DetectedSpace[] = [{
      id: 'space-0', name: 'Room', area: 30,
      polygon: [{ x: 0, y: 0 }, { x: 6, y: 0 }, { x: 6, y: 5 }, { x: 0, y: 5 }],
      wallIds: ['W-01', 'W-02', 'W-03', 'W-04'],
    }];

    const tree = buildTree(elements, spaces, 'Ground Floor');
    const spaceNode = tree.children[0];
    const colNode = spaceNode.children.find(c => c.type === 'Column');
    expect(colNode).toBeDefined();
    expect(colNode!.id).toBe('C-01');
  });

  it('puts unassigned elements at the storey level', () => {
    const elements: FloorplanElement[] = [
      makeWall('W-01', [0, 0], [6, 0]),
      makeSlab('SL-01'),
    ];
    const spaces: DetectedSpace[] = [];

    const tree = buildTree(elements, spaces, 'Ground Floor');
    expect(tree.children).toHaveLength(2); // wall + slab
    expect(tree.children[0].type).toBe('Wall');
    expect(tree.children[1].type).toBe('Slab');
  });

  it('handles multiple spaces with shared walls', () => {
    const elements: FloorplanElement[] = [
      makeWall('W-01', [0, 0], [6, 0]),
      makeWall('W-02', [6, 0], [6, 5]),
      makeWall('W-03', [6, 5], [0, 5]),
      makeWall('W-04', [0, 5], [0, 0]),
      makeWall('W-05', [6, 0], [11, 0]),
      makeWall('W-06', [11, 0], [11, 5]),
      makeWall('W-07', [11, 5], [6, 5]),
    ];
    const spaces: DetectedSpace[] = [
      {
        id: 'space-0', name: 'Kitchen', area: 30,
        polygon: [{ x: 0, y: 0 }, { x: 6, y: 0 }, { x: 6, y: 5 }, { x: 0, y: 5 }],
        wallIds: ['W-01', 'W-02', 'W-03', 'W-04'],
      },
      {
        id: 'space-1', name: 'Bedroom', area: 25,
        polygon: [{ x: 6, y: 0 }, { x: 11, y: 0 }, { x: 11, y: 5 }, { x: 6, y: 5 }],
        wallIds: ['W-02', 'W-05', 'W-06', 'W-07'],
      },
    ];

    const tree = buildTree(elements, spaces, 'Ground Floor');
    expect(tree.children).toHaveLength(2); // two spaces
    // W-02 is shared, assigned to first space only
    const kitchenWalls = tree.children[0].children.map(c => c.id);
    const bedroomWalls = tree.children[1].children.map(c => c.id);
    expect(kitchenWalls).toContain('W-02');
    expect(bedroomWalls).not.toContain('W-02'); // assigned once only
  });

  it('includes all elements exactly once in the tree', () => {
    const elements: FloorplanElement[] = [
      makeWall('W-01', [0, 0], [6, 0]),
      makeWall('W-02', [6, 0], [6, 5]),
      makeDoor('D-01', 'W-01'),
      makeWindow('WIN-01', 'W-02'),
      makeColumn('C-01', 3, 2.5),
      makeSlab('SL-01'),
    ];
    const spaces: DetectedSpace[] = [];

    const tree = buildTree(elements, spaces, 'Ground Floor');
    const allIds = flattenIds(tree).filter(id => id !== 'storey-0');
    // All element IDs should appear exactly once
    for (const el of elements) {
      expect(allIds.filter(id => id === el.id)).toHaveLength(1);
    }
  });

  it('sets correct depth levels', () => {
    const elements: FloorplanElement[] = [
      makeWall('W-01', [0, 0], [6, 0]),
      makeDoor('D-01', 'W-01'),
    ];
    const spaces: DetectedSpace[] = [{
      id: 'space-0', name: 'Room', area: 30,
      polygon: [{ x: 0, y: 0 }, { x: 6, y: 0 }, { x: 6, y: 5 }, { x: 0, y: 5 }],
      wallIds: ['W-01'],
    }];

    const tree = buildTree(elements, spaces, 'Ground Floor');
    expect(tree.depth).toBe(0);                        // storey
    expect(tree.children[0].depth).toBe(1);            // space
    expect(tree.children[0].children[0].depth).toBe(2); // wall
    expect(tree.children[0].children[0].children[0].depth).toBe(3); // door
  });
});
