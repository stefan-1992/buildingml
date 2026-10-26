import { describe, it, expect, beforeEach } from 'vitest';
import { useFloorplanStore } from '../../../state/floorplan-store.ts';
import type { WallElement } from '../element-model.ts';
import type { SemanticModel } from '../../types.ts';

/** Build a minimal SemanticModel with one storey. */
function makeModel(storeyName: string, elements: Array<{
  type: string;
  id: string;
  from?: [number, number];
  to?: [number, number];
}>): SemanticModel {
  const resolvedElements = elements.map(e => ({
    element_type: e.type as 'Wall',
    id: e.id,
    properties: e.from && e.to ? [
      {
        key: 'from',
        value: { Axis: { from: e.from, to: e.to } },
        span: { start: { line: 0, column: 0, byte_offset: 0 }, end: { line: 0, column: 0, byte_offset: 0 } },
      },
      {
        key: 'height',
        value: { Number: 2.8 },
        span: { start: { line: 0, column: 0, byte_offset: 0 }, end: { line: 0, column: 0, byte_offset: 0 } },
      },
      {
        key: 'thickness',
        value: { Number: 0.24 },
        span: { start: { line: 0, column: 0, byte_offset: 0 }, end: { line: 0, column: 0, byte_offset: 0 } },
      },
    ] : [],
    children: [],
    span: { start: { line: 0, column: 0, byte_offset: 0 }, end: { line: 0, column: 0, byte_offset: 0 } },
  }));

  return {
    sites: [{
      name: 'Test Site',
      properties: [],
      buildings: [{
        name: 'Test Building',
        properties: [],
        storeys: [{
          name: storeyName,
          properties: [
            { key: 'elevation', value: { Number: 0 }, span: { start: { line: 0, column: 0, byte_offset: 0 }, end: { line: 0, column: 0, byte_offset: 0 } } },
            { key: 'height', value: { Number: 2.8 }, span: { start: { line: 0, column: 0, byte_offset: 0 }, end: { line: 0, column: 0, byte_offset: 0 } } },
          ],
          spaces: [],
          elements: resolvedElements,
          span: { start: { line: 0, column: 0, byte_offset: 0 }, end: { line: 0, column: 0, byte_offset: 0 } },
        }],
        span: { start: { line: 0, column: 0, byte_offset: 0 }, end: { line: 0, column: 0, byte_offset: 0 } },
      }],
      span: { start: { line: 0, column: 0, byte_offset: 0 }, end: { line: 0, column: 0, byte_offset: 0 } },
    }],
    materials: [],
    asset_libraries: [],
  };
}

describe('applyBmlSource (code -> 2D sync)', () => {
  beforeEach(() => {
    // Reset the store fully, including undo stacks
    useFloorplanStore.setState({ undoStack: [], redoStack: [] });
    const store = useFloorplanStore.getState();
    store.loadExample();
  });

  it('applies a new model from BML source and updates elements', () => {
    const model = makeModel('Ground Floor', [
      { type: 'Wall', id: 'W-NEW-01', from: [0, 0], to: [10, 0] },
      { type: 'Wall', id: 'W-NEW-02', from: [10, 0], to: [10, 8] },
    ]);

    const bmlSource = '# Site: Test\n\n## Building: B\n\n### Storey: Ground Floor\n';
    useFloorplanStore.getState().applyBmlSource(bmlSource, model);

    const state = useFloorplanStore.getState();
    expect(state.elements).toHaveLength(2);
    expect(state.elements[0].id).toBe('W-NEW-01');
    expect(state.elements[1].id).toBe('W-NEW-02');
    expect(state.bmlOutput).toBe(bmlSource);
    expect(state.originalFileSource).toBe(bmlSource);
  });

  it('saves previous state to undo stack', () => {
    const initialElements = useFloorplanStore.getState().elements;
    const initialCount = initialElements.length;

    const model = makeModel('Ground Floor', [
      { type: 'Wall', id: 'W-X', from: [0, 0], to: [5, 0] },
    ]);

    useFloorplanStore.getState().applyBmlSource('source', model);

    const state = useFloorplanStore.getState();
    expect(state.undoStack).toHaveLength(1);
    expect(state.elements).toHaveLength(1);

    // Undo should restore the initial elements
    state.undo();
    const restored = useFloorplanStore.getState();
    expect(restored.elements.length).toBe(initialCount);
  });

  it('clears redo stack when applying new source', () => {
    // Do an edit then undo to create redo state
    const store = useFloorplanStore.getState();
    store.saveState();
    store.deleteSelected(); // noop since nothing selected, but triggers state

    const model = makeModel('Ground Floor', [
      { type: 'Wall', id: 'W-Y', from: [0, 0], to: [3, 0] },
    ]);
    useFloorplanStore.getState().applyBmlSource('src', model);

    expect(useFloorplanStore.getState().redoStack).toHaveLength(0);
  });

  it('clears selection when applying new source', () => {
    useFloorplanStore.getState().setSelectedId('W-01');
    expect(useFloorplanStore.getState().selectedId).toBe('W-01');

    const model = makeModel('Ground Floor', [
      { type: 'Wall', id: 'W-A', from: [0, 0], to: [4, 0] },
    ]);
    useFloorplanStore.getState().applyBmlSource('src', model);

    expect(useFloorplanStore.getState().selectedId).toBeNull();
  });

  it('updates loadedModel for storey switching', () => {
    const model = makeModel('First Floor', [
      { type: 'Wall', id: 'W-FF', from: [0, 0], to: [5, 0] },
    ]);
    useFloorplanStore.getState().applyBmlSource('src', model);

    expect(useFloorplanStore.getState().loadedModel).toBe(model);
  });
});

