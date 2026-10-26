# BuildingML — Specification v0.2

**Date: 2026-02-28 (updated 2026-03-01)**
**Author: Stefan (+ Claude brainstorming)**
**Status: IN PROGRESS — Phase 7a (~80%), 7a′ (complete), 7b (~85%), rest not started**

---

## 1. V0.1 As-Built vs Spec Gap Analysis

BuildingML v0.1 (Phases 1–5) delivered the core language, compiler pipeline, LSP, compliance checking, and 3D visualization. However, several areas of the v0.1 spec (`Specs.md`) were implemented with simplified geometry or stub-only IFC output. This section catalogs those gaps to inform v0.2 priorities.

| Area | Spec Promise | v0.1 Reality | v0.2 Priority |
|------|-------------|--------------|---------------|
| **Roof types** | Gable, hip, flat roof geometry | Flat roof only (ifc_rs limitation) | High |
| **Space boundaries** | Auto-inferred from walls or explicit polygon | Square derived from `area` property | High |
| **Column/Beam IFC geometry** | Full IfcColumn/IfcBeam with extruded profiles | Comment-only metadata in STEP output | High |
| **Stair/Ramp/Railing IFC** | Full entities with geometry representations | Raw STEP stubs, no geometry | Medium |
| **Furniture IFC** | IfcFurnishingElement with geometry | Raw STEP stub, no geometry | Medium |
| **Door entity** | Full IfcDoor with panels/lining | Opening in host wall only | Medium |
| **Slab properties** | Pset_SlabCommon property set | Skipped (ifc_rs `IfcSlabBuilder` lacks `add_properties()`) | Medium |
| **Circle columns (scene)** | Circular profile rendering | Rendered as square box | Medium |
| **Decompiler geometry** | Round-trip with defined loss | Names and hierarchy only, all geometry lost | Low |
| **Curved walls** | Not in v0.1 spec | N/A | Future |
| **Storey-level elements** | `ResolvedStorey.elements` exists in IR | Parser requires elements inside `#### Space:` | High |
| **Floor templates** | Not in v0.1 spec | No repetition mechanism; 5 identical floors = 5× copy-paste | High |
| **Elevator/Shaft element** | Not in v0.1 spec | No vertical transportation element type | Medium |
| **Railing/Ramp positioning** | Geometry properties | No `from:` axis or `position:` — can't be placed in coordinate system | Medium |
| **Space usage enum** | 13 values | Missing: `lobby`, `laundry`, `parking`, `terrace`, `balcony`, `mechanical`, etc. | Low |
| **Space grouping** | Not in v0.1 spec | No way to group rooms into zones (apartments, suites) | Low |
| **Auto-ID generation** | Not in v0.1 spec | 326 elements = 326 manually managed unique IDs | Low |
| **Grammar ghosts** | `Opening`, `Grid`, `Dimension` in grammar | Parsed without error but produce no output — LLM trap | Low |

### 1.1 Root Causes

Most gaps trace to two factors:

1. **ifc_rs limitations**: Missing builder methods, `pub(crate)` visibility restrictions, incomplete type support for certain IFC entities. Workarounds exist (raw STEP injection) but weren't prioritized in v0.1.
2. **Geometry simplifications**: The "no geometry kernel" approach works well for rectangular elements but needs extension for non-rectangular shapes (circular profiles, pitched roofs, arbitrary polygons).
3. **Language expressiveness gaps**: The v0.1 spec focused on individual elements. At scale (6-storey, 326-element building), the lack of repetition mechanisms, storey-level element placement, and limited enums became significant pain points. See `docs/lessons-learned.md` for the full analysis.

### 1.2 Approach for v0.2

Rather than attempting full IFC schema coverage, v0.2 focuses on:
- Bringing all v0.1 element types to proper IFC geometry output
- Language improvements for scale: floor templates, storey-level elements, new element types
- Adding non-rectangular geometry (polygons, cylinders, arcs)
- Building a sustainable web platform for desktop and mobile
- Introducing furniture assets as a composable geometry system
- Testing against real-world IFC files to prioritize features by actual usage

---

## 2. Sub-Phase Roadmap

V0.2 is organized into sub-phases (7a–7h), each with a focused goal. Phases can overlap where there are no dependencies.

```
Phase 7a: IFC Codegen Hardening ──┐
Phase 7a′: Language Improvements ─┤── can run in parallel
Phase 7b: Web Architecture ───────┤
Phase 7e: Real IFC Import ────────┘
          │
          ▼
Phase 7c: Enhanced Geometry ──────┐
Phase 7d: Furniture Assets ───────┤── depend on 7a geometry work
          │                       │
          ▼                       ▼
Phase 7f: Interactive Editing ────── depends on 7b web platform
          │
          ▼
Phase 7g: Chatbot Integration ───── depends on 7f editing
Phase 7h: Compiler Quality ──────── independent, can run anytime
```

---

### 2.1 Phase 7a: IFC Codegen Hardening (~80% COMPLETE)

**Goal**: Bring all v0.1 element types to proper IFC geometry output.

#### Tasks

