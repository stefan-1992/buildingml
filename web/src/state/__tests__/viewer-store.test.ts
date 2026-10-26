import { describe, it, expect, beforeEach } from 'vitest';
import { useViewerStore } from '../viewer-store.ts';

describe('viewer-store', () => {
  beforeEach(() => {
    // Reset store between tests
    useViewerStore.setState({
      scene: null,
      ifcScene: null,
      wireframe: false,
      selectedElement: null,
      rendererEngine: 'webgl',
    });
  });

  it('selectedElement defaults to null', () => {
    expect(useViewerStore.getState().selectedElement).toBeNull();
  });

  it('setSelectedElement updates selection', () => {
    const pick = {
      id: 'W-01',
      elementType: 'Wall',
      label: 'Wall: W-01',
      properties: [['height', '2.8'], ['thickness', '0.24']] as [string, string][],
    };
    useViewerStore.getState().setSelectedElement(pick);
    expect(useViewerStore.getState().selectedElement).toEqual(pick);
  });

  it('setSelectedElement(null) clears selection', () => {
    useViewerStore.getState().setSelectedElement({
      id: 'W-01',
      elementType: 'Wall',
      label: 'Wall: W-01',
      properties: [],
    });
    expect(useViewerStore.getState().selectedElement).not.toBeNull();

    useViewerStore.getState().setSelectedElement(null);
    expect(useViewerStore.getState().selectedElement).toBeNull();
  });

  it('setScene clears selection', () => {
    useViewerStore.getState().setSelectedElement({
      id: 'W-01',
      elementType: 'Wall',
      label: 'Wall: W-01',
      properties: [],
    });
    expect(useViewerStore.getState().selectedElement).not.toBeNull();

    useViewerStore.getState().setScene({ nodes: [] });
    expect(useViewerStore.getState().selectedElement).toBeNull();
  });

  it('toggleRendererEngine switches between webgl and threejs', () => {
    expect(useViewerStore.getState().rendererEngine).toBe('webgl');
    useViewerStore.getState().toggleRendererEngine();
    expect(useViewerStore.getState().rendererEngine).toBe('threejs');
    useViewerStore.getState().toggleRendererEngine();
    expect(useViewerStore.getState().rendererEngine).toBe('webgl');
  });
});
