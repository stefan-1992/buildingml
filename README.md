# BuildingML

A domain-specific language (DSL) for describing Building Information Models (BIM) in a markdown-inspired format. BuildingML bridges LLM-generated text and IFC (Industry Foundation Classes) building standards.
**Status**: V0.1 complete (Phases 1–5), V0.2 nearly complete (~90%, Phases 7a–7j), V0.3 planning started (Phases 8a–8g: refactoring, LLM features, spec-to-check generation, regulation libraries). Features: tree-sitter grammar, parser, semantic analysis, IFC codegen (all 15 element types with proper geometry, property sets, pitched roofs, curved walls, polygon holes), CLI (compile, lint, check, decompile, analyze, lsp, preview, scene, model, render, serve), WASM bindings, LSP server with VS Code extension (3D/2D/IFC preview with shared renderer, IFC export command), compliance checking engine, 3D scene graph generator, React web app with WebGL viewer and interactive 2D floorplan editor (hierarchical element tree, bi-directional BML<->2D sync, CodeMirror 6 code editor with syntax highlighting, unified undo/redo), 3D element selection (click-to-pick with raycasting/color-buffer picking, highlight shader, metadata properties panel, TransformControls gizmo), SVG rendering, section cut feature, DataProvider adapter pattern, floor templates, elevator type, auto-ID generation, space zones, furniture asset system with geometry primitives, IFC import coverage analysis, chatbot integration with LLM-powered building design assistant, IFC export & preview via web-ifc WASM parser, and responsive mobile design (phone/tablet/desktop breakpoints, bottom sheet panels, bottom navigation, long-press context menu, touch gestures).

## Why BuildingML?

IFC files are notoriously verbose and complex (STEP format, deeply nested entity graphs). A simple wall can be hundreds of lines in IFC. In BuildingML, it's 5 lines. This makes it feasible for LLMs to generate building descriptions, and for humans to read and edit them — while still producing standards-compliant IFC output for industry tools.

## How It Works

BuildingML uses a markdown-inspired syntax where **heading levels map to the IFC spatial hierarchy**, elements are declared with a `Type: ID` pattern, and properties are indented key-value pairs.

```
# Site: Example Residence                    ← IfcSite
  location: 49.4521, 12.1803

## Building: Single Family House             ← IfcBuilding
  type: residential

### Storey: Ground Floor                     ← IfcBuildingStorey
  elevation: 0.0 m

#### Space: Living Room                      ← IfcSpace
  usage: living
  area: 28.0 m2

  Wall: W-GF-01                              ← Building element with ID
    from: (0, 0) to: (6, 0)                  ← Geometry (wall axis line)
    height: 2.8 m                            ← Properties with units
    thickness: 0.24 m
    material: exterior_wall_layered
    is_external: true

    Window: WIN-01                           ← Nested inside the wall
      offset: 1.0 m
      width: 2.0 m
      height: 1.6 m
      sill_height: 0.80 m
      type: triple_glazed

  Slab: SLAB-GF
    boundary: [(0,0), (10,0), (10,8), (0,8)]
    thickness: 0.25 m
    material: reinforced_concrete

  Column: COL-01
    position: (5.0, 4.0)
    profile: rectangle(0.3, 0.3)
    height: 2.80 m
    material: reinforced_concrete
```

| Heading | IFC Entity        | Scope          |
|---------|-------------------|----------------|
| `#`     | IfcSite           | Project-level  |
| `##`    | IfcBuilding       | Site-level     |
| `###`   | IfcBuildingStorey | Building-level |
| `####`  | IfcSpace          | Storey-level   |

Elements written directly under `#` or `##` (before the first child heading) are contained in the site or building; a `####` space may also sit directly under a site or building. Floorplans and the 2D editor show these contents as an extra plan per building or site.

Supported element types: Wall, Slab, Window, Door, Column, Beam, Roof, Stair, Railing, Ramp, Furniture (with reusable asset geometry), Elevator, Proxy (IfcBuildingElementProxy), Member (IfcMember), Plate (IfcPlate), Opening (IfcOpeningElement), Grid (IfcGrid), Covering (IfcCovering), Footing (IfcFooting), Duct, Pipe, CableTray, Terminal, Equipment, Accessory and System (building services, `Specs_V0_3.md` §24, §26).

Point lists take plan curves (`Specs_V0_3.md` §28): a `smooth` point puts a cubic spline through it, and a `nurbs { degree, points, weights, knots }` item gives an exact NURBS segment (weights default to 1, knots to clamped uniform). Walls, railings, beams and members follow a curved `path:`, and the IFC output keeps the exact B-spline.