- [x] **Column geometry**: `IfcExtrudedAreaSolid` with `IfcRectangleProfileDef` or `IfcCircleProfileDef` based on profile property. Uses `StepGeometryWriter` for shared primitives.
- [x] **Beam geometry**: `IfcExtrudedAreaSolid` with proper profile and axis placement (rotated via `IfcAxis2Placement3D`)
- [x] **Stair geometry**: Bounding box representation with proper placement (simplified — full tread/riser geometry deferred)
- [x] **Ramp geometry**: Inclined slab representation
- [x] **Railing geometry**: Extruded profiles
- [x] **Door entity**: Full `IfcDoor` (13 attributes) with panel geometry, not just wall opening
- [ ] **Slab property sets**: Work around `IfcSlabBuilder` limitation — inject `Pset_SlabCommon` directly via raw STEP or alternative builder path
- [ ] **Space boundary polygons**: Support explicit `boundary: [(x,y), ...]` property and infer boundaries from enclosing walls when omitted
- [ ] **Roof pitch**: Evaluate ifc_rs support for `IfcExtrudedAreaSolid` with non-horizontal extrusion direction; fall back to custom STEP for gable/hip roofs
- [x] **Circular column profiles in scene graph**: Tessellated cylinder mesh (16-segment polygon approximation)
- [x] **Furniture geometry**: Extruded bounding box (not just metadata stub)
- [x] **StepGeometryWriter**: Auto-incrementing IDs, shared geometry primitives, proper placement for all element types

#### Success Criteria

- All v0.1 element types produce valid IFC geometry (not just metadata stubs)
- IFC output opens correctly in FreeCAD, BIMvision, or Xbim Xplorer
- Scene graph renders circular columns as cylinders (not boxes)

---

### 2.1′ Phase 7a′: Language Improvements for Scale (COMPLETE)

**Goal**: Address language expressiveness gaps discovered when authoring large buildings (see `docs/lessons-learned.md`).

> **Status**: All features implemented and tested (123 tests pass). Template expansion pass in `bml-core/src/template.rs`. Elevator type fully integrated across grammar, parser, semantic model, type checker, IFC codegen, scene graph, and LSP.

#### Floor Templates (High Priority)

The single biggest pain point at scale. A 6-storey building with 5 identical residential floors requires ~2,750 lines of duplicated BML. Floor templates define a layout once and stamp it across storeys.

See **Section 9** for the full template syntax design.

#### Storey-Level Elements (High Priority)

Allow elements at `###` (storey) level without requiring a `#### Space:` wrapper. The semantic model already supports this (`ResolvedStorey.elements: Vec<ResolvedElement>`), but the parser currently requires elements inside `#### Space:` blocks.

Structural elements like slabs, columns, and beams logically belong to the storey, not to any particular space:

```markdown
### Storey: Floor 1
  elevation: 3.50 m

  Slab: SLAB-F1
    boundary: [(0,0), (40,0), (40,18), (0,18)]
    thickness: 0.25 m

  Column: COL-F1-01
    position: (8.0, 9.0)
    profile: rectangle(0.3, 0.3)
    height: 2.80 m

  #### Space: Unit 101
    usage: living
    area: 95.0 m2
    ...
```

**Implementation**: Parser change only — accept element declarations between `###` heading and the first `####`. The semantic model and codegen paths already handle storey-level elements.

#### Elevator / Shaft Element Type (Medium Priority)

New element type for vertical transportation, present in nearly every multi-storey building:

```markdown
Elevator: EL-01
  position: (19.5, 8.5)
  shaft_width: 1.80 m
  shaft_depth: 2.00 m
  from_storey: Ground Floor
  to_storey: Roof
  capacity: 8 persons
  type: passenger
```

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `position` | Coordinate | Yes | Shaft corner position in storey coordinates |
| `shaft_width` | Length | Yes | Width of the shaft |
| `shaft_depth` | Length | Yes | Depth of the shaft |
| `from_storey` | Reference | No | Bottom storey (defaults to current) |
| `to_storey` | Reference | No | Top storey (defaults to current) |
| `capacity` | String | No | Capacity description |
| `type` | Enum | No | `passenger`, `freight`, `service` |

**IFC mapping**: `IfcTransportElement` with `IfcTransportElementType.ELEVATOR`, shaft as `IfcSpace` with `usage: shaft`.

**Scene graph**: Vertical box spanning from `from_storey` elevation to `to_storey` elevation + semi-transparent shaft walls.

#### Railing/Ramp Geometry (Medium Priority)

Add `from:` axis property to Railing and Ramp so they can be positioned in the coordinate system:

```markdown
Railing: RL-TERRACE-S
  from: (0, 0) to: (40, 0)
  height: 1.10 m

Ramp: RM-GF-01
  from: (16, 18) to: (19, 12)
  width: 5.50 m
  slope: 0.05
```

**Implementation**: Add `from` as an optional `Axis` property to Railing and Ramp in the type checker. Update scene graph to position these elements when axis is provided.

#### Extended Space Usage Enum (Low Priority)

Extend the allowed `usage:` values from 13 to ~25:

| Current | Add |
|---------|-----|
| `living`, `bedroom`, `kitchen`, `bathroom`, `office`, `classroom`, `corridor`, `staircase`, `escape_route`, `circulation`, `storage`, `garage`, `utility` | `lobby`, `laundry`, `mail_room`, `plant_room`, `waste`, `terrace`, `balcony`, `parking`, `mechanical`, `electrical`, `telecom`, `dining` |

These are standard building functions that appear in virtually every multi-storey project.

#### Space Grouping / Zones (Low Priority)

A mechanism to group spaces into logical units (apartment, suite, department) using a `zone:` property:

```markdown
#### Space: Living Room 101
  usage: living
  area: 25.0 m2
  zone: Unit 101

#### Space: Kitchen 101
  usage: kitchen
  area: 12.0 m2
  zone: Unit 101
```

