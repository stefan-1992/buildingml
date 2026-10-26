import { create } from 'zustand';
import type { Diagnostic } from '@shared/types.ts';
import type { DataProvider } from '@shared/data-provider.ts';
import { RestProvider } from '@shared/providers/rest-provider.ts';

export type ViewMode = '3d' | '2d' | 'split' | 'ifc';

interface AppState {
  viewMode: ViewMode;
  source: string;
  diagnostics: Diagnostic[];
  status: string;
  provider: DataProvider;
  building: boolean;
  exampleName: string;

  setViewMode: (mode: ViewMode) => void;
  setSource: (source: string) => void;
  setDiagnostics: (diags: Diagnostic[]) => void;
  setStatus: (status: string) => void;
  setProvider: (provider: DataProvider) => void;
  setBuilding: (building: boolean) => void;
  setExampleName: (name: string) => void;
}

const DEFAULT_SOURCE = `# Site: Example Residence
  location: 49.4521, 12.1803

## Building: House
  type: residential

### Storey: Ground Floor
  elevation: 0.0 m
  height: 2.80 m

#### Space: Living Room
  area: 30 m2

  Wall: W-01
    from: (0, 0) to: (6, 0)
    height: 2.80 m
    thickness: 0.24 m
    Window: WIN-01
      offset: 1.5 m
      width: 1.8 m
      height: 1.4 m
      sill_height: 0.8 m

  Wall: W-02
    from: (6, 0) to: (6, 5)
    height: 2.80 m
    thickness: 0.24 m
    Door: D-01
      offset: 1.0 m
      width: 0.9 m
      height: 2.1 m

  Wall: W-03
    from: (6, 5) to: (0, 5)
    height: 2.80 m
    thickness: 0.24 m
    Window: WIN-02
      offset: 1.0 m
      width: 2.0 m
      height: 1.6 m
      sill_height: 0.8 m

  Wall: W-04
    from: (0, 5) to: (0, 0)
    height: 2.80 m
    thickness: 0.24 m

  Slab: SLAB-GF
    boundary: [(0,0), (6,0), (6,5), (0,5)]
    thickness: 0.22 m

  Column: COL-01
    position: (3, 2.5)
    height: 2.80 m
    profile: rectangle(0.3, 0.3)

  Beam: B-01
    from: (0, 2.5) to: (6, 2.5)
    profile: rectangle(0.3, 0.4)
`;

export const DEFAULT_EXAMPLE = 'six_storey_residential.bml';

export const useAppStore = create<AppState>((set) => ({
  viewMode: '3d',
  source: DEFAULT_SOURCE,
  diagnostics: [],
  status: 'Ready',
  provider: new RestProvider(),
  building: false,
  exampleName: DEFAULT_EXAMPLE,

  setViewMode: (mode) => set({ viewMode: mode }),
  setSource: (source) => set({ source }),
  setDiagnostics: (diagnostics) => set({ diagnostics }),
  setStatus: (status) => set({ status }),
  setProvider: (provider) => set({ provider }),
  setBuilding: (building) => set({ building }),
  setExampleName: (name) => set({ exampleName: name }),
}));
