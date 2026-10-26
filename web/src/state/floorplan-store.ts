import { create } from 'zustand';
import type {
  FloorplanElement, DetectedSpace, IdCounters, Point2D, WallElement,
  ColumnElement, SlabElement,
} from '@shared/floorplan/element-model.ts';
import {
  createIdCounters, createWall, createDoor, createWindow, createSlab,
  genId,
} from '@shared/floorplan/element-model.ts';
import { detectSpaces, pointInPolygon } from '@shared/floorplan/space-detection.ts';
import { type CanvasCamera, createDefaultCamera } from '@shared/floorplan/canvas-camera.ts';
import { generateFullBml, keptBlocks, type KeptBlocks } from '@shared/floorplan/bml-generator.ts';
import type { SemanticModel } from '@shared/types.ts';
import { getStoreys, importStorey, type PlanRef } from '@shared/floorplan/model-importer.ts';

export type FloorplanTool = 'select' | 'wall' | 'door' | 'window' | 'column' | 'slab' | 'furniture';

interface FloorplanState {
  elements: FloorplanElement[];
  detectedSpaces: DetectedSpace[];
  selectedId: string | null;
  hoveredId: string | null;
  currentTool: FloorplanTool;
  showGrid: boolean;
  snapEnabled: boolean;
  camera: CanvasCamera;
  idCounters: IdCounters;
  wallStart: Point2D | null;
  slabPoints: Point2D[];
  ghostPos: Point2D | null;
  bmlOutput: string;
  storeyName: string;
  storeyElevation: number;
  storeyHeight: number;
  originalFileSource: string;
  /** Index into `getStoreys(loadedModel)` (storeys, then building/site-level plans). */
  currentStoreyIndex: number;
  /** Heading that `currentStoreyIndex` points at; set together with it from `importStorey().plan`. */
  currentPlan: PlanRef;
  /** Element blocks of the plan the 2D editor did not import (beams, proxies, MEP); kept on save. */
  keptBlocks: KeptBlocks;
  loadedModel: SemanticModel | null;

  // Undo/redo
  undoStack: string[];
  redoStack: string[];

  // Perp drag state
  perpDragMode: boolean;
  perpDragPolygon: Point2D[] | null;

  // Furniture placement
  selectedAsset: string;
  setSelectedAsset: (asset: string) => void;

  // Save tracking
  hasUnsavedChanges: boolean;
  lastSavedBml: string;

  // Actions
  setTool: (tool: FloorplanTool) => void;
  setSelectedId: (id: string | null) => void;
  setHoveredId: (id: string | null) => void;
  setGhostPos: (pos: Point2D | null) => void;
  setCamera: (cam: Partial<CanvasCamera>) => void;
  toggleGrid: () => void;
  toggleSnap: () => void;
  saveState: () => void;
  undo: () => void;
  redo: () => void;
  refreshAll: () => void;
  addElement: (elem: FloorplanElement) => void;
  deleteSelected: () => void;
  duplicateSelected: () => void;
  toggleAnchor: () => void;
  updateProperty: (elemId: string, path: string, value: unknown) => void;
  renameSpace: (idx: number, name: string) => void;
  setStoreyName: (name: string) => void;
  setStoreyElevation: (elev: number) => void;
  setStoreyHeight: (height: number) => void;
  loadFromModel: (model: SemanticModel, storeyIndex: number) => boolean;
  loadExample: () => void;
  setWallStart: (p: Point2D | null) => void;
  setSlabPoints: (pts: Point2D[]) => void;
  bringToFront: () => void;
  sendToBack: () => void;
  zoomToFit: (canvasWidth: number, canvasHeight: number) => void;
  zoomToElement: (id: string) => void;
  zoomToSpace: (spaceId: string) => void;
  saveBml: () => Promise<void>;
  nudgeSelected: (dx: number, dy: number) => void;
  nudgeElement: (id: string, dx: number, dy: number) => void;
  setPerpDragMode: (mode: boolean) => void;
  setPerpDragPolygon: (poly: Point2D[] | null) => void;
  setOriginalFileSource: (source: string) => void;

  // Bi-directional sync: apply BML source text to update 2D elements
  applyBmlSource: (source: string, model: SemanticModel) => void;
}

function regenerateBml(state: FloorplanState): string {
  return generateFullBml(
    state.elements,
    state.detectedSpaces,
    state.storeyName,
    state.storeyElevation,
    state.storeyHeight,
    state.originalFileSource,
    state.currentPlan,
    state.keptBlocks,
  );
}