All spaces sharing the same `zone:` value are grouped into an `IfcZone` in the IFC output.

**IFC mapping**: Compiler collects all unique `zone:` values and generates one `IfcZone` per value, grouping the referenced `IfcSpace` entities.

**Future extension**: If zone-level properties are needed (e.g., `type: dwelling`, `tenant:`, `total_area:`), a top-level `Zone:` declaration can be added that references the zone by name and attaches properties. The `zone:` property on spaces remains the linking mechanism.

#### Auto-ID Generation (Low Priority)

Allow the compiler to auto-generate element IDs when the ID is omitted:

```markdown
Wall:                              // auto-generates W-GF-001 (based on type + storey + sequence)
  from: (0, 0) to: (12, 0)
  height: 2.8 m

Wall:                              // auto-generates W-GF-002
  from: (12, 0) to: (19, 0)
  height: 2.8 m

Wall: W-GF-EXT-S                   // explicit ID — required when referenced by other elements
  from: (0, 0) to: (40, 0)
  height: 2.80 m

  Window: WIN-GF-01                // explicit ID on window (references host wall)
    offset: 1.5 m
    width: 2.0 m
```

**Auto-ID format**: `{TypePrefix}-{StoreyAbbrev}-{SequenceNumber}` (e.g., `W-GF-001`, `COL-F2-003`).

Bare `Type:` (no ID after the colon) triggers auto-generation. Explicit IDs remain required for elements that are referenced by other elements (e.g., walls hosting doors/windows, elements in `supports:`/`supported_by:` relationships, `from_storey:`/`to_storey:` targets).

For LLM authoring, managing hundreds of unique IDs is significant token overhead. Many elements don't need human-readable IDs — they just need unique identifiers for the IFC output.

**Implementation**: Parser accepts missing IDs (bare `Type:` with no identifier). Semantic analysis assigns auto-generated IDs before reference resolution. The compiler warns if an auto-ID element is referenced (since the generated ID is not stable across edits).

---

### 2.1″ Phase 7h: Compiler Quality (NOT STARTED)

**Goal**: Improve error messages, clean up grammar, and harden validation.

This phase can run independently at any time. No items implemented yet.

#### Error Message Improvements

- [ ] **Fuzzy storey reference matching**: When `from_storey: Ground floor` doesn't match `### Storey: Ground Floor`, suggest the closest match (case-insensitive Levenshtein distance)
- [ ] **Show provided properties on missing-property errors**: "Wall is missing required property `from`" should also show what properties *were* provided, to help spot typos like `axis:` instead of `from:`
- [ ] **Opening overflow validation**: Validate that `offset + width` fits within the host wall's length. Flag `offset: 9m` + `width: 2m` on a 10m wall as an error

#### Grammar Cleanup

- [ ] **Remove or implement ghost types**: `Opening`, `Grid`, and `Dimension` are accepted by the tree-sitter grammar but have no semantic model entries, no codegen, and no scene graph support. Either:
  - Remove them from the grammar (breaking change, but they were never functional)
  - Implement them as proper element types (if needed for v0.2 features)
- [ ] **Deprecation warnings**: If ghost types are removed, emit a helpful error: "Opening is not a supported element type. Did you mean Door or Window?"

#### Validation Enhancements

- [ ] **Cross-storey reference validation**: Validate that `from_storey` / `to_storey` references on stairs and elevators point to existing storeys
- [ ] **Material reference validation**: Warn when `material:` references a material ID that isn't defined
- [ ] **Coordinate bounds checking**: Warn when element coordinates are far outside the building footprint (likely a unit or digit error)

---

### 2.2 Phase 7b: Web Architecture & Hosting (~85% COMPLETE)

**Goal**: Sustainable web platform that works on desktop and phone.

> **Status**: Web modularization complete — React + Vite + TypeScript app with shared renderer/floorplan modules, DataProvider pattern, Zustand stores, VS Code IIFE bundle, section cut feature, full 2D floorplan editor. Remaining: CodeMirror code editor, hosting/deployment, responsive layout.

#### 2.2.1 Evaluation Spike (RESOLVED)

Framework decisions made and implemented:

| Category | Decision | Rationale |
|----------|----------|-----------|
| **3D rendering** | Custom WebGL (kept) | Works well, bundle size minimal, section cut via shader. Three.js deferred to Phase 7f for gizmos. |
| **2D canvas** | Custom Canvas 2D (kept) | Full floorplan editor working with selection, drag, snap, undo/redo. |
| **Code editor** | **TODO**: CodeMirror 6 | Current textarea with debounced linting. CodeMirror not yet integrated. |
| **Framework** | React + Vite + Zustand | React 19.2, Zustand 5.0 for state, Vite for build. Path aliases (@shared/, @components/, @state/). |

#### 2.2.2 Modularization (COMPLETE)

- [x] Split monolithic HTML files → 6 shared renderer modules + 7 shared floorplan modules
- [x] Separate concerns: rendering (`web/src/shared/renderer/`), state (`web/src/state/`), UI (`web/src/components/`), BML integration (`web/src/shared/providers/`)
- [x] Shared WASM initialization via `WasmProvider`, REST via `RestProvider`, VS Code via `VsCodeProvider`
- [x] CSS extracted from inline styles
- [x] Legacy HTML preserved at `web/viewer.legacy.html` and `web/floorplan.legacy.html`