```bml
  Wall: W-01
    path: [(1, 1), smooth (5, 2.5), smooth (9, 2), (13, 4)]
    height: 3 m
    thickness: 0.3 m

  Slab: SL-01
    boundary: [(0, 0), nurbs { degree: 2, points: [(10, 0), (12, 3), (10, 6)] }, (0, 6)]
    thickness: 0.25 m
```

## Compiler Pipeline

```
.bml files → Tree-sitter → CST → AST → Semantic Analysis → IR → IFC / 3D Scene / Compliance Report
```

```
  You / LLM writes .bml
         │
         ▼
  ┌──────────────┐
  │  Tree-sitter  │  ← Parses text into CST (with error recovery)
  └──────┬───────┘
         ▼
  ┌──────────────┐
  │   AST Pass    │  ← CST → typed Rust structs
  └──────┬───────┘
         ▼
  ┌──────────────┐
  │  Semantic     │  ← Type check, resolve refs, validate hierarchy
  │  Analysis     │     → produces SemanticModel (IR)
  └──────┬───────┘
         │
    ┌────┼────────────┐
    ▼    ▼            ▼
  ┌────┐ ┌─────┐  ┌────────┐
  │IFC │ │Scene│  │Checker │
  │Gen │ │Graph│  │Engine  │
  └────┘ └─────┘  └────────┘
    │       │          │
    ▼       ▼          ▼
  .ifc    WebGL     JSON/HTML
  file    viewer    report
```

The **IR (SemanticModel) is the single source of truth** — IFC generation, 3D visualization, and compliance checking all consume the same resolved model. Write once, get three outputs.

## Prerequisites

- **Rust** (stable toolchain) — install via [rustup](https://rustup.rs/)
- **C compiler** — required for the tree-sitter C parser (included with Xcode CLT on macOS, `build-essential` on Ubuntu)
- **wasi-sdk** — only needed if building the WASM target (not required for native builds)

## Getting Started

```bash
# Clone the repository
git clone https://github.com/stefan-1992/buildingml.git
cd buildingml

# Build the project
cargo build

# Run all tests
cargo test

# Create a new project (residential, office, or mixed-use templates)
# Nested mode (default): creates storeys/ subdirectory with per-storey files
# and a project.bml with top-level import statements for each storey file.
cargo run --bin buildingml -- init my-building --template residential
# Flat mode: all storey content is inlined directly in project.bml
cargo run --bin buildingml -- init my-office --template office --flat
# Note: --force only works on existing BuildingML projects (requires buildingml.toml)

# Compile a .bml file to IFC
cargo run --bin buildingml -- compile tests/fixtures/six_storey_residential.bml -o output.ifc

# Compile a multi-file project to IFC
cargo run --bin buildingml -- compile tests/fixtures/six_storey_residential_project/ -o output.ifc

# Larger example: fictitious university building (mensa, Audimax, offices, library)
cargo run --bin buildingml -- compile tests/fixtures/university_campus_project/ -o university.ifc

# Same building at LOD 300 (layered build-ups, curtain wall grids, u-shaped stairs, ceilings, footings)
cargo run --bin buildingml -- compile tests/fixtures/university_campus_lod300_project/ -o university_lod300.ifc

# Same building at LOD 350 (HVAC, plumbing, sprinkler, electrical routes with fittings, plant, terminals, generated openings, sleeves, hangers and brackets)
cargo run --bin buildingml -- compile tests/fixtures/university_campus_lod350_project/ -o university_lod350.ifc

# Same LOD 350 building with plan curves (wave curtain wall, flowing slab edges, pebble atrium, oval Audimax, curved library pavilion, curved ducts)
cargo run --bin buildingml -- compile tests/fixtures/university_campus_fluid_project/ -o university_fluid.ifc

# Lint a .bml file (syntax + semantic validation)
cargo run --bin buildingml -- lint tests/fixtures/spatial_hierarchy.bml

# Run compliance checks
cargo run --bin buildingml -- check tests/fixtures/compliance_checks.bml --format json

# Decompile an IFC file back to .bml (geometry fitted to each element's mesh)
cargo run --bin buildingml -- decompile output.ifc -o my-building/

# IFC -> BML -> IFC roundtrip report (needs ifcopenshell)
python3 scripts/ifc_roundtrip.py tests/fixtures/model_1.ifc

# Analyze IFC files for import coverage (entity frequency, BML mapping gaps)
cargo run --bin buildingml -- analyze tests/fixtures/model_1.ifc tests/fixtures/model_2.ifc tests/fixtures/model_3.ifc

# Render SVG floorplans and 3D views
cargo run --bin buildingml -- render tests/fixtures/two_storey_house.bml -o output/ --all

# Launch the 3D preview server (loads the file into the viewer)
cargo run --bin buildingml -- preview tests/fixtures/two_storey_house.bml --port 8080

# Generate semantic model JSON (used by VS Code 2D preview)
cargo run --bin buildingml -- model tests/fixtures/full_example.bml

# Generate 3D scene graph JSON (used by the VS Code 3D preview)
cargo run --bin buildingml -- scene tests/fixtures/full_example.bml

# Start the LSP server (used by the VS Code extension)
cargo run --bin buildingml -- lsp
```

## VS Code Extension

The VS Code extension provides full language support: syntax highlighting, diagnostics, completions, hover docs, go-to-definition, references, document outline, formatting, code actions, and a 3D/2D preview panel (toggle between 3D WebGL view and 2D floorplan view with cursor-driven storey selection).

### Setup

1. **Install the `buildingml` binary** so the extension can find the LSP server:

   ```bash
   cargo install --path crates/bml-cli
   ```

2. **Compile the extension**:

   ```bash
   cd editors/vscode
   npm install
   npm run compile
   ```

   If Review Pannnel needs updatting, run `cargo install --path crates/bml-cli` and then restart the vscode. shift + comd + p and select "Developer: Reload Window"

3. **Install in VS Code** — choose one:

   - **Symlink** (recommended for development — picks up recompiles automatically):
     ```bash
     ln -s "$(pwd)/editors/vscode" ~/.vscode/extensions/buildingml
     ```
   - **Extension Development Host** (for testing): Open `editors/vscode/` in VS Code and press `F5`

4. **Reload VS Code** (`Cmd+Shift+P` → "Developer: Reload Window"), then open any `.bml` file.

### Troubleshooting

If the extension reports `spawn buildingml ENOENT`, VS Code can't find the binary on its PATH (common on macOS where GUI apps don't inherit shell PATH). Fix by setting the absolute path in VS Code settings:

```json
{
  "buildingml.server.path": "/path/to/buildingml"
}
```

Find your path with `which buildingml`.

### Features

| Feature | Trigger |
|---------|---------|
| Completions | Type `#` (headings), `Wall:` (elements), or property names |
| Hover docs | Hover over any type, property, or unit |
| Go-to-definition | `Cmd+Click` on material, element, or storey references |
| Diagnostics | Automatic on save — syntax and semantic errors |
| Formatting | `Shift+Alt+F` — normalizes indentation |
| Code actions | Quick fixes for missing properties, unresolved materials |
| 3D/2D/IFC Preview | Preview button in editor title bar, or `Cmd+Shift+P` → "BuildingML: Open 3D Preview". Toggle 3D/2D/IFC in the preview. IFC mode renders the compiled IFC file via `web-ifc` for independent verification. 2D view auto-follows cursor to show the storey you're editing. |
| Element inspector | Click any element in the 3D preview to see its surface material (with colour swatch), identity (IFC type, name, GlobalId) and its IFC property sets and quantities, grouped by set |
| Material colours | BML and IFC previews use the same colours: a `Material:` block's `color: "#rrggbb"`, `opacity`, `specular`, `shininess`, else a palette matched on the material name (concrete, glass, oak, ...), else the element type default |
| Layered materials | `layers:` list of `- material: thickness` items compiles to an IFC material layer set with orientation (first layer outside / on top); lint warns when an element's `thickness` differs from the layer sum (`Specs_V0_3.md` §18) |
| Wall joins | Straight walls meeting at L corners are mitred, walls ending on another wall are cut to its face; writes `IfcRelConnectsPathElements` (`Specs_V0_3.md` §19) |
| Coverings | `Covering:` (ceilings, floor finishes) inside a space takes the space outline minus its walls and columns; a ceiling hangs at the space height; writes `IfcRelCoversSpaces` (`Specs_V0_3.md` §20) |
| Footings | `Footing:` strip footings take their outline from the walls they run `under:`, pad footings from `position:` and `width:`; body hangs down by `depth` (`Specs_V0_3.md` §21) |
| Curtain walls | A wall with `mullions:` (and `transoms:`), as a spacing or a list `[1.5 m, 3 m]`, becomes an `IfcCurtainWall` with generated mullions, transoms and glass panels (`Specs_V0_3.md` §22) |
| Stairs with landings | `type: l_shaped`/`u_shaped`, or `straight` with `landing_after:`, become an `IfcStair` with two generated `IfcStairFlight`s and a landing slab (`Specs_V0_3.md` §23) |
| Building services | `Duct:`, `Pipe:`, `CableTray:` with a `path:` route become segments, bends and tees with connected ports; `Terminal:`/`Equipment:` `type:` picks the IFC class (`IfcAirTerminal`, `IfcSanitaryTerminal`, `IfcPump`, ...); `system:` groups them into an `IfcDistributionSystem` (`Specs_V0_3.md` §24); runs cut openings in the walls and slabs they cross, `sleeve: true` lines them (§25); `hangers: 2 m` hangs them from the slab above with trapeze hangers, and `Accessory:` adds brackets, base plates and anchor bolts (§26); `connects: [C-01, FT-01]` writes connection relationships and `brackets: true` fixes curtain wall mullions to the slabs (§27) |
| Section cuts | `Plan` (horizontal) and `Section` (vertical) sliders in the 3D view, both off by default so the full model is always visible on open. A cut you set is remembered as a proportion of the model, so it stays where you put it when you switch to a building of a different size |
| IFC preview | Preview also works on a raw `.ifc` file, read by the native `buildingml ifc-scene` reader |
| Hide / isolate | `Hide` and `Isolate` on the inspector; `Filter` in the toolbar toggles whole IFC categories or storeys on and off, with a `Show all` reset |
| Export to IFC | `Cmd+Shift+P` → "BuildingML: Export to IFC" — compiles BML to IFC and saves to a file |
| Document outline | Spatial hierarchy in the sidebar |