export const useFloorplanStore = create<FloorplanState>((set, get) => ({
  elements: [],
  detectedSpaces: [],
  selectedId: null,
  hoveredId: null,
  currentTool: 'select',
  showGrid: true,
  snapEnabled: true,
  camera: createDefaultCamera(),
  idCounters: createIdCounters(),
  wallStart: null,
  slabPoints: [],
  ghostPos: null,
  bmlOutput: '',
  storeyName: 'Ground Floor',
  storeyElevation: 0,
  storeyHeight: 2.8,
  originalFileSource: '',
  currentStoreyIndex: 0,
  currentPlan: { level: 'storey', index: 0 },
  keptBlocks: new Map(),
  loadedModel: null,
  undoStack: [],
  redoStack: [],
  perpDragMode: false,
  perpDragPolygon: null,
  selectedAsset: '',
  hasUnsavedChanges: false,
  lastSavedBml: '',

  setTool: (tool) => set({ currentTool: tool, wallStart: null, slabPoints: [] }),
  setSelectedAsset: (asset) => set({ selectedAsset: asset }),
  setSelectedId: (id) => set({ selectedId: id }),
  setHoveredId: (id) => set({ hoveredId: id }),
  setGhostPos: (pos) => set({ ghostPos: pos }),
  setCamera: (cam) => set((s) => ({ camera: { ...s.camera, ...cam } })),
  toggleGrid: () => set((s) => ({ showGrid: !s.showGrid })),
  toggleSnap: () => set((s) => ({ snapEnabled: !s.snapEnabled })),

  saveState: () => set((s) => {
    const stack = [...s.undoStack, JSON.stringify(s.elements)];
    if (stack.length > 100) stack.shift();
    return { undoStack: stack, redoStack: [] };
  }),

  undo: () => set((s) => {
    if (s.undoStack.length === 0) return s;
    const newRedo = [...s.redoStack, JSON.stringify(s.elements)];
    const newUndo = [...s.undoStack];
    const last = newUndo.pop()!;
    const elements = JSON.parse(last);
    const detectedSpaces = detectSpaces(elements, s.detectedSpaces);
    const bmlOutput = regenerateBml({ ...s, elements, detectedSpaces });
    return { elements, undoStack: newUndo, redoStack: newRedo, selectedId: null, detectedSpaces, bmlOutput, originalFileSource: bmlOutput };
  }),

  redo: () => set((s) => {
    if (s.redoStack.length === 0) return s;
    const newUndo = [...s.undoStack, JSON.stringify(s.elements)];
    const newRedo = [...s.redoStack];
    const last = newRedo.pop()!;
    const elements = JSON.parse(last);
    const detectedSpaces = detectSpaces(elements, s.detectedSpaces);
    const bmlOutput = regenerateBml({ ...s, elements, detectedSpaces });
    return { elements, undoStack: newUndo, redoStack: newRedo, selectedId: null, detectedSpaces, bmlOutput, originalFileSource: bmlOutput };
  }),

  refreshAll: () => set((s) => {
    const detectedSpaces = detectSpaces(s.elements, s.detectedSpaces);
    const bmlOutput = regenerateBml({ ...s, detectedSpaces });
    return { detectedSpaces, bmlOutput, originalFileSource: bmlOutput, hasUnsavedChanges: bmlOutput !== s.lastSavedBml };
  }),

  addElement: (elem) => set((s) => {
    const elements = [...s.elements, elem];
    const detectedSpaces = detectSpaces(elements, s.detectedSpaces);
    const bmlOutput = regenerateBml({ ...s, elements, detectedSpaces });
    return { elements, detectedSpaces, bmlOutput, originalFileSource: bmlOutput };
  }),

  deleteSelected: () => {
    const s = get();
    if (!s.selectedId || s.selectedId.startsWith('space-')) return;
    const sel = s.elements.find(e => e.id === s.selectedId);
    let elements: FloorplanElement[];
    if (sel?.type === 'Wall') {
      elements = s.elements.filter(e => e.id !== s.selectedId && !('parentWall' in e && e.parentWall === s.selectedId));
    } else {
      elements = s.elements.filter(e => e.id !== s.selectedId);
    }
    const detectedSpaces = detectSpaces(elements, s.detectedSpaces);
    const bmlOutput = regenerateBml({ ...s, elements, detectedSpaces });
    set({ elements, selectedId: null, detectedSpaces, bmlOutput, originalFileSource: bmlOutput });
  },

  duplicateSelected: () => {
    const s = get();
    if (!s.selectedId || s.selectedId.startsWith('space-')) return;
    const sel = s.elements.find(e => e.id === s.selectedId);
    if (!sel) return;
    const clone = JSON.parse(JSON.stringify(sel));
    clone.id = genId(clone.type, s.idCounters);
    if (clone.type === 'Wall') { clone.from.x += 0.5; clone.from.y += 0.5; clone.to.x += 0.5; clone.to.y += 0.5; }
    else if (clone.type === 'Column') { clone.x += 0.5; clone.y += 0.5; }
    else if (clone.type === 'Furniture') { clone.x += 0.5; clone.y += 0.5; }
    else if (clone.type === 'Slab') { for (const p of clone.boundary) { p.x += 0.5; p.y += 0.5; } }
    set((s) => {
      const elements = [...s.elements, clone];
      const detectedSpaces = detectSpaces(elements, s.detectedSpaces);
      const bmlOutput = regenerateBml({ ...s, elements, detectedSpaces });
      return { elements, selectedId: clone.id, detectedSpaces, bmlOutput, originalFileSource: bmlOutput };
    });
  },

  toggleAnchor: () => {
    const s = get();
    if (!s.selectedId || s.selectedId.startsWith('space-')) return;
    const sel = s.elements.find(e => e.id === s.selectedId) as WallElement | undefined;
    if (!sel || sel.type !== 'Wall') return;
    sel.anchored = !sel.anchored;
    set({ elements: [...s.elements] });
  },

  updateProperty: (elemId, path, value) => {
    const s = get();
    const elem = s.elements.find(e => e.id === elemId);
    if (!elem) return;

    if (path === 'id') {
      const oldId = elem.id;
      (elem as { id: string }).id = value as string;
      s.elements.forEach(e => {
        if ('parentWall' in e && e.parentWall === oldId) {
          (e as { parentWall: string }).parentWall = value as string;
        }
      });
      set({ elements: [...s.elements], selectedId: value as string });
    } else if (path.includes('.')) {
      const parts = path.split('.');
      let obj: Record<string, unknown> = elem as unknown as Record<string, unknown>;
      for (let i = 0; i < parts.length - 1; i++) obj = obj[parts[i]] as Record<string, unknown>;
      obj[parts[parts.length - 1]] = value;
      set({ elements: [...s.elements] });
    } else {
      (elem as unknown as Record<string, unknown>)[path] = value;
      set({ elements: [...s.elements] });
    }
    get().refreshAll();
  },

  renameSpace: (idx, name) => set((s) => {
    const spaces = [...s.detectedSpaces];
    if (spaces[idx]) {
      spaces[idx] = { ...spaces[idx], name };
      const bmlOutput = regenerateBml({ ...s, detectedSpaces: spaces });
      return { detectedSpaces: spaces, bmlOutput, originalFileSource: bmlOutput };
    }
    return s;
  }),

  setStoreyName: (name) => set((s) => {
    const bmlOutput = regenerateBml({ ...s, storeyName: name });
    return { storeyName: name, bmlOutput, originalFileSource: bmlOutput };
  }),

  setStoreyElevation: (elev) => set((s) => {
    const bmlOutput = regenerateBml({ ...s, storeyElevation: elev });
    return { storeyElevation: elev, bmlOutput, originalFileSource: bmlOutput };
  }),

  setStoreyHeight: (height) => set((s) => {
    const bmlOutput = regenerateBml({ ...s, storeyHeight: height });
    return { storeyHeight: height, bmlOutput, originalFileSource: bmlOutput };
  }),

  loadFromModel: (model, storeyIndex) => {
    const result = importStorey(model, storeyIndex);
    if (!result) return false;
    const s = get();
    const plan = {
      elements: result.elements,
      detectedSpaces: detectSpaces(result.elements, []),
      storeyName: result.name,
      storeyElevation: result.elevation,
      storeyHeight: result.height,
      currentPlan: result.plan,
      keptBlocks: keptBlocks(s.originalFileSource, result.plan, result.importedIds),
    };
    const bmlOutput = regenerateBml({ ...s, ...plan });
    set({
      ...plan,
      idCounters: result.idCounters,
      currentStoreyIndex: storeyIndex,
      loadedModel: model,
      selectedId: null,
      bmlOutput,
      originalFileSource: bmlOutput,
    });
    return true;
  },

  loadExample: () => {
    const counters = createIdCounters();
    const elements: FloorplanElement[] = [];
    elements.push(createWall({ x: 0, y: 0 }, { x: 6, y: 0 }, 2.8, counters));
    elements.push(createWall({ x: 6, y: 0 }, { x: 6, y: 5 }, 2.8, counters));
    elements.push(createWall({ x: 6, y: 5 }, { x: 0, y: 5 }, 2.8, counters));
    elements.push(createWall({ x: 0, y: 5 }, { x: 0, y: 0 }, 2.8, counters));
    elements.push(createWall({ x: 6, y: 0 }, { x: 11, y: 0 }, 2.8, counters));
    elements.push(createWall({ x: 11, y: 0 }, { x: 11, y: 5 }, 2.8, counters));
    elements.push(createWall({ x: 11, y: 5 }, { x: 6, y: 5 }, 2.8, counters));
    elements.push(createWindow('W-01', 1.5, counters));
    elements.push(createDoor('W-02', 1.5, counters));
    elements.push(createWindow('W-03', 1.5, counters));
    elements.push(createWindow('W-05', 1.5, counters));
    elements.push(createWindow('W-06', 1.0, counters));
    elements.push(createSlab([{ x: 0, y: 0 }, { x: 11, y: 0 }, { x: 11, y: 5 }, { x: 0, y: 5 }], counters));

    const detectedSpaces = detectSpaces(elements, []);
    const bmlOutput = generateFullBml(elements, detectedSpaces, 'Ground Floor', 0, 2.8, '', { level: 'storey', index: 0 });
    set({
      elements,
      idCounters: counters,
      camera: { viewX: 5.5, viewY: 2.5, zoom: 50 },
      selectedId: null,
      detectedSpaces,
      bmlOutput,
    });
  },

  bringToFront: () => {
    const s = get();
    if (!s.selectedId || s.selectedId.startsWith('space-')) return;
    const idx = s.elements.findIndex(e => e.id === s.selectedId);
    if (idx < 0 || idx === s.elements.length - 1) return;
    const elems = [...s.elements];
    const [el] = elems.splice(idx, 1);
    elems.push(el);
    set({ elements: elems });
  },

  sendToBack: () => {
    const s = get();
    if (!s.selectedId || s.selectedId.startsWith('space-')) return;
    const idx = s.elements.findIndex(e => e.id === s.selectedId);
    if (idx <= 0) return;
    const elems = [...s.elements];
    const [el] = elems.splice(idx, 1);
    elems.unshift(el);
    set({ elements: elems });
  },

  zoomToFit: (canvasWidth, canvasHeight) => set((s) => {
    const walls = s.elements.filter((e): e is WallElement => e.type === 'Wall');
    const cols = s.elements.filter((e): e is ColumnElement => e.type === 'Column');
    const slabs = s.elements.filter((e): e is SlabElement => e.type === 'Slab');
    if (walls.length === 0 && cols.length === 0 && slabs.length === 0) return s;

    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    for (const w of walls) {
      minX = Math.min(minX, w.from.x, w.to.x); maxX = Math.max(maxX, w.from.x, w.to.x);
      minY = Math.min(minY, w.from.y, w.to.y); maxY = Math.max(maxY, w.from.y, w.to.y);
    }
    for (const c of cols) {
      minX = Math.min(minX, c.x); maxX = Math.max(maxX, c.x);
      minY = Math.min(minY, c.y); maxY = Math.max(maxY, c.y);
    }
    for (const sl of slabs) {
      for (const p of sl.boundary) {
        minX = Math.min(minX, p.x); maxX = Math.max(maxX, p.x);
        minY = Math.min(minY, p.y); maxY = Math.max(maxY, p.y);
      }
    }

    const dx = maxX - minX || 1;
    const dy = maxY - minY || 1;
    const margin = 1.2; // 20% margin
    const zoom = Math.min(canvasWidth / (dx * margin), canvasHeight / (dy * margin));
    return { camera: { viewX: (minX + maxX) / 2, viewY: (minY + maxY) / 2, zoom: Math.max(5, Math.min(500, zoom)) } };
  }),

  zoomToElement: (id) => set((s) => {
    const el = s.elements.find(e => e.id === id);
    if (!el) return s;
    let cx: number, cy: number;
    if (el.type === 'Wall') {
      cx = (el.from.x + el.to.x) / 2; cy = (el.from.y + el.to.y) / 2;
    } else if (el.type === 'Column') {
      cx = (el as ColumnElement).x; cy = (el as ColumnElement).y;
    } else if (el.type === 'Slab') {
      const b = (el as SlabElement).boundary;
      cx = b.reduce((s, p) => s + p.x, 0) / b.length;
      cy = b.reduce((s, p) => s + p.y, 0) / b.length;
    } else {
      return s; // doors/windows — can't center on them meaningfully without parent
    }
    return { camera: { ...s.camera, viewX: cx, viewY: cy } };
  }),

  zoomToSpace: (spaceId) => set((s) => {
    const idx = parseInt(spaceId.split('-')[1]);
    const space = s.detectedSpaces[idx];
    if (!space) return s;
    const cx = space.polygon.reduce((sum, p) => sum + p.x, 0) / space.polygon.length;
    const cy = space.polygon.reduce((sum, p) => sum + p.y, 0) / space.polygon.length;
    return { camera: { ...s.camera, viewX: cx, viewY: cy } };
  }),

  saveBml: async () => {
    const s = get();
    try {
      await fetch('/api/source', { method: 'POST', headers: { 'Content-Type': 'text/plain' }, body: s.bmlOutput });
      set({ lastSavedBml: s.bmlOutput, hasUnsavedChanges: false });
    } catch {
      // Save failed silently — user can retry
    }
  },

  nudgeSelected: (dx, dy) => {
    const s = get();
    if (!s.selectedId) return;
    s.saveState();

    if (s.selectedId.startsWith('space-')) {
      // Nudge space: move all wall endpoints on the space boundary
      const idx = parseInt(s.selectedId.split('-')[1]);
      const space = s.detectedSpaces[idx];
      if (!space) return;
      for (const wallId of space.wallIds) {
        const wall = s.elements.find(e => e.id === wallId) as WallElement | undefined;
        if (wall) {
          wall.from.x += dx; wall.from.y += dy;
          wall.to.x += dx; wall.to.y += dy;
        }
      }
      // Move columns inside space
      for (const el of s.elements) {
        if (el.type === 'Column') {
          const col = el as ColumnElement;
          // Check if the original position was inside the space
          const origX = col.x - dx, origY = col.y - dy;
          if (pointInPolygon({ x: origX, y: origY }, space.polygon)) {
            col.x += dx; col.y += dy;
          }
        }
      }
      set({ elements: [...s.elements] });
      get().refreshAll();
      return;
    }

    const sel = s.elements.find(e => e.id === s.selectedId);
    if (!sel) return;
    if (sel.type === 'Wall') {
      sel.from.x += dx; sel.from.y += dy;
      sel.to.x += dx; sel.to.y += dy;
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
    set({ elements: [...s.elements] });
    get().refreshAll();
  },

  nudgeElement: (id, dx, dy) => {
    const s = get();
    const el = s.elements.find(e => e.id === id);
    if (!el) return;
    s.saveState();
    if (el.type === 'Wall') {
      el.from.x += dx; el.from.y += dy;
      el.to.x += dx; el.to.y += dy;
    } else if (el.type === 'Column') {
      (el as ColumnElement).x += dx;
      (el as ColumnElement).y += dy;
    } else if (el.type === 'Furniture') {
      (el as any).x += dx;
      (el as any).y += dy;
    } else if (el.type === 'Slab') {
      for (const p of (el as SlabElement).boundary) {
        p.x += dx; p.y += dy;
      }
    } else {
      return; // doors/windows use wall-relative offset
    }
    set({ elements: [...s.elements] });
    get().refreshAll();
  },

  setWallStart: (p) => set({ wallStart: p }),
  setSlabPoints: (pts) => set({ slabPoints: pts }),
  setPerpDragMode: (mode) => set({ perpDragMode: mode }),
  setPerpDragPolygon: (poly) => set({ perpDragPolygon: poly }),
  setOriginalFileSource: (source) => set({ originalFileSource: source }),

  applyBmlSource: (source, model) => {
    const s = get();
    // Save current state for undo before applying external changes
    const undoStack = [...s.undoStack, JSON.stringify(s.elements)];
    if (undoStack.length > 100) undoStack.shift();

    // Follow the same heading: plan positions shift when a building/site plan appears or goes
    const samePlan = getStoreys(model).findIndex(p => p.level === s.currentPlan.level && p.index === s.currentPlan.index);
    const storeyIndex = samePlan >= 0 ? samePlan : s.currentStoreyIndex;
    const result = importStorey(model, storeyIndex);
    if (!result) return;

    const detectedSpaces = detectSpaces(result.elements, []);
    set({
      elements: result.elements,
      idCounters: result.idCounters,
      storeyName: result.name,
      storeyElevation: result.elevation,
      storeyHeight: result.height,
      currentStoreyIndex: storeyIndex,
      currentPlan: result.plan,
      keptBlocks: keptBlocks(source, result.plan, result.importedIds),
      loadedModel: model,
      detectedSpaces,
      bmlOutput: source,
      originalFileSource: source,
      selectedId: null,
      undoStack,
      redoStack: [],
      hasUnsavedChanges: source !== s.lastSavedBml,
    });
  },
}));