#### 2.2.3 Code Editor (TODO)

Replace the current `<textarea>` (in `web/src/components/editor/CodeEditor.tsx`) with a proper code editor:

- [ ] Syntax highlighting for BML (reuse TextMate grammar or write CodeMirror language package)
- [ ] Basic autocomplete (element types, property names)
- [ ] Error markers from lint results
- [ ] Line numbers, bracket matching
- [ ] Mobile-friendly (collapsible, resizable)

#### 2.2.4 Hosting Setup (TODO)

```
buildingml.example.com
├── /                    → Web editor (2D + 3D + code editor)
├── /api/scene           → Compile BML → scene JSON (fallback if WASM unavailable)
├── /api/lint            → Lint BML source (fallback)
├── /api/save            → Persist project files
├── /api/projects        → List/manage saved projects
└── /api/compile         → Compile BML → IFC (download)
```

**Infrastructure**:

- **Server**: VPS
- **Reverse proxy**: Caddy (add to existing Caddyfile)
- **Deployment**: Docker Compose
  - `frontend` service: static files (Caddy serves directly)
  - `buildingml-server` service: Rust binary for API endpoints
  - Shared volume for project file persistence
- **CI/CD**: GitHub Actions — build + deploy on push to `main` (SSH deploy)
- **WASM-first**: Client-side compilation for all operations; server API as fallback only
- **File persistence**: Server-side project storage with user sessions (simple, no auth initially)

#### 2.2.5 Responsive Layout (TODO)

- **Desktop**: Side-by-side panels (editor | viewer) with resizable splitter
- **Tablet**: Same layout, collapsible editor panel
- **Phone**: Single panel with bottom toolbar to switch between views
  - Viewer mode (read-only 3D/2D) is priority for phone
  - Editor mode available on tablet+ (needs keyboard)
- Touch events: tap to select, drag to pan, pinch to zoom, two-finger rotate (3D)
- Collapsible side panels become bottom sheets on narrow screens

---

### 2.3 Phase 7c: Enhanced Geometry & Irregular Shapes

**Goal**: Support non-rectangular geometry throughout the compiler pipeline.

#### Arbitrary Polygon Spaces

Rooms defined by explicit boundary or inferred from wall topology:

```markdown
#### Space: L-Shaped Office
  boundary: [(0,0), (6,0), (6,3), (3,3), (3,5), (0,5)]
  height: 2.8 m
```

When `boundary` is omitted but the space is enclosed by walls, the compiler infers the boundary polygon from wall intersection analysis.

#### Slab-Follows-Space

When a room is reshaped (boundary changes), its floor slab adjusts automatically:

- Slab with `boundary: auto` inherits the space boundary
- Explicit slab boundaries remain independent
- IFC output: `IfcExtrudedAreaSolid` with `IfcArbitraryClosedProfileDef`

#### Tessellated Cylinders

N-sided polygon approximation for circular profiles:

- Configurable segment count (default 16, via `buildingml.toml`)
- Used for: circular columns, pipe cross-sections, furniture cylinder primitives
- Scene graph: new `cylinder_mesh(radius, height, segments)` primitive

#### Arc Segments

For curved wall sections and other arc geometry:

- BML syntax: `arc: center (cx, cy) radius r from_angle to_angle` (or `from: (x1,y1) via: (mx,my) to: (x2,y2)`)
- IFC: `IfcTrimmedCurve` on `IfcCircle`
- Scene graph: tessellated arc extrusion

#### Polygon Holes

Interior voids in slabs, roofs, and floors (courtyards, stairwell openings):

```markdown
Slab: SLAB-GF
  boundary: [(0,0), (12,0), (12,10), (0,10)]
  holes: [[(4,3), (8,3), (8,7), (4,7)]]
  thickness: 0.25 m
```

IFC: `IfcArbitraryProfileDefWithVoids`

#### New Scene Graph Primitives

| Primitive | Function | Used By |
|-----------|----------|---------|
| `cylinder_mesh()` | Tessellated cylinder | Circular columns, pipes, furniture |
| `arc_extrude()` | Arc-swept extrusion | Curved walls |
| `polygon_with_holes()` | Polygon with interior voids | Slabs, roofs with openings |

---

### 2.4 Phase 7d: Furniture Asset System

**Goal**: Define reusable geometry components in BML, place instances in buildings.

This is a major language extension. See **Section 3** for the full syntax design.

#### Summary

- **Asset definitions**: Multi-part geometry assemblies defined in their own heading hierarchy
- **Asset libraries**: Dedicated `.bml` files for reusable asset collections
- **Asset placement**: `Furniture: ID` with `asset:` reference, position, and rotation
- **Geometry primitives**: `box()`, `cylinder()`, `extrude()`, `import()` for part definitions
- **IFC mapping**: Asset → `IfcFurnishingElementType`, Part → shape representation component, Placement → `IfcFurnishingElement` instance

---

### 2.5 Phase 7e: Real IFC Import & Priority Analysis

**Goal**: Test the decompiler against real-world IFC files to identify priority features.

#### Tasks

- [ ] **Collect open IFC datasets**: buildingSMART sample files, OSArch open models, IFC model repositories
- [ ] **Run decompiler on real files**: Measure entity coverage (% of entities successfully mapped to BML)
- [ ] **Frequency analysis**: Rank IFC entity types by occurrence frequency across the dataset
- [ ] **Gap identification**: Compare current element support against the 80% coverage threshold
- [ ] **Build test suite**: Select representative real-world files as regression tests
- [ ] **Schema documentation**: Query IFC4 schema docs for unsupported entity definitions → generate task list ordered by priority

