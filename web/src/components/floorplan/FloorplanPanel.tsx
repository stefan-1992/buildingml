import { useEffect, useRef, useCallback, useState } from 'react';
import { useFloorplanStore } from '@state/floorplan-store.ts';
import { drawAll, type DrawState } from '@shared/floorplan/drawing.ts';
import { hitTest, findWallAtPoint } from '@shared/floorplan/hit-testing.ts';
import { screenToWorld, snapPoint, applyZoom, snapToGrid } from '@shared/floorplan/canvas-camera.ts';
import { createWall, createDoor, createWindow, createColumn, createSlab, createFurniture } from '@shared/floorplan/element-model.ts';
import type { WallElement, ColumnElement, SlabElement, Point2D } from '@shared/floorplan/element-model.ts';
import type { SemanticModel } from '@shared/types.ts';

function getAssetDimensions(model: SemanticModel | null, assetId: string): { width: number; depth: number } {
  for (const lib of model?.asset_libraries || []) {
    const asset = lib.assets.find(a => a.id === assetId);
    if (asset) {
      let width = 1.0, depth = 1.0;
      for (const p of asset.properties) {
        if (p.key === 'width' && 'Number' in p.value) width = p.value.Number;
        if (p.key === 'depth' && 'Number' in p.value) depth = p.value.Number;
      }
      return { width, depth };
    }
  }
  return { width: 1.0, depth: 1.0 };
}
import { identifySharedWalls, findConnectedEndpoints, spaceHasAnchoredWalls, pointInPolygon, CONNECT_TOL } from '@shared/floorplan/space-detection.ts';
import { genId } from '@shared/floorplan/element-model.ts';
import { useIsMobile } from '@shared/use-is-mobile.ts';

interface ContextMenuState { x: number; y: number }