## Project Structure

```
crates/
  tree-sitter-buildingml/   # Tree-sitter grammar + C external scanner
  bml-core/                 # Core library: parser, semantic analysis, codegen,
                            #   checker, decompiler, scene graph
  bml-cli/                  # CLI binary (compile, lint, check, decompile, render, lsp, preview, model)
  bml-lsp/                  # LSP server (tower-lsp)
  bml-wasm/                 # WASM bindings (lint, compile, scene generation)
editors/
  vscode/                   # VS Code extension (LSP client, 3D/2D preview via shared renderer)
    media/                  # Shared renderer IIFE bundle (3D renderer + 2D floorplan modules)
web/
  src/
    shared/                 # Framework-agnostic TypeScript modules
      renderer/             # Dual renderer (WebGL + Three.js), orbit camera, section cut, scene loader
      floorplan/            # Element model, space detection, drawing, BML generator
      providers/            # DataProvider implementations (REST, WASM, VS Code)
    components/             # React components (viewer, editor, floorplan, chat, layout)
    state/                  # Zustand stores (app, viewer, floorplan, chat)
  viewer.legacy.html        # Legacy WebGL viewer (kept during migration)
  floorplan.legacy.html     # Legacy 2D editor (kept during migration)
tests/fixtures/             # Example .bml files (incl. two_storey_house.bml)
scripts/
  check.sh                  # Run file size check + tests + clippy + IFC validation
  check_file_sizes.sh       # Fail if a tracked file exceeds 20 MB (EXCEPTIONS list in script)
  build.sh                  # Quick cargo check wrapper
  validate_ifc.py           # IfcOpenShell IFC validator (schema + geometry)
  validate_ifc_all.sh       # IFC writer definition-of-done
prototypes/
  usd_brep/                 # Exploratory: exact BRep geometry via NVIDIA USD BRep (see Specs_V0_3.md §14)
```

### Crate Details