#### Expected Outcome

A prioritized list of IFC entity types to implement, based on real-world frequency rather than spec completeness. This informs which elements to add in future phases.

---

### 2.6 Phase 7f: Interactive Editing

**Goal**: Move from read-only 3D viewer to interactive 3D+2D editing.

#### Element Selection

- **3D view**: Raycasting/picking — click on a mesh to select the corresponding BML element
- **2D view**: Already partially implemented (floorplan editor) — extend with element tree
- **Highlight**: Selected element highlighted in both views simultaneously

#### Transform Gizmos

- Translate gizmo (arrows for X/Y/Z axis movement)
- Rotate gizmo (ring for Y-axis rotation, relevant for furniture)
- Likely requires Three.js or similar library with gizmo support (evaluation in 7b)

#### Property Editing

- Selecting an element in 3D/2D opens a property panel
- Edit properties → BML source updates → views re-render
- Numerical inputs with unit awareness

#### Hierarchical Element Tree

- Tree structure in the floorplan/editor panel showing: Site → Building → Storey → Space → Elements
- Expand/collapse nodes
- Click to select and zoom to element
- Drag to reorder (moves element between spaces/storeys)

#### Bi-Directional Sync

The core challenge: keeping three representations in sync.

```
BML Source ←→ 2D Editor ←→ 3D Viewer
     ↑              ↑            ↑
     └──── all three stay in sync ────┘
```

- Edit in code → 2D and 3D update
- Drag in 2D → code and 3D update
- Transform in 3D → code and 2D update

#### Undo/Redo

- Unified undo stack across all views
- Operations: property changes, element moves, element creation/deletion
- Undo in any view reverts the change everywhere

---

### 2.7 Phase 7g: Chatbot Integration

**Goal**: LLM assistant that reads/writes BML while the user edits visually.

#### UI

- Collapsible chat panel (sidebar on desktop, bottom sheet on mobile)
- Streaming responses for natural conversation feel
- Chat history preserved per project session

#### Workflow

1. User opens a project in the web editor
2. Chat panel shows the LLM context: current BML state, selected element, recent changes
3. User types: "Add a bathroom next to the kitchen with a shower and toilet"
4. LLM generates BML modifications → preview updates live
5. User adjusts visually → BML updates → chat context stays current
6. LLM can explain: "The bathroom is 4m² because the remaining space between Kitchen and Wall W-03 is 2m × 2m"

#### Technical Approach

- **API**: Claude API (or similar) with streaming responses
- **Context management**: Send relevant BML excerpt (current storey, selected space) not entire project
- **Change application**: LLM outputs BML diff or replacement block → compiler validates → views update
- **Error handling**: If LLM-generated BML has errors, show lint results in chat and ask for correction
- **Phase 6 results**: LLM evaluation benchmarks inform prompt engineering and few-shot examples

#### Works in Both Views

- In 3D view: "Rotate this building 45 degrees", "Make all exterior walls thicker"
- In 2D view: "Add a door between the kitchen and dining room", "Move this wall 0.5m north"
- Chat understands spatial context from whichever view is active

---

## 3. Furniture Asset Syntax (Full Design)

### 3.1 Asset Definition

Assets are defined using a parallel heading hierarchy. They can live in dedicated `.bml` files (asset libraries) or inline within a project.

```markdown
# AssetLibrary: Office Furniture
  description: Standard office furniture collection
  version: 1.0

## Asset: OfficeDesk
  description: Standard office desk with drawer unit
  category: desk
  /// Bounding box dimensions (for placement preview)
  width: 1.6 m
  depth: 0.8 m
  height: 0.75 m

  ### Part: Desktop
    geometry: box(1.6, 0.8, 0.03)
    position: (0, 0, 0.72)
    material: oak_veneer

  ### Part: LeftLeg
    geometry: box(0.04, 0.6, 0.72)
    position: (0.02, 0.1, 0)
    material: steel_chrome

  ### Part: RightLeg
    geometry: box(0.04, 0.6, 0.72)
    position: (1.54, 0.1, 0)
    material: steel_chrome

  ### Part: DrawerUnit
    geometry: box(0.4, 0.5, 0.5)
    position: (1.1, 0.15, 0.22)
    material: oak_veneer

## Asset: OfficeChair
  category: seating
  width: 0.65 m
  depth: 0.65 m
  height: 1.2 m

  ### Part: Seat
    geometry: box(0.45, 0.45, 0.05)
    position: (0.1, 0.1, 0.45)
    material: fabric_black

  ### Part: Base
    geometry: cylinder(0.02, 0.45)
    position: (0.325, 0.325, 0)
    material: steel_chrome

  ### Part: Backrest
    geometry: box(0.45, 0.03, 0.45)
    position: (0.1, 0.07, 0.50)
    material: fabric_black
```

#### Heading Level Mapping (Asset Context)

| Heading | Meaning | Scope |
|---------|---------|-------|
| `#` | `AssetLibrary` | Top-level collection |
| `##` | `Asset` | Single reusable component type |
| `###` | `Part` | Geometry part within an asset |

The parser distinguishes asset context from building context by the heading type keyword (`AssetLibrary` / `Asset` / `Part` vs `Site` / `Building` / `Storey`).

### 3.2 Geometry Primitives for Parts