export function FloorplanPanel() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const store = useFloorplanStore;
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [mouseWorldPos, setMouseWorldPos] = useState<Point2D>({ x: 0, y: 0 });
  const altKeyRef = useRef(false);
  const [altHeld, setAltHeld] = useState(false);
  const isMobile = useIsMobile();

  // Touch state for mobile pan/pinch
  const lastTouchDistRef = useRef(0);
  const lastTouchCenterRef = useRef<[number, number]>([0, 0]);
  const touchCountRef = useRef(0);

  // Refs for space drag
  const spaceDragEndpointsRef = useRef<{ wallId: string; end: 'from' | 'to' }[]>([]);
  const spaceDragColumnsRef = useRef<string[]>([]);
  const spaceDragSlabVertsRef = useRef<{ slabId: string; vertexIdx: number }[]>([]);
  const isSpaceDragRef = useRef(false);
  const isPerpDragRef = useRef(false);
  const perpExclusiveEndpointsRef = useRef<{ wallId: string; end: 'from' | 'to' }[]>([]);
  const perpReplacementWallIdsRef = useRef<string[]>([]);

  // Ref for anchored wall drag
  const anchoredEndpointsRef = useRef<{ wallId: string; end: 'from' | 'to' }[]>([]);

  // Keyboard shortcuts
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;

      if (e.key === 'Alt') { altKeyRef.current = true; setAltHeld(true); }

      const s = store.getState();

      // Ctrl combos
      if (e.ctrlKey || e.metaKey) {
        if (e.key === 'z') { e.preventDefault(); s.undo(); return; }
        if (e.key === 'y') { e.preventDefault(); s.redo(); return; }
        if (e.key === 'd') { e.preventDefault(); s.duplicateSelected(); return; }
        if (e.key === 's') {
          e.preventDefault();
          s.saveBml();
          return;
        }
        return;
      }

      switch (e.key.toLowerCase()) {
        case 'v': s.setTool('select'); break;
        case 'w': s.setTool('wall'); break;
        case 'd': s.setTool('door'); break;
        case 'n': s.setTool('window'); break;
        case 'c': s.setTool('column'); break;
        case 's': s.setTool('slab'); break;
        case 'u': s.setTool('furniture'); break;
        case 'a': s.toggleAnchor(); break;
        case 'g': s.toggleGrid(); break;
        case 'x': s.toggleSnap(); break;
        case 'f': {
          const canvas = canvasRef.current;
          if (canvas) {
            const rect = canvas.getBoundingClientRect();
            s.zoomToFit(rect.width, rect.height);
          }
          break;
        }
        case 'escape':
          s.setWallStart(null);
          s.setSlabPoints([]);
          s.setSelectedId(null);
          s.setTool('select');
          setContextMenu(null);
          break;
        case 'delete':
        case 'backspace':
          e.preventDefault();
          s.deleteSelected();
          break;
        case 'arrowleft':
        case 'arrowright':
        case 'arrowup':
        case 'arrowdown': {
          if (!s.selectedId) break;
          e.preventDefault();
          const step = e.shiftKey ? 1.0 : (s.snapEnabled ? 0.25 : 0.1);
          let dx = 0, dy = 0;
          if (e.key === 'ArrowLeft') dx = -step;
          else if (e.key === 'ArrowRight') dx = step;
          else if (e.key === 'ArrowUp') dy = step;
          else if (e.key === 'ArrowDown') dy = -step;
          s.nudgeSelected(dx, dy);
          break;
        }
      }
    };

    const onKeyUp = (e: KeyboardEvent) => {
      if (e.key === 'Alt') { altKeyRef.current = false; setAltHeld(false); }
    };

    document.addEventListener('keydown', onKeyDown);
    document.addEventListener('keyup', onKeyUp);
    return () => { document.removeEventListener('keydown', onKeyDown); document.removeEventListener('keyup', onKeyUp); };
  }, [store]);

  // Beforeunload warning
  useEffect(() => {
    const handler = (e: BeforeUnloadEvent) => {
      if (store.getState().hasUnsavedChanges) {
        e.preventDefault();
      }
    };
    window.addEventListener('beforeunload', handler);
    return () => window.removeEventListener('beforeunload', handler);
  }, [store]);

  // Animation frame loop
  useEffect(() => {
    let frameId: number;
    const loop = () => {
      const canvas = canvasRef.current;
      if (!canvas) { frameId = requestAnimationFrame(loop); return; }
      const ctx = canvas.getContext('2d');
      if (!ctx) { frameId = requestAnimationFrame(loop); return; }

      const rect = canvas.parentElement?.getBoundingClientRect();
      if (rect) {
        const dpr = window.devicePixelRatio || 1;
        canvas.width = rect.width * dpr;
        canvas.height = rect.height * dpr;
        canvas.style.width = rect.width + 'px';
        canvas.style.height = rect.height + 'px';
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      }

      const s = store.getState();
      const cw = rect?.width ?? canvas.width;
      const ch = rect?.height ?? canvas.height;

      const drawState: DrawState = {
        elements: s.elements,
        detectedSpaces: s.detectedSpaces,
        selectedId: s.selectedId,
        hoveredId: s.hoveredId,
        currentTool: s.currentTool,
        showGrid: s.showGrid,
        snapEnabled: s.snapEnabled,
        wallStart: s.wallStart,
        slabPoints: s.slabPoints,
        ghostPos: s.ghostPos,
        camera: s.camera,
        perpDragMode: s.perpDragMode,
        perpDragPolygon: s.perpDragPolygon,
      };

      drawAll(ctx, drawState, cw, ch);
      frameId = requestAnimationFrame(loop);
    };
    frameId = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(frameId);
  }, [store]);

  // Mouse handlers
  const isPanningRef = useRef(false);
  const isDraggingRef = useRef(false);
  const dragStartRef = useRef<{ x: number; y: number } | null>(null);
  const dragStartWorldRef = useRef<{ x: number; y: number } | null>(null);
  const dragSavedRef = useRef(false);

  const getCanvasSize = useCallback((): [number, number] => {
    const canvas = canvasRef.current;
    if (!canvas) return [800, 600];
    const rect = canvas.getBoundingClientRect();
    return [rect.width, rect.height];
  }, []);

  // Helper: collect space drag endpoints
  const setupSpaceDrag = useCallback((spaceId: string) => {
    const s = store.getState();
    const idx = parseInt(spaceId.split('-')[1]);
    const space = s.detectedSpaces[idx];
    if (!space) return;

    const endpoints: { wallId: string; end: 'from' | 'to' }[] = [];
    const columns: string[] = [];
    const slabVerts: { slabId: string; vertexIdx: number }[] = [];

    for (const el of s.elements) {
      if (el.type === 'Wall') {
        const wall = el as WallElement;
        for (const v of space.polygon) {
          if (Math.abs(wall.from.x - v.x) < CONNECT_TOL && Math.abs(wall.from.y - v.y) < CONNECT_TOL) {
            if (!endpoints.some(ep => ep.wallId === wall.id && ep.end === 'from'))
              endpoints.push({ wallId: wall.id, end: 'from' });
          }
          if (Math.abs(wall.to.x - v.x) < CONNECT_TOL && Math.abs(wall.to.y - v.y) < CONNECT_TOL) {
            if (!endpoints.some(ep => ep.wallId === wall.id && ep.end === 'to'))
              endpoints.push({ wallId: wall.id, end: 'to' });
          }
        }
      } else if (el.type === 'Column') {
        const col = el as ColumnElement;
        if (pointInPolygon({ x: col.x, y: col.y }, space.polygon)) {
          columns.push(col.id);
        }
      } else if (el.type === 'Slab') {
        const slab = el as SlabElement;
        for (let vi = 0; vi < slab.boundary.length; vi++) {
          for (const sv of space.polygon) {
            if (Math.abs(slab.boundary[vi].x - sv.x) < CONNECT_TOL && Math.abs(slab.boundary[vi].y - sv.y) < CONNECT_TOL) {
              if (!slabVerts.some(sv2 => sv2.slabId === slab.id && sv2.vertexIdx === vi))
                slabVerts.push({ slabId: slab.id, vertexIdx: vi });
            }
          }
        }
      }
    }

    spaceDragEndpointsRef.current = endpoints;
    spaceDragColumnsRef.current = columns;
    spaceDragSlabVertsRef.current = slabVerts;
  }, [store]);

  // Helper: setup perpendicular (Alt) drag for space
  const setupPerpDrag = useCallback((spaceId: string) => {
    const s = store.getState();
    const idx = parseInt(spaceId.split('-')[1]);
    const space = s.detectedSpaces[idx];
    if (!space) return;

    const { shared, exclusive } = identifySharedWalls(spaceId, s.detectedSpaces, s.elements);

    // Collect exclusive endpoints on space boundary
    const exclusiveEndpoints: { wallId: string; end: 'from' | 'to' }[] = [];
    for (const wallId of exclusive) {
      const wall = s.elements.find(e => e.id === wallId) as WallElement | undefined;
      if (!wall) continue;
      for (const v of space.polygon) {
        if (Math.abs(wall.from.x - v.x) < CONNECT_TOL && Math.abs(wall.from.y - v.y) < CONNECT_TOL) {
          if (!exclusiveEndpoints.some(ep => ep.wallId === wall.id && ep.end === 'from'))
            exclusiveEndpoints.push({ wallId: wall.id, end: 'from' });
        }
        if (Math.abs(wall.to.x - v.x) < CONNECT_TOL && Math.abs(wall.to.y - v.y) < CONNECT_TOL) {
          if (!exclusiveEndpoints.some(ep => ep.wallId === wall.id && ep.end === 'to'))
            exclusiveEndpoints.push({ wallId: wall.id, end: 'to' });
        }
      }
    }

    // Clone shared walls — create replacement walls for each shared wall
    const newWallIds: string[] = [];
    for (const wallId of shared) {
      const wall = s.elements.find(e => e.id === wallId) as WallElement | undefined;
      if (!wall) continue;
      const clone: WallElement = JSON.parse(JSON.stringify(wall));
      clone.id = genId('Wall', s.idCounters);
      s.elements.push(clone);
      newWallIds.push(clone.id);

      // Add clone's endpoints that are on the space boundary to exclusive set
      for (const v of space.polygon) {
        if (Math.abs(clone.from.x - v.x) < CONNECT_TOL && Math.abs(clone.from.y - v.y) < CONNECT_TOL) {
          exclusiveEndpoints.push({ wallId: clone.id, end: 'from' });
        }
        if (Math.abs(clone.to.x - v.x) < CONNECT_TOL && Math.abs(clone.to.y - v.y) < CONNECT_TOL) {
          exclusiveEndpoints.push({ wallId: clone.id, end: 'to' });
        }
      }
    }

    // Also collect columns and slab verts inside
    const columns: string[] = [];
    const slabVerts: { slabId: string; vertexIdx: number }[] = [];
    for (const el of s.elements) {
      if (el.type === 'Column') {
        const col = el as ColumnElement;
        if (pointInPolygon({ x: col.x, y: col.y }, space.polygon)) columns.push(col.id);
      } else if (el.type === 'Slab') {
        const slab = el as SlabElement;
        for (let vi = 0; vi < slab.boundary.length; vi++) {
          for (const sv of space.polygon) {
            if (Math.abs(slab.boundary[vi].x - sv.x) < CONNECT_TOL && Math.abs(slab.boundary[vi].y - sv.y) < CONNECT_TOL) {
              slabVerts.push({ slabId: slab.id, vertexIdx: vi });
            }
          }
        }
      }
    }

    perpExclusiveEndpointsRef.current = exclusiveEndpoints;
    perpReplacementWallIdsRef.current = newWallIds;
    spaceDragColumnsRef.current = columns;
    spaceDragSlabVertsRef.current = slabVerts;

    // Set the polygon for visual feedback
    s.setPerpDragMode(true);
    s.setPerpDragPolygon([...space.polygon]);
  }, [store]);

  const handleMouseDown = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    setContextMenu(null);
    const s = store.getState();
    const canvas = canvasRef.current!;
    const rect = canvas.getBoundingClientRect();
    const sx = e.clientX - rect.left;
    const sy = e.clientY - rect.top;
    const [cw, ch] = getCanvasSize();
    const world = screenToWorld(s.camera, sx, sy, cw, ch);
    const snapped = snapPoint(world, s.snapEnabled);

    // Middle-click or shift+click panning
    if (e.button === 1 || (e.button === 0 && e.shiftKey)) {
      isPanningRef.current = true;
      dragStartRef.current = { x: e.clientX, y: e.clientY };
      e.preventDefault();
      return;
    }

    // Right-click context menu
    if (e.button === 2) {
      e.preventDefault();
      const id = hitTest(world, s.elements, s.detectedSpaces);
      if (id) {
        s.setSelectedId(id);
        s.refreshAll();
      }
      setContextMenu({ x: e.clientX, y: e.clientY });
      return;
    }

    if (e.button !== 0) return;

    if (s.currentTool === 'select') {
      const id = hitTest(world, s.elements, s.detectedSpaces);
      s.setSelectedId(id);
      if (id) {
        dragStartRef.current = { x: e.clientX, y: e.clientY };
        dragStartWorldRef.current = { ...world };
        dragSavedRef.current = false;
        isSpaceDragRef.current = false;
        isPerpDragRef.current = false;
      }
      s.refreshAll();
    } else if (s.currentTool === 'wall') {
      if (!s.wallStart) {
        s.setWallStart({ ...snapped });
      } else {
        if (Math.abs(snapped.x - s.wallStart.x) > 0.01 || Math.abs(snapped.y - s.wallStart.y) > 0.01) {
          s.saveState();
          const wall = createWall(s.wallStart, snapped, s.storeyHeight, s.idCounters);
          s.addElement(wall);
          s.setWallStart({ ...snapped });
        }
      }
    } else if (s.currentTool === 'door') {
      const hit = findWallAtPoint(world, s.elements);
      if (hit) {
        s.saveState();
        const door = createDoor(hit.wall.id, Math.max(0, snapToGrid(hit.offset - 0.45, s.snapEnabled)), s.idCounters);
        s.addElement(door);
      }
    } else if (s.currentTool === 'window') {
      const hit = findWallAtPoint(world, s.elements);
      if (hit) {
        s.saveState();
        const win = createWindow(hit.wall.id, Math.max(0, snapToGrid(hit.offset - 0.6, s.snapEnabled)), s.idCounters);
        s.addElement(win);
      }
    } else if (s.currentTool === 'column') {
      s.saveState();
      const col = createColumn(snapped, s.storeyHeight, s.idCounters);
      s.addElement(col);
    } else if (s.currentTool === 'slab') {
      if (s.slabPoints.length >= 3) {
        const d = Math.sqrt((snapped.x - s.slabPoints[0].x) ** 2 + (snapped.y - s.slabPoints[0].y) ** 2);
        if (d < 0.3) {
          s.saveState();
          const slab = createSlab(s.slabPoints, s.idCounters);
          s.addElement(slab);
          s.setSlabPoints([]);
          return;
        }
      }
      s.setSlabPoints([...s.slabPoints, { ...snapped }]);
    } else if (s.currentTool === 'furniture') {
      const assetName = s.selectedAsset || 'Furniture';
      const assetDims = getAssetDimensions(s.loadedModel, assetName);
      s.saveState();
      // Place with center at cursor: store corner = cursor - half dimensions
      const corner = { x: snapped.x - assetDims.width / 2, y: snapped.y - assetDims.depth / 2 };
      const furn = createFurniture(corner, assetName, assetDims.width, assetDims.depth, s.idCounters);
      s.addElement(furn);
    }
  }, [store, getCanvasSize, setupSpaceDrag, setupPerpDrag]);

  const handleMouseMove = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    const s = store.getState();
    const canvas = canvasRef.current!;
    const rect = canvas.getBoundingClientRect();
    const sx = e.clientX - rect.left;
    const sy = e.clientY - rect.top;
    const [cw, ch] = getCanvasSize();
    const world = screenToWorld(s.camera, sx, sy, cw, ch);
    const snapped = snapPoint(world, s.snapEnabled);

    s.setGhostPos(snapped);
    setMouseWorldPos(snapped);

    if (isPanningRef.current && dragStartRef.current) {
      const dx = (e.clientX - dragStartRef.current.x) / s.camera.zoom;
      const dy = -(e.clientY - dragStartRef.current.y) / s.camera.zoom;
      s.setCamera({ viewX: s.camera.viewX - dx, viewY: s.camera.viewY - dy });
      dragStartRef.current = { x: e.clientX, y: e.clientY };
      return;
    }

    // Start drag detection
    if (!isDraggingRef.current && s.selectedId && dragStartRef.current && dragStartWorldRef.current) {
      const dist = Math.sqrt((e.clientX - dragStartRef.current.x) ** 2 + (e.clientY - dragStartRef.current.y) ** 2);
      if (dist >= 3) {
        isDraggingRef.current = true;
        if (!dragSavedRef.current) {
          s.saveState();
          dragSavedRef.current = true;
        }

        // Setup space drag or anchored wall drag on first drag start
        if (s.selectedId.startsWith('space-')) {
          const usePerp = altKeyRef.current || spaceHasAnchoredWalls(s.selectedId, s.detectedSpaces, s.elements);
          if (usePerp) {
            isPerpDragRef.current = true;
            setupPerpDrag(s.selectedId);
          } else {
            isSpaceDragRef.current = true;
            setupSpaceDrag(s.selectedId);
          }
        } else {
          // For anchored walls, collect connected endpoints
          const sel = s.elements.find(el => el.id === s.selectedId);
          if (sel?.type === 'Wall' && (sel as WallElement).anchored) {
            anchoredEndpointsRef.current = findConnectedEndpoints(sel as WallElement, s.elements);
          } else {
            anchoredEndpointsRef.current = [];
          }
        }
      }
    }

    // Dragging
    if (isDraggingRef.current && s.selectedId && dragStartWorldRef.current) {
      const ssx = snapToGrid(dragStartWorldRef.current.x, s.snapEnabled);
      const ssy = snapToGrid(dragStartWorldRef.current.y, s.snapEnabled);
      const dx = snapped.x - ssx;
      const dy = snapped.y - ssy;
      if (Math.abs(dx) < 0.001 && Math.abs(dy) < 0.001) return;

      if (isPerpDragRef.current) {
        // Perp drag: move only exclusive endpoints + columns + slab verts
        for (const ep of perpExclusiveEndpointsRef.current) {
          const wall = s.elements.find(el => el.id === ep.wallId) as WallElement | undefined;
          if (wall) { wall[ep.end].x += dx; wall[ep.end].y += dy; }
        }
        for (const colId of spaceDragColumnsRef.current) {
          const col = s.elements.find(el => el.id === colId) as ColumnElement | undefined;
          if (col) { col.x += dx; col.y += dy; }
        }
        for (const sv of spaceDragSlabVertsRef.current) {
          const slab = s.elements.find(el => el.id === sv.slabId) as SlabElement | undefined;
          if (slab) { slab.boundary[sv.vertexIdx].x += dx; slab.boundary[sv.vertexIdx].y += dy; }
        }
        // Update perp polygon for visual feedback
        if (s.perpDragPolygon) {
          s.setPerpDragPolygon(s.perpDragPolygon.map(p => ({ x: p.x + dx, y: p.y + dy })));
        }
        dragStartWorldRef.current = { ...snapped };
        s.refreshAll();
      } else if (isSpaceDragRef.current) {
        // Space drag: move all collected endpoints, columns, slab verts
        for (const ep of spaceDragEndpointsRef.current) {
          const wall = s.elements.find(el => el.id === ep.wallId) as WallElement | undefined;
          if (wall) { wall[ep.end].x += dx; wall[ep.end].y += dy; }
        }
        for (const colId of spaceDragColumnsRef.current) {
          const col = s.elements.find(el => el.id === colId) as ColumnElement | undefined;
          if (col) { col.x += dx; col.y += dy; }
        }
        for (const sv of spaceDragSlabVertsRef.current) {
          const slab = s.elements.find(el => el.id === sv.slabId) as SlabElement | undefined;
          if (slab) { slab.boundary[sv.vertexIdx].x += dx; slab.boundary[sv.vertexIdx].y += dy; }
        }
        dragStartWorldRef.current = { ...snapped };
        s.refreshAll();
      } else {
        // Regular element drag
        const sel = s.elements.find(el => el.id === s.selectedId);
        if (sel) {
          if (sel.type === 'Wall') {
            sel.from.x += dx; sel.from.y += dy;
            sel.to.x += dx; sel.to.y += dy;
            // Move connected endpoints for anchored walls
            for (const ep of anchoredEndpointsRef.current) {
              const other = s.elements.find(el => el.id === ep.wallId) as WallElement | undefined;
              if (other) { other[ep.end].x += dx; other[ep.end].y += dy; }
            }
          } else if (sel.type === 'Column') {
            (sel as ColumnElement).x += dx;
            (sel as ColumnElement).y += dy;
          } else if (sel.type === 'Furniture') {
            (sel as any).x += dx;
            (sel as any).y += dy;
          } else if (sel.type === 'Slab') {
            for (const p of (sel as SlabElement).boundary) {
              p.x += dx; p.y += dy;
            }
          }
          dragStartWorldRef.current = { ...snapped };
          s.refreshAll();
        }
      }
    }

    // Hover + cursor management
    if (s.currentTool === 'select' && !isDraggingRef.current) {
      const id = hitTest(world, s.elements, s.detectedSpaces);
      if (id !== s.hoveredId) s.setHoveredId(id);

      // Cursor
      if (id?.startsWith('space-') && altKeyRef.current) {
        canvas.style.cursor = 'grab';
      } else if (id?.startsWith('space-')) {
        canvas.style.cursor = 'move';
      } else if (id) {
        canvas.style.cursor = 'pointer';
      } else {
        canvas.style.cursor = 'default';
      }
    } else if (s.currentTool !== 'select') {
      canvas.style.cursor = 'crosshair';
    } else if (isDraggingRef.current && isPerpDragRef.current) {
      canvas.style.cursor = 'grabbing';
    }
  }, [store, getCanvasSize, setupSpaceDrag, setupPerpDrag]);

  const handleMouseUp = useCallback(() => {
    const s = store.getState();
    if (isDraggingRef.current && s.selectedId) {
      if (isSpaceDragRef.current || isPerpDragRef.current) {
        // Re-detect spaces and try to keep selection on the moved space
        s.refreshAll();
        // Clear perp state
        if (isPerpDragRef.current) {
          s.setPerpDragMode(false);
          s.setPerpDragPolygon(null);
        }
      } else {
        s.refreshAll();
      }
    }
    isDraggingRef.current = false;
    isPanningRef.current = false;
    isSpaceDragRef.current = false;
    isPerpDragRef.current = false;
    dragStartRef.current = null;
    dragStartWorldRef.current = null;
    dragSavedRef.current = false;
    spaceDragEndpointsRef.current = [];
    spaceDragColumnsRef.current = [];
    spaceDragSlabVertsRef.current = [];
    perpExclusiveEndpointsRef.current = [];
    perpReplacementWallIdsRef.current = [];
    anchoredEndpointsRef.current = [];
  }, [store]);

  const handleWheel = useCallback((e: React.WheelEvent<HTMLCanvasElement>) => {
    e.preventDefault();
    const s = store.getState();
    const canvas = canvasRef.current!;
    const rect = canvas.getBoundingClientRect();
    const sx = e.clientX - rect.left;
    const sy = e.clientY - rect.top;
    const [cw, ch] = getCanvasSize();

    const factor = e.deltaY > 0 ? 0.9 : 1.1;
    const cam = { ...s.camera };
    applyZoom(cam, factor, sx, sy, cw, ch);
    s.setCamera(cam);
  }, [store, getCanvasSize]);

  const handleDblClick = useCallback(() => {
    const s = store.getState();
    if (s.currentTool === 'wall' && s.wallStart) {
      s.setWallStart(null);
    }
    if (s.currentTool === 'slab' && s.slabPoints.length >= 3) {
      s.saveState();
      const slab = createSlab(s.slabPoints, s.idCounters);
      s.addElement(slab);
      s.setSlabPoints([]);
    }
  }, [store]);

  // Context menu actions
  const ctxAction = useCallback((action: string) => {
    setContextMenu(null);
    const s = store.getState();
    switch (action) {
      case 'delete': s.deleteSelected(); break;
      case 'duplicate': s.duplicateSelected(); break;
      case 'anchor': s.toggleAnchor(); break;
      case 'front': s.bringToFront(); break;
      case 'back': s.sendToBack(); break;
    }
  }, [store]);

  // Touch handlers for mobile: pan + pinch zoom + tap-to-select + long-press context menu
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    let longPressTimer: ReturnType<typeof setTimeout> | null = null;
    let longPressFired = false;
    let touchStartPos: [number, number] = [0, 0];
    let touchMoved = false;

    const clearLongPress = () => {
      if (longPressTimer !== null) {
        clearTimeout(longPressTimer);
        longPressTimer = null;
      }
    };

    const handleTouchStart = (e: TouchEvent) => {
      e.preventDefault();
      setContextMenu(null);
      touchCountRef.current = e.touches.length;
      longPressFired = false;

      if (e.touches.length === 1) {
        const { clientX, clientY } = e.touches[0];
        touchStartPos = [clientX, clientY];
        touchMoved = false;
        lastTouchCenterRef.current = [clientX, clientY];

        clearLongPress();
        longPressTimer = setTimeout(() => {
          if (!touchMoved) {
            longPressFired = true;
            // Long press: select element + show context menu
            const s = store.getState();
            const rect = canvas.getBoundingClientRect();
            const sx = touchStartPos[0] - rect.left;
            const sy = touchStartPos[1] - rect.top;
            const [cw, ch] = getCanvasSize();
            const world = screenToWorld(s.camera, sx, sy, cw, ch);
            const id = hitTest(world, s.elements, s.detectedSpaces);
            if (id) {
              s.setSelectedId(id);
              s.refreshAll();
              setContextMenu({ x: clientX, y: clientY });
            }
          }
          longPressTimer = null;
        }, 500);
      } else if (e.touches.length === 2) {
        touchMoved = true; // suppress tap on multi-touch
        clearLongPress();
        const t0 = e.touches[0], t1 = e.touches[1];
        lastTouchDistRef.current = Math.hypot(t1.clientX - t0.clientX, t1.clientY - t0.clientY);
        lastTouchCenterRef.current = [(t0.clientX + t1.clientX) / 2, (t0.clientY + t1.clientY) / 2];
      }
    };

    const handleTouchMove = (e: TouchEvent) => {
      e.preventDefault();

      // Don't pan if long press just fired (prevents jump)
      if (longPressFired) return;

      const s = store.getState();
      const [cw, ch] = getCanvasSize();

      if (e.touches.length === 1 && touchCountRef.current === 1) {
        // Check if finger moved beyond threshold
        if (!touchMoved) {
          const dx = e.touches[0].clientX - touchStartPos[0];
          const dy = e.touches[0].clientY - touchStartPos[1];
          if (Math.abs(dx) > 10 || Math.abs(dy) > 10) {
            touchMoved = true;
            clearLongPress();
            lastTouchCenterRef.current = [e.touches[0].clientX, e.touches[0].clientY];
          }
          return; // Don't pan yet while deciding
        }
        // Single finger pan
        const cx = e.touches[0].clientX, cy = e.touches[0].clientY;
        const dx = (cx - lastTouchCenterRef.current[0]) / s.camera.zoom;
        const dy = -(cy - lastTouchCenterRef.current[1]) / s.camera.zoom;
        s.setCamera({ viewX: s.camera.viewX - dx, viewY: s.camera.viewY - dy });
        lastTouchCenterRef.current = [cx, cy];
      } else if (e.touches.length === 2) {
        const t0 = e.touches[0], t1 = e.touches[1];
        const dist = Math.hypot(t1.clientX - t0.clientX, t1.clientY - t0.clientY);
        const center: [number, number] = [(t0.clientX + t1.clientX) / 2, (t0.clientY + t1.clientY) / 2];

        // Pinch zoom
        if (lastTouchDistRef.current > 0) {
          const factor = dist / lastTouchDistRef.current;
          const rect = canvas.getBoundingClientRect();
          const sx = center[0] - rect.left;
          const sy = center[1] - rect.top;
          const cam = { ...s.camera };
          applyZoom(cam, factor, sx, sy, cw, ch);
          s.setCamera(cam);
        }

        // Two-finger pan
        const dx = (center[0] - lastTouchCenterRef.current[0]) / s.camera.zoom;
        const dy = -(center[1] - lastTouchCenterRef.current[1]) / s.camera.zoom;
        if (Math.abs(dx) > 0.001 || Math.abs(dy) > 0.001) {
          s.setCamera({ viewX: s.camera.viewX - dx, viewY: s.camera.viewY - dy });
        }

        lastTouchDistRef.current = dist;
        lastTouchCenterRef.current = center;
      }
    };

    const handleTouchEnd = (e: TouchEvent) => {
      clearLongPress();
      // If no significant movement and single finger, treat as tap → select element
      if (e.touches.length === 0 && !touchMoved && !longPressFired && touchCountRef.current === 1) {
        const s = store.getState();
        const rect = canvas.getBoundingClientRect();
        const sx = touchStartPos[0] - rect.left;
        const sy = touchStartPos[1] - rect.top;
        const [cw, ch] = getCanvasSize();
        const world = screenToWorld(s.camera, sx, sy, cw, ch);
        const id = hitTest(world, s.elements, s.detectedSpaces);
        s.setSelectedId(id);
        s.refreshAll();
      }
      touchCountRef.current = e.touches.length;
      if (e.touches.length === 0) {
        lastTouchDistRef.current = 0;
      } else if (e.touches.length === 1) {
        lastTouchCenterRef.current = [e.touches[0].clientX, e.touches[0].clientY];
        lastTouchDistRef.current = 0;
      }
    };

    canvas.addEventListener('touchstart', handleTouchStart, { passive: false });
    canvas.addEventListener('touchmove', handleTouchMove, { passive: false });
    canvas.addEventListener('touchend', handleTouchEnd, { passive: false });
    canvas.addEventListener('touchcancel', handleTouchEnd, { passive: false });

    return () => {
      clearLongPress();
      canvas.removeEventListener('touchstart', handleTouchStart);
      canvas.removeEventListener('touchmove', handleTouchMove);
      canvas.removeEventListener('touchend', handleTouchEnd);
      canvas.removeEventListener('touchcancel', handleTouchEnd);
    };
  }, [store, getCanvasSize]);

  const s = useFloorplanStore();
  const zoom = s.camera.zoom;
  const toolNames: Record<string, string> = {
    select: 'Select', wall: 'Wall', door: 'Door', window: 'Window', column: 'Column', slab: 'Slab', furniture: 'Furniture',
  };
  const selectedIsWall = s.selectedId && !s.selectedId.startsWith('space-') && s.elements.find(e => e.id === s.selectedId)?.type === 'Wall';

  return (
    <div style={{ position: 'relative', width: '100%', height: '100%', background: '#15151f', overflow: 'hidden', touchAction: 'none' }}>
      <canvas
        ref={canvasRef}
        style={{ display: 'block', width: '100%', height: '100%', touchAction: 'none' }}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onWheel={handleWheel}
        onDoubleClick={handleDblClick}
        onContextMenu={(e) => e.preventDefault()}
      />

      {/* Status bar — compact on mobile */}
      <div style={{
        position: 'absolute', bottom: 0, left: 0, right: 0, height: isMobile ? 20 : 24,
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        padding: '0 12px', background: 'rgba(21,21,31,0.9)',
        borderTop: '1px solid rgba(255,255,255,0.06)',
        fontSize: isMobile ? 9 : 11, color: 'rgba(255,255,255,0.5)', pointerEvents: 'none',
        fontFamily: 'inherit', gap: 12,
      }}>
        <div style={{ display: 'flex', gap: isMobile ? 6 : 12, alignItems: 'center' }}>
          <span>{s.detectedSpaces.length} spaces, {s.elements.length} elements</span>
          {!isMobile && (
            <>
              <span style={{ opacity: 0.3 }}>|</span>
              <span style={{ color: 'var(--accent)' }}>{toolNames[s.currentTool]}</span>
              <span style={{ opacity: 0.3 }}>|</span>
              <span>{Math.round(zoom)}px/m</span>
              <span style={{ opacity: 0.3 }}>|</span>
              <span>Snap: {s.snapEnabled ? 'on' : 'off'}</span>
              {altHeld && s.selectedId?.startsWith('space-') && (
                <>
                  <span style={{ opacity: 0.3 }}>|</span>
                  <span style={{ color: '#f5a623' }}>Alt</span>
                </>
              )}
            </>
          )}
        </div>
        {!isMobile && (
          <div>
            <span>{mouseWorldPos.x.toFixed(2)}, {mouseWorldPos.y.toFixed(2)}</span>
          </div>
        )}
      </div>

      {/* Context menu — desktop right-click + mobile long press */}
      {contextMenu && (
        <div
          style={{
            position: 'fixed', left: contextMenu.x, top: contextMenu.y,
            background: 'var(--surface, #1e1e2e)', border: '1px solid var(--border, #444)',
            borderRadius: 6, padding: '4px 0', zIndex: 1000, minWidth: 180,
            boxShadow: '0 4px 16px rgba(0,0,0,0.4)',
          }}
          onMouseDown={(e) => e.stopPropagation()}
          onTouchStart={(e) => e.stopPropagation()}
        >
          {s.selectedId && !s.selectedId.startsWith('space-') && (
            <>
              <CtxItem label="Delete" shortcut={isMobile ? undefined : 'Del'} onClick={() => ctxAction('delete')} />
              <CtxItem label="Duplicate" shortcut={isMobile ? undefined : 'Ctrl+D'} onClick={() => ctxAction('duplicate')} />
              <div style={{ height: 1, background: 'var(--border, #444)', margin: '4px 0' }} />
            </>
          )}
          {selectedIsWall && (
            <>
              <CtxItem
                label={(s.elements.find(e => e.id === s.selectedId) as WallElement | undefined)?.anchored ? 'Unanchor Wall' : 'Anchor Wall'}
                shortcut="A"
                onClick={() => ctxAction('anchor')}
              />
              <div style={{ height: 1, background: 'var(--border, #444)', margin: '4px 0' }} />
            </>
          )}
          {s.selectedId && !s.selectedId.startsWith('space-') && (
            <>
              <CtxItem label="Bring to Front" onClick={() => ctxAction('front')} />
              <CtxItem label="Send to Back" onClick={() => ctxAction('back')} />
            </>
          )}
          {!s.selectedId && (
            <div style={{ padding: '6px 16px', fontSize: 12, color: 'var(--text-dim, #888)' }}>No selection</div>
          )}
        </div>
      )}
    </div>
  );
}

function CtxItem({ label, shortcut, onClick }: { label: string; shortcut?: string; onClick: () => void }) {
  return (
    <div
      onClick={onClick}
      style={{
        padding: '6px 16px', fontSize: 12, cursor: 'pointer',
        display: 'flex', justifyContent: 'space-between', alignItems: 'center',
        color: 'var(--text, #cdd6f4)',
      }}
      onMouseEnter={(e) => (e.currentTarget.style.background = 'rgba(137,180,250,0.1)')}
      onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
    >
      <span>{label}</span>
      {shortcut && <span style={{ fontSize: 10, color: 'var(--text-dim, #888)', marginLeft: 24 }}>{shortcut}</span>}
    </div>
  );
}
