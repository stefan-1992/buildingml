import { create } from 'zustand';
import type { SceneGraph } from '@shared/types.ts';
import type { SectionCutState } from '@shared/renderer/section-cut.ts';
import { DEFAULT_SECTION_CUT } from '@shared/renderer/section-cut.ts';
import type { PickResult } from '@shared/renderer/renderer-interface.ts';

export type RendererEngine = 'webgl' | 'threejs';

interface ViewerState {
  scene: SceneGraph | null;
  ifcScene: SceneGraph | null;
  wireframe: boolean;
  sectionCut: SectionCutState;
  lateralCut: SectionCutState;
  rendererEngine: RendererEngine;

  /** Currently selected element in 3D view. */
  selectedElement: PickResult | null;

  setScene: (scene: SceneGraph) => void;
  setIfcScene: (scene: SceneGraph | null) => void;
  toggleWireframe: () => void;
  setSectionCut: (update: Partial<SectionCutState>) => void;
  setLateralCut: (update: Partial<SectionCutState>) => void;
  setRendererEngine: (engine: RendererEngine) => void;
  toggleRendererEngine: () => void;
  setSelectedElement: (element: PickResult | null) => void;
}

export const useViewerStore = create<ViewerState>((set) => ({
  scene: null,
  ifcScene: null,
  wireframe: false,
  sectionCut: { ...DEFAULT_SECTION_CUT },
  lateralCut: { ...DEFAULT_SECTION_CUT },
  rendererEngine: 'webgl',
  selectedElement: null,

  setScene: (scene) => set({ scene, selectedElement: null }),
  setIfcScene: (ifcScene) => set({ ifcScene }),
  toggleWireframe: () => set((s) => ({ wireframe: !s.wireframe })),
  setSectionCut: (update) => set((s) => ({ sectionCut: { ...s.sectionCut, ...update } })),
  setLateralCut: (update) => set((s) => ({ lateralCut: { ...s.lateralCut, ...update } })),
  setRendererEngine: (rendererEngine) => set({ rendererEngine }),
  toggleRendererEngine: () => set((s) => ({
    rendererEngine: s.rendererEngine === 'webgl' ? 'threejs' : 'webgl',
  })),
  setSelectedElement: (selectedElement) => set({ selectedElement }),
}));