| Primitive | Syntax | Description |
|-----------|--------|-------------|
| Box | `geometry: box(w, d, h)` | Rectangular cuboid (width, depth, height) |
| Cylinder | `geometry: cylinder(r, h)` | Cylinder, tessellated to N segments (default 16) |
| Extrude | `geometry: extrude([(x,y), ...], h)` | Arbitrary polygon extruded to height |
| Import | `geometry: import("path.glb")` | External 3D model (glTF/GLB format) |

Each part also has:
- `position: (x, y, z)` — offset relative to the asset origin
- `rotation: degrees` — Y-axis rotation (optional, default 0)
- `material: material_id` — reference to a material definition

#### Future Primitives (post-v0.2)

- `sphere(r)` — sphere
- `cone(r, h)` — cone
- `torus(R, r)` — torus
- Boolean operations: `subtract(A, B)`, `union(A, B)`, `intersect(A, B)`

### 3.3 Asset Placement in Buildings

Assets are placed within spaces using the existing `Furniture:` element type, extended with an `asset:` property:

```markdown
#### Space: Office 101
  area: 25 m2

  Furniture: DESK-01
    asset: OfficeDesk
    position: (2.0, 1.5)
    rotation: 90

  Furniture: CHAIR-01
    asset: OfficeChair
    position: (2.5, 2.0)
    rotation: 180
```

- `asset:` references the Asset ID (case-sensitive, matches `## Asset: <ID>`)
- `position:` is 2D (x, y) within the storey coordinate system; Z is floor level
- `rotation:` is degrees around the vertical axis (clockwise from above)

If `asset:` is omitted, the furniture element behaves as in v0.1 (bounding-box stub).

### 3.4 Asset Import

Asset libraries are imported like any other BML file:

```markdown
import assets from "./assets/office-furniture.bml"
import assets from "./assets/bathroom-fixtures.bml"
```

Assets are resolved by name across all imported libraries. Name collisions are a compile error.

### 3.5 IFC Mapping

| BML Concept | IFC Entity |
|-------------|-----------|
| `AssetLibrary` | Project-level type library grouping |
| `Asset` | `IfcFurnishingElementType` (type definition) |
| `Part` | `IfcShapeRepresentation` component within the type |
| Furniture placement | `IfcFurnishingElement` instance with `IfcLocalPlacement` |
| Part geometry (box) | `IfcExtrudedAreaSolid` with `IfcRectangleProfileDef` |
| Part geometry (cylinder) | `IfcExtrudedAreaSolid` with `IfcCircleProfileDef` |
| Part geometry (extrude) | `IfcExtrudedAreaSolid` with `IfcArbitraryClosedProfileDef` |
| Part geometry (import) | `IfcFacetedBrep` or external reference |

### 3.6 Scene Graph Rendering

- Asset instances are expanded into their parts during scene generation
- Each part becomes a `SceneMesh` with proper transform:
  - Asset placement position + part offset position + rotation
  - Transform order: part local → asset rotation → asset position → storey elevation
- Part colors derived from material definitions or element-type defaults
- Bounding box wireframe overlay for asset selection in editor

---

## 4. Round Geometry & MEP Preview

### 4.1 Round Shapes (v0.2)

Circular geometry is needed across multiple features:

| Use Case | Geometry | Segment Count |
|----------|----------|---------------|
| Circular column profiles | Tessellated cylinder | 16 (configurable) |
| Furniture cylinder parts | Tessellated cylinder | 16 |
| Pipe cross-sections (future) | Swept disk solid | 12 |
| Arc wall segments | Tessellated arc extrusion | Proportional to arc angle |

Configuration in `buildingml.toml`:

```toml
[geometry]
circle_segments = 16     # Default tessellation for circular profiles
arc_segments_per_90 = 8  # Arc tessellation density
```

### 4.2 MEP Preview (Future — Documented for Planning)

MEP (Mechanical, Electrical, Plumbing) is not in scope for v0.2, but the geometry primitives being added (cylinders, arcs) lay the groundwork. Documenting the direction here for future reference.

#### Planned BML Syntax Direction

```markdown
### System: Domestic Hot Water

Pipe: P-DHW-01
  from: (2.0, 3.0, 0.5) to: (2.0, 3.0, 2.8)
  diameter: 0.022 m
  material: copper
  system: dhw

Pipe: P-DHW-02
  from: (2.0, 3.0, 2.8) to: (5.0, 3.0, 2.8)
  diameter: 0.022 m
  material: copper
  system: dhw

Fitting: F-DHW-01
  type: elbow_90
  connects: P-DHW-01, P-DHW-02
```

#### IFC Entities

- `IfcPipeSegment` with `IfcSweptDiskSolid` (circular cross-section swept along path)
- `IfcDuctSegment` with rectangular or circular profiles
- `IfcPipeFitting`, `IfcDuctFitting` for elbows, tees, reducers
- `IfcFlowTerminal` for fixtures (sinks, toilets, radiators)

#### Rendering

- Pipes: cylinders with proper bends at fittings
- Ducts: rectangular or circular extrusions
- Color-coded by system (DHW = red, cold water = blue, drainage = grey)

---

## 5. Mobile & Responsive Design

### 5.1 Design Principles

- **Progressive capability**: Phone gets viewer, tablet gets viewer + basic editing, desktop gets full experience
- **Touch-first for viewers**: All gesture interactions designed for touch, mouse is secondary on viewers
- **Keyboard-required for code**: BML code editing requires a keyboard (tablet+ only)
- **Offline-capable**: WASM compilation runs client-side, no server dependency for core features

