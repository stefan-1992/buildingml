import { useCallback, useState } from 'react';
import { useFloorplanStore } from '@state/floorplan-store.ts';
import type { FloorplanElement, WallElement, DetectedSpace } from '@shared/floorplan/element-model.ts';
import { pointInPolygon } from '@shared/floorplan/space-detection.ts';

const colorMap: Record<string, string> = {
  Wall: '#c8c0b8', Door: '#a06020', Window: '#5588cc',
  Column: '#888886', Slab: '#d0d0ce', Beam: '#666',
  Space: 'rgba(137,180,250,0.5)',
};

const typeIcons: Record<string, string> = {
  Storey: '\u25a3', // filled square with inner square
  Space: '\u25a1',  // white square
  Wall: '\u2500',   // horizontal line
  Door: '\u25af',   // white rectangle
  Window: '\u25a1',  // white square
  Column: '\u253c',  // cross
  Slab: '\u25ad',    // horizontal rectangle
};

interface TreeNode {
  id: string;
  label: string;
  type: string;
  children: TreeNode[];
  depth: number;
  selectable: boolean;
}

/** Build tree hierarchy: Storey → Spaces → Elements, with doors/windows under walls. */
function buildTree(
  elements: FloorplanElement[],
  detectedSpaces: DetectedSpace[],
  storeyName: string,
): TreeNode {
  const assignedElements = new Set<string>();

  const storeyNode: TreeNode = {
    id: 'storey-0',
    label: storeyName,
    type: 'Storey',
    children: [],
    depth: 0,
    selectable: false,
  };

  // Build per-space subtrees
  for (const space of detectedSpaces) {
    const spaceNode: TreeNode = {
      id: space.id,
      label: space.name,
      type: 'Space',
      children: [],
      depth: 1,
      selectable: true,
    };

    // Walls belonging to this space
    for (const wallId of space.wallIds) {
      const wall = elements.find(e => e.id === wallId) as WallElement | undefined;
      if (!wall || assignedElements.has(wallId)) continue;
      assignedElements.add(wallId);
      const wallNode = buildWallNode(wall, elements, 2);
      spaceNode.children.push(wallNode);
    }

    // Columns inside this space polygon
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

  // Unassigned elements (not in any space)
  const unassigned: TreeNode[] = [];
  for (const el of elements) {
    if (assignedElements.has(el.id)) continue;
    if (el.type === 'Door' || el.type === 'Window') continue; // children of walls
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

function buildWallNode(wall: WallElement, elements: FloorplanElement[], depth: number): TreeNode {
  const wallNode: TreeNode = {
    id: wall.id, label: wall.id, type: 'Wall',
    children: [], depth, selectable: true,
  };
  // Add child doors/windows
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

export function ElementTree() {
  const elements = useFloorplanStore((s) => s.elements);
  const detectedSpaces = useFloorplanStore((s) => s.detectedSpaces);
  const selectedId = useFloorplanStore((s) => s.selectedId);
  const storeyName = useFloorplanStore((s) => s.storeyName);
  const setSelectedId = useFloorplanStore((s) => s.setSelectedId);

  const tree = buildTree(elements, detectedSpaces, storeyName);

  const handleDoubleClick = useCallback((id: string) => {
    const s = useFloorplanStore.getState();
    if (id.startsWith('space-')) {
      s.zoomToSpace(id);
    } else {
      s.zoomToElement(id);
    }
  }, []);

  return (
    <div style={{
      background: 'var(--surface)', display: 'flex', flexDirection: 'column', overflow: 'hidden',
    }}>
      <div style={{
        padding: '8px 12px', fontSize: 11, color: 'var(--text-dim)',
        borderBottom: '1px solid var(--border)',
        fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.5,
        display: 'flex', justifyContent: 'space-between', alignItems: 'center',
      }}>
        <span>Element Tree</span>
        <span style={{ fontWeight: 400, fontSize: 10, opacity: 0.7 }}>
          {detectedSpaces.length} spaces, {elements.length} elements
        </span>
      </div>
      <div style={{ flex: 1, overflowY: 'auto', padding: '4px 0' }}>
        <TreeNodeRow
          node={tree}
          selectedId={selectedId}
          onSelect={setSelectedId}
          onDoubleClick={handleDoubleClick}
        />
      </div>
    </div>
  );
}

function TreeNodeRow({
  node,
  selectedId,
  onSelect,
  onDoubleClick,
}: {
  node: TreeNode;
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  onDoubleClick: (id: string) => void;
}) {
  const [collapsed, setCollapsed] = useState(false);
  const hasChildren = node.children.length > 0;
  const isSelected = node.id === selectedId;
  const indent = node.depth * 16;

  return (
    <>
      <div
        onClick={() => {
          if (node.selectable) onSelect(node.id);
          if (hasChildren) setCollapsed(!collapsed);
        }}
        onDoubleClick={() => {
          if (node.selectable) onDoubleClick(node.id);
        }}
        style={{
          padding: '4px 8px 4px 0', fontSize: 12, cursor: 'pointer',
          display: 'flex', alignItems: 'center', gap: 4,
          paddingLeft: indent + 8,
          borderLeft: isSelected ? '3px solid var(--accent)' : '3px solid transparent',
          background: isSelected ? 'rgba(137,180,250,0.1)' : undefined,
          color: isSelected ? 'var(--accent)' : undefined,
          userSelect: 'none',
        }}
        onMouseEnter={(e) => {
          if (!isSelected) e.currentTarget.style.background = 'rgba(255,255,255,0.03)';
        }}
        onMouseLeave={(e) => {
          if (!isSelected) e.currentTarget.style.background = '';
        }}
      >
        {/* Expand/collapse toggle */}
        <span style={{
          width: 14, textAlign: 'center', fontSize: 10,
          color: 'var(--text-dim)', flexShrink: 0,
          opacity: hasChildren ? 1 : 0,
        }}>
          {hasChildren ? (collapsed ? '\u25b6' : '\u25bc') : ''}
        </span>
        {/* Type indicator */}
        <div style={{
          width: 14, height: 14, borderRadius: 2, flexShrink: 0,
          background: colorMap[node.type] || '#888',
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          fontSize: 10, lineHeight: 1, color: '#1e1e2e',
        }}>
          {typeIcons[node.type] || ''}
        </div>
        {/* Label */}
        <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {node.label}
        </span>
        {/* Type badge */}
        <span style={{ fontSize: 10, color: 'var(--text-dim)', flexShrink: 0 }}>
          {node.type}
        </span>
      </div>
      {/* Children */}
      {hasChildren && !collapsed && node.children.map(child => (
        <TreeNodeRow
          key={child.id}
          node={child}
          selectedId={selectedId}
          onSelect={onSelect}
          onDoubleClick={onDoubleClick}
        />
      ))}
    </>
  );
}