describe('bi-directional sync round-trip', () => {
  it('2D edit -> BML update -> code-apply -> 2D restored correctly', () => {
    // Start with example
    useFloorplanStore.getState().loadExample();
    const initialBml = useFloorplanStore.getState().bmlOutput;

    // Simulate a 2D edit (move a wall)
    const state = useFloorplanStore.getState();
    state.saveState();
    const wall = state.elements.find(e => e.type === 'Wall') as WallElement;
    if (wall) {
      wall.from.x += 1;
      wall.to.x += 1;
    }
    state.refreshAll();

    const afterEditBml = useFloorplanStore.getState().bmlOutput;
    expect(afterEditBml).not.toBe(initialBml);

    // Now simulate code edit that restores original walls via model
    const model = makeModel('Ground Floor', [
      { type: 'Wall', id: 'W-01', from: [0, 0], to: [6, 0] },
      { type: 'Wall', id: 'W-02', from: [6, 0], to: [6, 5] },
    ]);
    useFloorplanStore.getState().applyBmlSource('restored source', model);

    const afterApply = useFloorplanStore.getState();
    expect(afterApply.elements).toHaveLength(2);
    expect((afterApply.elements[0] as WallElement).from.x).toBe(0);
    expect((afterApply.elements[0] as WallElement).to.x).toBe(6);
  });
});

describe('undo across edit types', () => {
  it('undo after code edit restores previous 2D state', () => {
    useFloorplanStore.getState().loadExample();
    const initialCount = useFloorplanStore.getState().elements.length;

    // Apply code edit
    const model = makeModel('Ground Floor', [
      { type: 'Wall', id: 'W-Z', from: [0, 0], to: [2, 0] },
    ]);
    useFloorplanStore.getState().applyBmlSource('code edit', model);
    expect(useFloorplanStore.getState().elements).toHaveLength(1);

    // Undo
    useFloorplanStore.getState().undo();
    expect(useFloorplanStore.getState().elements.length).toBe(initialCount);
  });

  it('redo after undo restores code edit', () => {
    useFloorplanStore.getState().loadExample();

    // Apply code edit
    const model = makeModel('Ground Floor', [
      { type: 'Wall', id: 'W-REDO', from: [0, 0], to: [3, 0] },
    ]);
    useFloorplanStore.getState().applyBmlSource('code edit', model);

    // Undo
    useFloorplanStore.getState().undo();

    // Redo
    useFloorplanStore.getState().redo();
    const state = useFloorplanStore.getState();
    expect(state.elements).toHaveLength(1);
    expect(state.elements[0].id).toBe('W-REDO');
  });
});

describe('applyBmlSource keeps the active plan', () => {
  it('stays on the site plan when a building plan appears before it', () => {
    const wall = (id: string) => makeModel('X', [{ type: 'Wall', id, from: [0, 0], to: [5, 0] }]).sites[0].buildings[0].storeys[0].elements[0];
    const model = makeModel('GF', [{ type: 'Wall', id: 'W1', from: [0, 0], to: [5, 0] }]);
    model.sites[0].elements = [wall('W-SITE')];
    useFloorplanStore.getState().loadFromModel(model, 1);
    expect(useFloorplanStore.getState().currentPlan).toEqual({ level: 'site', index: 0 });

    const edited = structuredClone(model);
    edited.sites[0].buildings[0].elements = [wall('W-BLDG')];
    useFloorplanStore.getState().applyBmlSource('source', edited);
    const state = useFloorplanStore.getState();
    expect(state.currentStoreyIndex).toBe(2);
    expect(state.currentPlan).toEqual({ level: 'site', index: 0 });
    expect(state.elements[0].id).toBe('W-SITE');
  });
});