### 5.2 Layout Breakpoints

| Breakpoint | Layout | Capabilities |
|------------|--------|-------------|
| **Phone** (< 768px) | Single panel, bottom toolbar | View 3D/2D, switch views, chat, read-only code |
| **Tablet** (768–1024px) | Collapsible split panels | Full editing, collapsible editor panel |
| **Desktop** (> 1024px) | Side-by-side with resizable splitter | Full experience, multi-panel layout |

### 5.3 Touch Events

| Gesture | 3D Viewer | 2D Editor |
|---------|-----------|-----------|
| Tap | Select element | Select element |
| Drag (one finger) | Orbit / rotate | Pan canvas |
| Pinch | Zoom | Zoom |
| Two-finger drag | Pan | — |
| Two-finger rotate | Rotate camera | — |
| Long press | Context menu | Context menu |

### 5.4 Panel Behavior on Mobile

- Side panels (element tree, properties, chat) become bottom sheets
- Bottom sheets are draggable: collapsed (peek), half-height, full-height
- Only one bottom sheet open at a time
- Bottom toolbar with icons: 3D view, 2D view, code, chat, properties

---

## 6. Hosting Architecture

### 6.1 Infrastructure

```
                    ┌─────────────────────────┐
                    │     VPS                  │
                    │                          │
  Internet ───────▶ │  Caddy (reverse proxy)   │
                    │    └─ buildingml.example.com ──▶ Docker Compose
                    │                          │        ├─ frontend (static)
                    │                          │        └─ buildingml-server
                    └─────────────────────────┘
```

### 6.2 Docker Compose Services

```yaml
# docker-compose.yml (conceptual)
services:
  frontend:
    # Static files served by Caddy directly
    # Built WASM, JS modules, HTML, CSS
    volumes:
      - ./dist:/srv/buildingml

  buildingml-server:
    build: .
    # Rust binary for server-side API
    # Only needed for: file persistence, IFC download, sharing
    ports:
      - "3001:3001"
    volumes:
      - projects-data:/data/projects

volumes:
  projects-data:
```

### 6.3 Caddy Configuration

```
buildingml.example.com {
    handle /api/* {
        reverse_proxy buildingml-server:3001
    }
    handle {
        root * /srv/buildingml
        file_server
        try_files {path} /index.html
    }
}
```

### 6.4 CI/CD Pipeline

```
Push to main
  └─▶ GitHub Actions
       ├─ cargo test
       ├─ cargo clippy
       ├─ Build WASM (wasm-pack)
       ├─ Build frontend (bundle JS/CSS)
       ├─ Build Docker image
       └─ SSH deploy to VPS
            ├─ docker compose pull
            └─ docker compose up -d
```

### 6.5 WASM-First Architecture

The server exists primarily for persistence, not computation:

| Operation | Client (WASM) | Server (API) |
|-----------|--------------|--------------|
| Parse BML | Primary | — |
| Lint | Primary | Fallback |
| Compile to scene | Primary | Fallback |
| Compile to IFC | Primary | Download endpoint |
| Save project | — | Primary |
| Load project | — | Primary |
| Share link | — | Primary |

This means the web editor works offline (except save/load) and scales without server load.

---

## 7. Phase 6 Relationship

Phase 6 (LLM Evaluation) from the v0.1 roadmap remains relevant and informs Phase 7g:

- Token efficiency benchmarks → optimize BML syntax for LLM generation
- Success rate metrics → identify which element types LLMs struggle with → improve docs/examples
- Edit distance analysis → measure how close LLM output is to valid BML → inform error recovery
- Benchmarks vs ifcJSON/IfcOpenShell → validate BuildingML's value proposition

Phase 6 can run independently of v0.2 implementation phases.

---

## 8. Floor Template Syntax (Full Design)

### 8.1 Problem Statement

A 6-storey residential building with 5 identical floors requires ~2,750 lines of duplicated BML (550 lines × 5 floors). Each floor has the same spaces, walls, doors, windows, and structural elements — only the storey name, elevation, and element IDs differ. This duplication is:

- **Token-inefficient**: ~37K tokens for a fully-detailed 48-unit building, where templates could reduce it to ~12K
- **Error-prone**: Copy-paste introduces ID collisions and inconsistent modifications
- **LLM-hostile**: Generating 5 identical copies wastes context window and increases error accumulation

### 8.2 Template Definition

Templates are defined at the top level using a `Template:` heading. They contain spaces and elements with placeholder variables in `{braces}`:

```markdown
Template: TypicalResidentialFloor

  #### Space: Corridor {F}
    usage: corridor
    area: 72.0 m2

  #### Space: Unit {F}01
    usage: living
    area: 95.0 m2

    Wall: W-{F}-01-S
      from: (0, 0) to: (12, 0)
      height: 2.80 m
      thickness: 0.24 m
      material: exterior_wall_layered
      is_external: true

      Window: WIN-{F}-01
        offset: 1.5 m
        width: 2.0 m
        height: 1.6 m
        sill_height: 0.80 m

    Wall: W-{F}-01-E
      from: (12, 0) to: (12, 8)
      height: 2.80 m
      thickness: 0.20 m

    Slab: SLAB-{F}-01
      boundary: [(0,0), (12,0), (12,8), (0,8)]
      thickness: 0.25 m
      material: reinforced_concrete

  #### Space: Unit {F}02
    usage: living
    area: 78.0 m2
    ...

  Column: COL-{F}-01
    position: (8.0, 9.0)
    profile: rectangle(0.3, 0.3)
    height: 2.80 m
```