**`tree-sitter-buildingml`** — A [tree-sitter](https://tree-sitter.github.io/) grammar (`grammar.js`) with a custom C external scanner (`scanner.c`) for indentation handling. Provides incremental parsing and error recovery.

**`bml-core`** — The core library with several stages:
- `parser/` — Converts tree-sitter CST to typed Rust AST structs (`Site`, `Building`, `Wall`, etc.)
- `semantic/` — Type checking, reference resolution, spatial containment validation → produces `SemanticModel` IR
- `codegen/` — IR → IFC4 STEP file via an own STEP writer (maps elements to IFC entities, properties to property sets, geometry to extruded solids)
- `checker/` — Compliance checking engine for first-class `Check:` blocks (building codes, fire safety, accessibility rules) with JSON/HTML/SARIF reports
- `scene/` — 3D scene graph generator (triangle meshes with box/polygon extrusion, per-face normals, material colours shared with the IFC preview)
- `render/` — SVG rendering: 2D floorplans with architectural conventions, 3D orthographic projections, section clipping
- `decompile/` — IFC → BuildingML reverse compiler; `mesh_fit.rs` fits walls, beams, columns or prisms to each element's mesh so the recompiled IFC matches the original

**`bml-lsp`** — Full LSP server: diagnostics, completion, hover, go-to-definition, references, document symbols, semantic tokens, formatting, code actions, and live 3D preview via VS Code webview panel.

**`bml-wasm`** — Compiles the full compiler to WebAssembly for browser use. Powers the web playground where you can write BML and see 3D output with no server.

## Preview Server

The `preview` command starts a local server with two modes:

**React app** (with `--web-dir web/dist`):
- **Unified app** (`http://localhost:8080/`) — React web app with 3D viewer, 2D floorplan editor, and code editor. Switch between views via toolbar. Section cut slider, wireframe toggle, undo/redo.
- **Example selector** — Add `--examples-dir tests/fixtures/` to serve a dropdown of `.bml` example files. The hosted version loads `six_storey_residential.bml` by default.

**Legacy mode** (without `--web-dir`):
- **3D Viewer** (`http://localhost:8080/`) — WebGL renderer with orbit camera, wireframe mode, and live editing.
- **Floorplan Editor** (`http://localhost:8080/floorplan`) — Interactive 2D editor for drawing walls, placing doors/windows/columns/slabs.

Legacy routes are always available at `/legacy/viewer` and `/legacy/floorplan`.

### Floorplan Editor Shortcuts

| Key | Action |
| --- | ------ |
| `V` | Select tool |
| `W` | Wall tool |
| `D` | Door tool |
| `N` | Window tool |
| `C` | Column tool |
| `S` | Slab tool |
| `G` | Toggle grid |
| `X` | Toggle snap |
| `F` | Zoom to fit |
| Arrow keys | Nudge selected element (snap size or 0.1m) |
| Shift+Arrow | Nudge 1m |
| `Delete` | Delete selected |
| `Cmd+Z` / `Cmd+Y` | Undo / Redo |
| `Cmd+D` | Duplicate selected |
| `Escape` | Deselect / cancel |

## SVG Rendering

The `render` command generates SVG files — architectural floorplans and 3D orthographic views — without a browser or GPU.

```bash
# All floorplans + all 3D views
buildingml render input.bml -o output/ --all

# Just floorplans
buildingml render input.bml -o output/ --floorplans

# Specific 3D views
buildingml render input.bml -o output/ --views front,iso

# Wireframe mode
buildingml render input.bml -o output/ --views iso --style wireframe

# Section cut at 3m elevation
buildingml render input.bml -o output/ --all --clip-elevation 3.0
```

Floorplans include section-cut walls, door/window symbols, room labels, and dimensions. 3D views support 6 angles (front, back, left, right, top, iso) with shaded or wireframe styles. SVG output is text-based XML, making it easy for LLMs to parse and analyze.

## Development

```bash
cargo test                     # Run all tests (190 tests)
cargo test <test_name>         # Run a single test
cargo clippy                   # Lint
cargo fmt                      # Format code
./scripts/check.sh             # Run file size check + tests + clippy + IFC validation
./scripts/build.sh             # Quick cargo check wrapper

# IFC validation (requires: pip install ifcopenshell)
./scripts/validate_ifc_all.sh  # Validate all fixtures (IFC writer DoD)
```

## Technology Stack

- **Language**: Rust (edition 2024)
- **Parser**: tree-sitter with custom external scanner for indentation
- **IFC Backend**: own STEP writer (IFC4 output)
- **CLI**: clap 4
- **LSP**: tower-lsp + tokio
- **WASM**: wasm-bindgen
- **3D Rendering**: Custom WebGL + Three.js (switchable at runtime)
- **Testing**: insta (snapshot testing), vitest (web frontend)

## V0.2 Roadmap

V0.2 (Phases 7a–7h) focuses on bringing BuildingML from a working compiler to a full web-based building design platform. See `Specs_V0_2.md` for the complete specification.

- **7a: IFC Codegen Hardening** — Proper geometry for all element types (columns, beams, stairs, doors, roofs)
- **7a': Language Improvements** — Floor templates (define once, stamp across storeys), storey-level elements, elevator/shaft type, auto-IDs (bare `Type:` without ID), `zone:` property for space grouping, extended usage enums
- **7b: Web Architecture** (~95%) — Web frontend modularized (React + Vite, shared modules, DataProvider pattern), CodeMirror 6 code editor (BML syntax highlighting, line numbers), mobile responsive design (read-only mode with touch pan/pinch-zoom, compact toolbar, full-screen chat overlay)
- **7c: Enhanced Geometry** — Arbitrary polygon spaces, curved walls, polygon holes, tessellated cylinders
- **7d: Furniture Asset System** ✅ — Define reusable multi-part geometry assets (`AssetLibrary`/`Asset`/`Part` headings), geometry primitives (`box()`, `cylinder()`, `extrude()`), place instances via `asset:` property on Furniture elements
- **7e: Real IFC Import** ✅ — Decompiler rewritten for real-world Revit IFC files, all 15 BML element types, spatial containment, unit conversion, property extraction, `buildingml analyze` CLI for coverage reporting
- **7f: Interactive Editing** (~95%) — Hierarchical element tree (Storey→Space→Elements), CodeMirror 6 BML code editor with bi-directional sync, property editing from 2D selection, unified undo/redo, VS Code 2D preview (3D/2D toggle, cursor-driven storey selection), 3D element selection (raycasting + color-buffer picking), selection highlight shader, metadata properties panel, TransformControls gizmo, 3D↔BML bi-directional sync
- **7g: Chatbot Integration** ✅ — Collapsible chat panel with LLM assistant that reads/writes BML while the user edits visually. Context-aware prompting (current storey, selected element, diagnostics), OpenAI-compatible API proxy with streaming, automatic BML extraction and application from responses. Configure via `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `OPENAI_MODEL` environment variables.
- **7h: Compiler Quality** ✅ — "Did you mean?" suggestions for unknown properties/element types (Levenshtein edit distance), codegen warning system (geometry approximations, skipped elements), multi-file diagnostic reporting. Remaining items completed in Phase 8a.
- **7j: Additional IFC Elements** ✅ — Proxy (IfcBuildingElementProxy), Member (IfcMember), Plate (IfcPlate) — full pipeline support for each

## V0.3 Roadmap

V0.3 (Phases 8a–8g) focuses on refactoring, LLM features, and compliance automation. See `Specs_V0_3.md` for the complete specification.

- **8a: Compiler Quality** ✅ — Fuzzy storey matching (Levenshtein + word-prefix), opening overflow validation, Opening/Grid element types (full pipeline), cross-storey reference validation, coordinate bounds checking (IQR outlier detection), linting rules (8 rules + TOML config, incl. coincident/duplicate wall detection), file_id in Span/Diagnostic, multi-location diagnostics
- **8b: Refactoring & Architecture** — Element type registry (`ElementTypeDescriptor` pattern to reduce 15-file changes to 1), codegen module decomposition, test infrastructure improvements
- **8c: LLM Evaluation & Benchmarks** — Token efficiency analysis (BML vs ifcJSON vs STEP), LLM generation success rate across models, edit distance analysis, benchmark infrastructure
- **8d: Spec-to-Check Generation** — LLM reads design specifications and auto-generates `Check:` blocks, validation pipeline, chat integration for spec upload and check review
- **8e: Regulation-to-Check Library** — Curated compliance checks from German MBO, International IBC, and Eurocode; check import mechanism; computed property extensions
- **8f: LLM Prompt & Context Improvements** — Semantic model context, compliance results in chat, few-shot examples, auto-retry on parse errors, structured output mode
- **8g: VS Code & Editor Polish** — VS Code 2D preview tab, quick fix code actions, compliance dashboard, regulation browser

Research: NVIDIA's open-source USD BRep kernel is being evaluated as an optional geometry backend. A prototype (`prototypes/usd_brep/`) builds exact BRep solids with boolean openings from BML. See `Specs_V0_3.md` §14.

## Language Specification

See `Specs.md` for the v0.1 language specification, including all syntax rules, element types, property definitions, compliance checking DSL, and design decisions. See `Specs_V0_2.md` for the v0.2 specification, `Specs_V0_3.md` for the v0.3 specification.

## License

MIT License — Copyright (c) 2026 Stefan Fuchs

The Revit IFC test models `tests/fixtures/model_1.ifc`, `model_2.ifc` and `model_3.ifc` come from the GNI BIM Dataset (Wang et al., 2026, [doi:10.5281/zenodo.19722012](https://doi.org/10.5281/zenodo.19722012)) and are licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