#### Template Variables

- Variables use `{name}` syntax (single braces, like shell variables)
- Variables can appear in: element IDs, property values, space names
- Variables are simple string substitution (no expressions, no conditionals)
- Convention: `{F}` for floor identifier, `{N}` for unit number, but any name is allowed

### 8.3 Template Application

Templates are applied to storeys using `apply_template:`:

```markdown
### Storey: Floor 2
  elevation: 6.50 m
  height: 3.00 m
  apply_template: TypicalResidentialFloor
    F: F2

### Storey: Floor 3
  elevation: 9.50 m
  height: 3.00 m
  apply_template: TypicalResidentialFloor
    F: F3

### Storey: Floor 4
  elevation: 12.50 m
  height: 3.00 m
  apply_template: TypicalResidentialFloor
    F: F4
```

- `apply_template:` references the Template ID (case-sensitive)
- Variable bindings are indented key-value pairs below `apply_template:`
- All variables used in the template must be bound (compiler error if missing)
- A storey can have both `apply_template:` and additional elements (the additional elements are appended)

### 8.4 Simpler Alternative: `same_as` Directive

For cases where the floors are truly identical (no variable customization needed):

```markdown
### Storey: Floor 3
  elevation: 9.50 m
  same_as: Floor 2
```

- Copies all spaces and elements from the referenced storey
- Auto-prefixes IDs: replaces the source storey's abbreviation with the target's (e.g., `W-F2-01` → `W-F3-01`)
- Auto-renames spaces: replaces floor reference in space names
- Cannot customize individual elements (use full templates for that)

**Implementation**: `same_as` is syntactic sugar — the compiler expands it to the equivalent of `apply_template` with auto-derived variable bindings.

### 8.5 Template Files

Templates can live in separate files and be imported:

```markdown
import templates from "./templates/residential-floor.bml"
```

A template file contains one or more `Template:` definitions. Templates are resolved by name across all imported files.

### 8.6 Processing Model

Templates are expanded **before** semantic analysis:

```
Parse → Template Expansion → Semantic Analysis → IR → Codegen
```

1. **Parse**: Templates are parsed into AST nodes (`TemplateDefinition`, `TemplateApplication`)
2. **Expand**: Each `apply_template:` is replaced with the template body, variables substituted
3. **Analyze**: The expanded AST is analyzed as if the elements were written inline

This means:
- Templates don't create a new scoping mechanism
- Expanded elements participate in normal reference resolution
- Error messages point to the expanded location (with a "from template X, line Y" note)

### 8.7 Quantitative Impact

| Metric | Without Templates | With Templates | Reduction |
|--------|------------------|---------------|-----------|
| 6-storey building (lines) | ~4,500 | ~1,200 | 73% |
| Element IDs to manage | 326 | ~80 + auto | 75% |
| Token estimate | ~37,000 | ~12,000 | 68% |
| LLM generation turns | 3–4 | 1 | 75% |

---

## 9. Open Questions

### Resolved

1. **Web framework choice**: **React + Vite + Zustand** — implemented in Phase 7b.
2. **Three.js vs custom WebGL**: **Custom WebGL kept** for viewer. Three.js may be adopted in Phase 7f for transform gizmos.
7. **Template variable syntax**: **`{F}` single braces** — implemented, no conflicts encountered.
8. **`same_as` vs full templates**: **Templates only** implemented so far. `same_as` deferred — can be added as syntactic sugar later.
9. **Auto-ID prefix derivation**: **Counter-based** format `{TypePrefix}-{StoreyAbbrev}-{SeqNum}` (e.g., W-GF-001). Implemented in Phase 7a′.
10. **Zone property vs Zone declaration**: **`zone:` property on spaces** implemented. Top-level `Zone:` declaration deferred to future need.

### Still Open

3. **Furniture asset file format**: Should `.bml` files with `AssetLibrary` headings use a different extension (`.bml-assets`)? Leaning toward keeping `.bml` for simplicity.
4. **glTF import pipeline**: How to embed imported 3D models in IFC output? Needs research into `IfcFacetedBrep` conversion.
5. **Authentication for hosted version**: Deferred. Start with anonymous sessions, add auth later if needed.
6. **Real-time collaboration**: Not in v0.2 scope but hosting architecture should not preclude it.

---

## Appendix A: Glossary

| Term | Definition |
|------|-----------|
| **BML** | BuildingML file format (.bml) |
| **IFC** | Industry Foundation Classes — ISO 16739 standard for BIM data exchange |
| **CST** | Concrete Syntax Tree — direct output of tree-sitter parser |
| **AST** | Abstract Syntax Tree — typed Rust structs |
| **IR** | Intermediate Representation — `SemanticModel` after analysis |
| **STEP** | Standard for the Exchange of Product Data — IFC file encoding format |
| **MEP** | Mechanical, Electrical, Plumbing building systems |
| **BREP** | Boundary Representation — solid geometry format |
| **glTF/GLB** | GL Transmission Format — 3D model format for web |

## Appendix B: Related Documents

- `Specs.md` — BuildingML v0.1 language specification
- `CLAUDE.md` — Developer guide for Claude Code
- `docs/lessons-learned.md` — Implementation lessons from v0.1
