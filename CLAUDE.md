# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Always update CLAUDE.md and README.md when making changes to the codebase, especially if they affect the architecture, technology stack, build commands, CLI commands, or implementation roadmap.

Limitations and future TODOs MUST go in the relevant section of Specs_V0_3.md — do NOT add implementation notes or limitations to this file, as they may become outdated. Reference that section here instead.

Consider if tests are useful for the code you're working on. Especially if doing bug fixes or refactors. If adding new features, tests are highly recommended.

## Definition of Done

Code review runs locally, not in CI (`ci.yml` only runs tests). Before a change is done (committed or PR opened):

1. `./scripts/check.sh` passes (plus `cd web && npm run test` if `web/` changed). It includes a 20 MB limit per tracked file; keep larger fixtures out of git
2. Run the `simplify` skill on the changes and apply its fixes
3. Run Claude Code's built-in `/code-review` on the changes and fix the confirmed findings. It takes an effort level (`low`, `medium`, `high`, `xhigh`, `max`) and remembers the last one typed, so pass it explicitly: `/code-review high` for the first pass, only `/code-review low` when re-running it on follow-up fixes
4. If steps 2-3 changed code, re-run step 1

For a BML model (fixture or example building), additionally:

- `buildingml lint` reports no errors and no warnings
- Every file lints clean on its own, not only the project: VS Code analyzes each open file with its imports, so a storey file must import the materials and assets it uses
- It renders in the VS Code preview, both the BML and its compiled IFC (`buildingml scene` / `buildingml ifc-scene` succeed on every file). VS Code runs the installed `buildingml` (`~/.cargo/bin`), not `target/`: run `cargo install --path crates/bml-cli` first and verify with that binary
- The IFC roundtrip works: `python3 scripts/ifc_roundtrip.py <model>.ifc`
- Deliver both the BML folder and the compiled `.ifc` file next to it

six_storey_residential.bml is a comprehensive test file covering all element types, relationships, and v0.2 features (templates, auto-IDs, elevator, zones, extended usage enums, railing axis positioning, storey-level elements). Floors 1-4 use `Template: TypicalResidentialFloor` with `{f}` and `{n}` variable substitution. Used for both unit tests and LLM evaluation. When making changes that affect parsing, semantic analysis, or code generation, ensure that this file is included in the test suite to prevent regressions. When adding new features, consider whether they can be expressed in six_storey_residential.bml or if a new test file is needed.

six_storey_residential_project/ is the multi-file equivalent of six_storey_residential.bml, split into 8 BML files across `storeys/`, `checks/`, and `assets/` directories. Each storey file redeclares `# Site` / `## Building` headings (required for per-file analysis). Templates must be co-located with `apply_template` directives. Used for multi-file project integration tests (9 tests).

university_campus_project/ is a fictitious 5-level university building (Forum Nord): mensa with a curved glass bulge, fan-shaped Audimax with raked tiers, lecture/seminar rooms, office floors via `Template: OfficeFloor` (L2/L3, 4 m cantilever), glazed library pavilion with roof terrace, roof slab with an atrium skylight lantern and PV plates. 8 BML files. `test_university_campus_project_clean` requires it to analyze without errors or warnings; it is also in `validate_ifc_all.sh`. Compiled IFC next to it: `university_campus_project.ifc`.

university_campus_lod300_project/ is the same building detailed to LOD 300 (contents listed in its `project.bml` header). Covered by `test_university_campus_project_clean` and `validate_ifc_all.sh`. Compiled IFC next to it: `university_campus_lod300_project.ifc`.

university_campus_lod350_project/ adds the services at LOD 350 to the LOD 300 building (contents listed in its `project.bml` header). Covered by `test_university_campus_project_clean` and `validate_ifc_all.sh`. Compiled IFC next to it: `university_campus_lod350_project.ifc`.

university_campus_fluid_project/ is the LOD 350 building with plan curves (`smooth` points) replacing many right angles (contents listed in its `project.bml` header). Covered by `test_university_campus_project_clean` and `validate_ifc_all.sh`. Compiled IFC next to it: `university_campus_fluid_project.ifc`.

## Project Overview

BuildingML is a domain-specific language (DSL) for describing Building Information Models (BIM) in a markdown-inspired format. It bridges LLM-generated text and IFC (Industry Foundation Classes) building standards. The full language specification is in `Specs.md`.

**Status**: V0.1 complete (Phases 1–5). V0.2 nearly complete (~90%) — all major phases done (7a–7j), remaining items are compiler quality polish (Phase 7h at ~30%) and VS Code 2D preview tab. V0.3 planning started — focused on refactoring, LLM evaluation, spec-to-check generation, and regulation check libraries. Tree-sitter grammar, parser, semantic analysis, IFC codegen (all element types with proper geometry, property sets, pitched roofs, curved walls, polygon holes), CLI (compile, lint, check, decompile, analyze, lsp, preview, scene, model, render, serve), WASM bindings, LSP server with VS Code extension (including 3D/2D preview panel with cursor-driven storey selection, IFC export command), compliance checking engine with report generation, 3D scene graph generator, WebGL viewer, 2D floorplan editor with React app (hierarchical element tree, bi-directional BML sync, editable code editor, unified undo/redo), floor templates, elevator type, auto-ID generation, space zones, furniture asset system with geometry primitives, IFC import analysis with coverage reporting, chatbot integration (collapsible chat panel, LLM context builder, OpenAI-compatible API proxy with streaming, BML change application), IFC export and preview (web-ifc WASM parser in web app, IFC→SceneGraph converter, independent IFC geometry verification), Dockerfile and CI are implemented. See `Specs_V0_2.md` for v0.2 specification, `Specs_V0_3.md` for v0.3 specification.

## Technology Stack

- **Language**: Rust (parser, compiler, LSP server, WASM)
- **Parser**: tree-sitter (grammar.js → C parser → Rust bindings, CST → AST conversion pass)
- **IFC Backend**: own raw STEP writer (`codegen/step_writer.rs`); ifc_rs only for property value types and test re-parsing
- **LSP**: tower-lsp crate
- **Build**: Cargo (Rust), Vite (web frontend)
- **Web Frontend**: React + TypeScript + Zustand (state), Vite bundler, Three.js (optional 3D renderer), CodeMirror 6 (code editor)
- **WASM**: Full compiler compiled to WASM from Phase 1 (all deps must be WASM-compatible)

## Build Commands

```bash
cargo build                    # Build the project
cargo test                     # Run all tests (194 Rust tests)
cargo test <test_name>         # Run a single test
cargo clippy                   # Lint
cargo fmt                      # Format code
./scripts/check.sh             # Run file size check + tests + clippy + IFC validation
./scripts/check_file_sizes.sh  # Fail if a tracked file exceeds 20 MB, except files in its EXCEPTIONS list (also runs in CI)
./scripts/build.sh             # Quick cargo check wrapper
cd web && npm run test         # Run web frontend tests (vitest)

# IfcOpenShell IFC validation. One-time setup of the repo's uv env (check.sh puts .venv/bin first on PATH):
#   uv venv -p 3.12 .venv && uv pip install -p .venv/bin/python ifcopenshell numpy
python3 scripts/validate_ifc.py output.ifc              # Validate a single IFC file
python3 scripts/validate_ifc.py --compile input.bml      # Compile BML then validate
python3 scripts/validate_ifc.py --geometry output.ifc    # Also check geometry
python3 scripts/validate_ifc.py --json output.ifc        # JSON output for programmatic use
./scripts/validate_ifc_all.sh                            # Validate all BML fixtures (DoD)
./scripts/validate_ifc_all.sh --geometry                 # With geometry validation
python3 scripts/ifc_roundtrip.py tests/fixtures/model_1.ifc  # IFC -> BML -> IFC bbox comparison per GlobalId
```

## CLI Commands (Implemented)

```bash
buildingml analyze input.ifc [more.ifc...]        # IFC import coverage analysis
buildingml analyze *.ifc -f json -o report.json   # Analysis report in JSON format
buildingml compile input.bml -o output.ifc       # Compile to IFC
buildingml lint input.bml                         # Validate syntax + semantics
buildingml check input.bml --format json -o report.json  # Run compliance checks
buildingml decompile input.ifc -o my-building/   # Reverse to BuildingML (meshes each element and fits BML geometry to it)
buildingml render input.bml -o output/ --all     # SVG floorplans + 3D views
buildingml model input.bml                        # Output semantic model JSON to stdout
buildingml scene input.bml                        # Generate 3D scene graph JSON to stdout
buildingml ifc-scene input.ifc                    # Generate 3D scene graph JSON directly from IFC (native ifc-lite reader; resolves real IFC surface styles and materials)
buildingml lsp                                    # Start language server (stdio)
buildingml preview my-building/ --port 8080       # 3D preview server (legacy HTML)
buildingml preview my-building/ --web-dir web/dist # Preview with React app
buildingml serve --web-dir web/dist --port 3001    # Production server (POST-only API, no file access)
buildingml serve --web-dir web/dist --examples-dir tests/fixtures/ --port 3001  # With example selector
buildingml init my-building                       # Create new project (residential template)
buildingml init my-building --template office     # Create with office template
buildingml init my-building --template mixed-use  # Create with mixed-use template
buildingml init my-building --flat                # Flat structure (no subdirectories)
buildingml init my-building --force               # Overwrite existing directory
buildingml init                                   # Interactive mode (prompts for name)
```

### CLI Commands (Planned — Future Phases)

```bash
buildingml fmt my-building/                       # Auto-format files
```

## Architecture

### Compiler Pipeline

```
.bml files → tree-sitter Parser → CST → AST → Semantic Analysis → IR → IFC/Visualization/Checker
```

1. **Parsing**: tree-sitter grammar → CST with error recovery → typed AST conversion
2. **Semantic Analysis**: Type checking, reference resolution, spatial containment validation
3. **IR**: Fully resolved, typed model with linked references
4. **Code Generation**: IR → IFC4 STEP file (raw STEP writer), 3D scene graph, or compliance reports

### Language Syntax Key Concepts

- Heading levels (`#`–`####`) denote IFC spatial hierarchy: Site → Building → Storey → Space
- Elements before the first child heading of `# Site` / `## Building` are site/building-level; a `####` space attaches to the nearest preceding Site, Building or Storey. Iterate with `SemanticModel::containers()` (storey, building, site with their spaces and elements), `elements()`, `spaces()` (`Specs_V0_3.md` §15.1). 2D views (SVG floorplan, web editor, VS Code 2D) show building/site-level contents as extra plans after the storeys: `getStoreys()` in `model-importer.ts` returns these plans with a `PlanRef` (`level` + heading index) that `generateFullBml()` uses to merge edits back
- `Type: ID` pattern for element declarations (e.g., `Wall: W-GF-01`); bare `Type:` (no ID) for auto-generated IDs (v0.2). Element types: Wall, Slab, Column, Beam, Window, Door, Roof, Stair, Railing, Ramp, Furniture, Elevator, Proxy, Member, Plate, Opening, Grid, Covering, Footing, Duct, Pipe, CableTray, Terminal, Equipment, Accessory, System
- `//` for line comments, `///` for doc comments, `#` exclusively for headings
- Block-only geometry (key-value on separate lines, no inline shorthand)
- Units: default from `buildingml.toml`, explicit per-value override allowed
- Enums: freeform `snake_case` validated by compiler (e.g., `type: double_glazed`)
- Multi-line values via indentation continuation
- Multi-file projects with `import` statements
- Coordinates: local per storey, buildings placed site-relative
- Wall connectivity: implicit from endpoint proximity (auto L/T/X junction detection)
- Relationships: explicit property references (`supports:` / `supported_by:`, one direction suffices)

### Compliance Checking

- First-class citizen with `Check:` blocks using `applies_to`, `where`, `assert`
- Extended with quantifiers/aggregations: `count()` implemented; `sum()`, `min()`, `max()` are stubs (return errors); `forall`, `exists` not yet implemented
- Parametric checks for cross-jurisdictional reuse
- `depends_on` for check composability
- Structured regulation modeling (jurisdiction, version, sections)
- 5 built-in computed properties: `clear_height`, `escape_route_length`, `window_to_floor_ratio`, `is_fire_compartment_boundary`, `structural_span`
- Extensible via `ComputedProperty` trait
- Reports in JSON/HTML/SARIF format

### 3D Visualization

- Scene graph module (`bml-core/src/scene/`) converts SemanticModel IR to triangle meshes
- Box and polygon extrusion primitives with per-face normals for all element types
- Wall rotation via axis angle, storey elevation offsets
- Serializable to JSON (`SceneGraph` → `SceneNode` → `SceneMesh` → `Vertex`); `SceneNode.properties` carries element metadata as `Vec<(String, String)>` for 3D inspector
- `SceneGraph.materials` is a shared `SceneMaterial` table (name, RGBA, specular, shininess); `SceneNode.material` indexes into it. `node.mesh.color` holds the same RGBA, since the renderers read colour from the mesh
- Material looks (`scene/appearance.rs`, `Specs_V0_3.md` §16) are shared by the BML scene, the IFC writer (`IfcSurfaceStyle` per `IfcMaterial`) and `ifc-scene` (fallback for elements without a surface style), so the BML and IFC previews show the same colours. Order: `Material:` `color`/`opacity`/`specular`/`shininess` > keyword palette on the material name > element type default
- Both renderers shade with a Blinn-Phong specular term from the material plus a Fresnel alpha boost, so glazing stays legible edge-on (`Specs_V0_3.md` §16.4)
- `Opening` elements are not drawn; they are cut into host walls and slabs (`Specs_V0_3.md` §16.4)
- React web app (`web/src/`) with shared renderer modules, Zustand state management
- Dual renderer engine: custom WebGL renderer and Three.js renderer (`ThreeJSRenderer`), selectable at runtime via toolbar toggle
- Shared renderer modules in `web/src/shared/renderer/` (used by both React app and VS Code extension IIFE bundle)
- `IRenderer` interface (`renderer-interface.ts`) — common API for both renderers: `loadScene()`, `start()`, `stop()`, `dispose()`, `pick()`, `attachGizmo()`, `camera`, `wireframe`, `sectionCut`, `lateralCut`, `highlightedId`, `onTransform`, `isDraggingGizmo`
- Section cut feature: vertical slider with clipping plane via fragment shader `discard`
- Example selector: `--examples-dir` flag serves `/api/examples` (list) and `/api/examples/{file}` (content) for .bml fixture files
- CLI `preview` serves viewer + REST API (`/api/scene`, `/api/lint`, `/api/model`)
- VS Code webview loads shared renderer IIFE bundle from `editors/vscode/media/shared-renderer.js` (lean bundle: WebGL renderer + 2D floorplan modules only, excludes Three.js + web-ifc to avoid CSP violations)
- VS Code preview panel: 3D/2D toggle, 2D view uses cursor line to auto-select storey via `buildingml model` CLI + span data, click-to-inspect overlay in 3D (surface material with colour swatch, identity, and property sets/quantities grouped by set), Hide/Isolate buttons, and a Filter panel for category/storey visibility
- IIFE entry point: `web/src/shared/renderer/index-iife.ts` (VS Code), full `index.ts` (web app)
- Legacy viewers preserved at `web/viewer.legacy.html` and `web/floorplan.legacy.html`
- WASM bindings: `generate_scene()`, `generate_scene_project()`, `generate_model()`, `generate_model_project()`

### Web Architecture

- React + Vite + TypeScript in `web/src/`
- **Shared modules** (`web/src/shared/`): framework-agnostic TypeScript for renderer, floorplan editor, data providers
- **DataProvider pattern**: `RestProvider` (preview server), `WasmProvider` (client-side WASM), `VsCodeProvider` (postMessage bridge)
- **Zustand stores**: `app-store` (view mode, source, diagnostics), `viewer-store` (scene, wireframe, section cut, renderer engine, 3D selectedElement), `floorplan-store` (elements, selection, tools, undo/redo), `chat-store` (messages, streaming state, open/close)
- **Multi-storey save**: `originalFileSource` is kept in sync with `bmlOutput` after every edit so that `mergeStoreyIntoSource` always merges into the latest full content. This prevents editing one storey from reverting another. On import the store takes `keptBlocks()` (`bml-generator.ts`), the plan's element blocks whose id `importStorey` did not import (beams, proxies, MEP), and `generateFullBml` adds them back on every regenerate.
- **Bi-directional sync**: CodeMirror 6 BML editor (`BmlEditor.tsx` + `BmlCodeMirror.tsx`) with syntax highlighting, line numbers, and debounced parse — user edits BML text → provider.lint() + provider.generateModel() → `applyBmlSource()` updates 2D elements; 2D edits → `refreshAll()` regenerates BML → editor updates. Parse errors shown inline.
- **CodeMirror modules** (`web/src/shared/codemirror/`): `bml-language.ts` (StreamLanguage tokenizer for BML headings, element types, properties, comments, numbers, strings, template variables), `bml-theme.ts` (dark theme + syntax colours matching Catppuccin palette), `BmlCodeMirror.tsx` (React wrapper with controlled value, diagnostic border, key propagation control)
- **Hierarchical element tree**: `ElementTree.tsx` shows Storey → Space → Elements with expand/collapse. Doors/windows nested under their parent walls. Columns placed inside spaces via `pointInPolygon`. Unassigned elements shown at storey level.
- **Unified undo/redo**: `applyBmlSource()` pushes current elements to undo stack before applying code changes. Undo/redo works across both 2D canvas edits and code editor edits.
- **IIFE bundle**: `npm run build:lib` in `web/` builds shared renderer as IIFE (~26KB) for VS Code extension (uses `index-iife.ts` — excludes Three.js + web-ifc)
- Path aliases: `@shared/`, `@components/`, `@state/`
- **Chatbot**: `ChatPanel.tsx` collapsible sidebar with streaming LLM responses, context-aware prompting (current BML source, storey, selected element, diagnostics), automatic BML extraction and application from LLM responses
- **Chat modules** (`web/src/shared/chat/`): `context-builder.ts` builds LLM system prompt from editor state, `extractBmlFromResponse()` parses BML code blocks from responses
- **Tests**: `vitest` in `web/src/shared/floorplan/__tests__/`, `web/src/shared/chat/__tests__/`, `web/src/shared/renderer/__tests__/`, `web/src/shared/__tests__/`, `web/src/components/__tests__/`, and `web/src/state/__tests__/` — regression tests for BML generator multi-storey merge, element tree hierarchy, bi-directional sync with undo/redo, chat context builder, BML extraction, IFC loader, Three.js renderer (including picking, highlight, gizmo), viewer-store selection state, breakpoint classification, long-press timing, bottom sheet state transitions
- **Mobile responsive**: `useBreakpoint()` hook (phone <768px, tablet 768-1024px, desktop >1024px), `BottomSheet` component (draggable: collapsed/half/full states for element tree, properties, chat), `BottomNav` component (5-tab bottom navigation: 3D/2D/Code/Chat/Properties), long-press context menu on 2D floorplan canvas (500ms hold), tablet-specific compact toolbar layout, touch navigation (single-finger rotate/pan, two-finger pinch zoom), iOS safe-area support, `touch-action: none` on canvases

### IFC Export & Preview

- **web-ifc** (MPL-2.0) WASM IFC parser for independent IFC geometry verification (web app only)
- Pipeline: BML → `buildingml compile` → IFC STEP string → web-ifc `OpenModel`/`StreamAllMeshes` → extract vertices/indices/colors with 4x4 placement transforms → convert to SceneGraph → existing WebGLRenderer renders it
- **ifc-loader.ts** (`web/src/shared/renderer/ifc-loader.ts`): singleton `initIfcApi()` with lazy init, `ifcToSceneGraph()` converter
- Renderer uses Uint32 indices (`OES_element_index_uint`) for IFC meshes that exceed 65535 vertices
- VS Code: 3D/2D toggle, "Export to IFC" command (uses CLI, not web-ifc). IFC preview removed — web-ifc Emscripten uses eval/new Function() which violates VS Code webview CSP
- Web app: `ViewMode: 'ifc'`, `ifcScene` in viewer-store, "Export IFC" button (Blob download), Three.js toggle
- web-ifc.wasm served from `web/public/` (Vite dev)

### IFC Material & Style Resolution

Native IFC viewing (`buildingml ifc-scene`) reads the presentation styles authored in the file instead of colouring by IFC type. Implemented in `crates/bml-cli/src/commands/ifc_styles.rs` (`StyleIndex`), 10 unit tests.

- Two style channels, both funnelling into `IfcSurfaceStyle` → `IfcSurfaceStyleRendering`/`…Shading` → `IfcColourRgb`:
  1. **Per geometry item** — `IfcStyledItem.Item` keyed by representation item id, matched against the `geometry_id` of ifc-lite's `process_element_with_submeshes()`. Lets a window's frame and glazing take different materials.
  2. **Per material** — `IfcRelAssociatesMaterial` → `IfcMaterial` → `IfcMaterialDefinitionRepresentation` → `IfcStyledRepresentation`. Used as fallback; some exporters write only this channel.
- `IfcPresentationStyleAssignment` (IFC2x3) is unwrapped transparently. Layer sets, constituent sets, profile sets and material lists are flattened to their `IfcMaterial` members for the inspector's material buildup.
- Materials are deduplicated by surface style entity id into `SceneGraph.materials`; elements emit one node per distinct material, extra materials becoming children that reuse the parent's id so picking/highlighting stays whole-element.
- **Revit quirk**: glass is exported with `Transparency = 1.0`, meaning "as transparent as glass gets", not "invisible". Taken literally it erased 409 elements (a whole curtain wall facade) on `tests/fixtures/model_2.ifc`, so opacity is clamped to `MIN_ALPHA` (0.12) as mainstream viewers do.
- Coverage on the Revit fixtures: 97–98% of meshes resolve to a real material (36/54/25 materials, 35/48/11 distinct colours for model_1/2/3), versus 10 type colours before.

### IFC Property Sets & Quantities

`crates/bml-cli/src/commands/ifc_properties.rs` (`PropertyIndex`, 6 unit tests) fills `SceneNode.properties` for the element inspector.

- `IfcRelDefinesByProperties` → `IfcPropertySet` (`IfcPropertySingleValue`/`EnumeratedValue`/`ListValue`) and `IfcElementQuantity` (every `IfcQuantity*` keeps its measure at attribute 3)
- Rows are emitted flat as `"<set> · <property>" -> "<value>"`; the viewers split on the first `" · "` to render group headings. Identity rows (IFC Type, Name, GlobalId, Object Type, Material, Surface Style, Storey) have no separator and render ungrouped
- Only sets referenced by a *rendered* element are decoded — most sets in an export hang off the project/site/spaces, so this skips the bulk of model_2's 15,808 sets
- STEP typed values (`IFCLABEL('x')`, `IFCBOOLEAN(.T.)`, `IFCAREAMEASURE(56.3)`) decode as `[type name, inner]`, the same shape as a plain string list — the `IFC` prefix on element 0 disambiguates. Floats are trimmed to 4 dp; empty values are dropped
- Only the first sub-mesh of an element carries the rows; `scene-loader.ts` and the Three.js loader pass them down to children, keeping the payload flat (model_2: 104 MB vs 102 MB, median 28 rows/element, 100% of elements covered)

### IFC Preview (VS Code)

The VS Code preview also renders raw `.ifc` files via `buildingml ifc-scene`.

- Spawns are async (`PreviewPanel.run()`); a `renderToken` discards superseded results
- `PreviewPanel.inputKey()` skips renders whose inputs are unchanged
- Rendered classes, dangling-reference repair, packed binary transfer and large-file handling: `Specs_V0_3.md` §17
- Render timings and webview errors go to the "BuildingML Preview" output channel

### Scene Payload Precision

`Vertex` holds `f32`, but JSON has one number type, so serde promoted each coordinate to
`f64` and printed the promotion in full: `-15.2642` serialized as `-15.264200210571289`. On a
real model that was 59MB of a 71MB payload. `Vertex` now serializes through `round4`
(`scene/graph.rs`), keeping four decimals - a tenth of a millimetre, finer than `f32`
resolves at building scale and under a hundredth of a degree on a unit normal. Payload drops
to 38.5MB with identical scene bounds. Property rows are only 0.6MB and were never the cause.

### Section Cuts

Both cuts (`sectionCut` on Z, `lateralCut` on Y) start **disabled**, and their position is
persisted as a **fraction** of the model's extent, never as metres. An absolute value
restored into a model with different bounds lands arbitrarily, which is what left sliders
parked mid-model. `cutFraction()` / `applyCutFraction()` in
`web/src/shared/renderer/section-cut.ts` are shared by the React app and the VS Code
webview; a fraction at or above 0.999 snaps exactly to `max` so no-clip stays no-clip.
Saved states predating the fraction format read as 1. Tested in
`web/src/shared/renderer/__tests__/section-cut.test.ts` (10 tests).

### Element Selection & Visibility (VS Code preview)

- `IRenderer.hiddenIds: Set<string>` — assign a **new** Set to apply; the setter pushes visibility onto meshes so the draw/pick loops stay branch-free. Re-applied after `loadScene` so a filter survives a reload
- Hidden meshes are excluded from picking in both renderers. Three's `Raycaster` only tests layers and *will* report `visible: false` geometry, so `ThreeJSRenderer.pick()` filters explicitly
- `IRenderer.listElements(): ElementSummary[]` — one entry per element (sub-meshes sharing an id collapse), used to build the filter lists
- VS Code webview keeps three independent filter sets (`manuallyHidden`, `hiddenCategories`, `hiddenStoreys`) and recomputes the union, so re-enabling a category does not resurrect individually hidden elements. Storey filtering reads the `Storey` property row

### Geometry Approach

No geometry kernel. Parametric geometry + basic 2D line math for wall trimming + IfcOpeningElement for door/window openings. Future BREP/OBJ support is data transcription. NVIDIA USD BRep (open-source SMLib kernel) is being evaluated as an optional kernel — see `Specs_V0_3.md` §14 and the prototype in `prototypes/usd_brep/`.

### Project Configuration

`buildingml.toml` for project settings: `buildingml_version`, IFC version, output format, default units, check suites, editor snap settings.

## Implementation Roadmap

- **Phase 1** ✅: tree-sitter grammar, Rust parser/AST, basic semantic analysis, single-file compilation, CLI (compile, lint), WASM target
- **Phase 2** ✅: IFC decompiler, multi-file support, material definitions, all v0.1 element types
- **Phase 3** ✅: LSP server (tower-lsp) with diagnostics/completion/hover/go-to-def/symbols/semantic tokens/formatting/code actions, VS Code extension, `buildingml lsp` CLI
- **Phase 4** ✅: Compliance check parser (`Check:`/`CheckSuite:` blocks), evaluation engine (comparisons, set membership, path resolution, computed properties), report generation (JSON/HTML/SARIF), regulation modeling, CLI `check` command
- **Phase 5** ✅: 3D scene graph generator, WebGL viewer (web/viewer.html), VS Code preview panel, CLI `preview` command, WASM scene bindings
- **Phase 6**: LLM evaluation (token efficiency, success rate, semantic accuracy, edit distance), benchmarks vs ifcJSON + IfcOpenShell
- **Phase 7a** (v0.2): IFC codegen hardening — proper geometry for all element types
- **Phase 7a′** (v0.2): Language improvements — floor templates, storey-level elements, elevator type, railing/ramp positioning, auto-IDs (bare `Type:` without ID), `zone:` property for space grouping, extended usage enums
- **Phase 7a** (v0.2, complete): IFC codegen hardening — proper geometry for all element types (column, beam, stair, ramp, railing, door, furniture, pitched roofs). StepGeometryWriter with shared primitives and arbitrary profile support. Slab property sets via raw STEP injection (Pset_SlabCommon). Space boundary polygons (explicit `boundary:` property). Roof pitch via IfcArbitraryClosedProfileDef extrusion (gable, hip, shed, mansard). Per-storey spatial containment for raw STEP entities. Storey-relative IfcLocalPlacement (PlacementRelTo → storey, storey-relative Z coordinates). Door/railing centered profile placement. IFC string quoting fix (StringPrimitive double-quote prevention). Elevator multi-storey height span.
- **Phase 7a′** (v0.2, complete): Language improvements — floor templates with `{variable}` substitution, `apply_template:` directive, storey-level elements, elevator type (IfcTransportElement), railing/ramp `from:` axis, extended usage enums (25 values), space `zone:` property, auto-ID generation (bare `Type:` without ID)
- **Phase 7b** (v0.2, ~95%): Web frontend modularization complete — React + Vite app, shared renderer/floorplan modules, DataProvider adapter pattern, VS Code extension using shared bundle, section cut feature, full 2D floorplan editor with undo/redo, `serve` CLI command, Dockerfile, GitHub Actions CI, example selector dropdown (`--examples-dir` flag), mobile responsive design (`useBreakpoint` hook with phone/tablet/desktop breakpoints, `BottomSheet` draggable panels, `BottomNav` 5-tab bottom navigation, long-press context menu on 2D canvas, tablet-specific compact toolbar, touch pan/pinch-zoom on 3D and 2D canvases, iOS safe-area support), CodeMirror 6 code editor (BML syntax highlighting, line numbers, dark theme). Remaining: VS Code 2D preview tab.
- **Phase 7c** (v0.2, complete): Enhanced geometry — arbitrary polygon spaces, space boundary inference from wall topology, slab-follows-space (auto boundary), tessellated cylinder mesh, arc segments for curved walls (IfcTrimmedCurve), polygon holes (IfcArbitraryProfileDefWithVoids), new scene graph primitives (`cylinder_mesh()`, `arc_extrude()`, `extrude_polygon_with_holes()`), tree-sitter grammar support for `via:` in axis values and `polygon_with_holes` value type
- **Phase 7d** (v0.2, complete): Furniture asset system — `# AssetLibrary` / `## Asset` / `### Part` heading hierarchy, geometry primitives (`box()`, `cylinder()`, `extrude()`), 3D point values `(x, y, z)` for part positioning, `asset:` property on Furniture elements for instance placement, IFC codegen (per-part IfcFurnishingElement entities), scene graph expansion (asset parts → child mesh nodes), cross-file asset import via semantic model merge, reference validation (duplicate asset IDs, unresolved asset references)
- **Phase 7e** (v0.2, complete): Real IFC import analysis — decompiler rewritten for real-world IFC files (bypasses ifc_rs, direct STEP parsing), all 12 BML element types + spaces, spatial containment (IfcRelContainedInSpatialStructure), spatial aggregation (IfcRelAggregates), unit conversion (mm/cm/m), property extraction (Pset_* via IfcRelDefinesByProperties), door/window nesting (IfcRelVoidsElement + IfcRelFillsElement), IFC entity frequency analysis module, `buildingml analyze` CLI command (text + JSON), 61.7% BML coverage across 3 real Revit IFC files
- **Phase 7f** (v0.2, ~95%): Interactive editing — hierarchical element tree (Storey → Space → Elements with expand/collapse, doors/windows nested under walls), editable BML code editor with bi-directional sync (debounced parse, error display, syntax validation), unified undo/redo across 2D canvas edits and code edits, property editing from 2D selection, VS Code 2D floorplan preview (3D/2D toggle, cursor-driven storey selection via `model` CLI + span data, read-only pan/zoom, auto-fit camera), 3D element selection via raycasting (Three.js Raycaster) and color-buffer picking (WebGL FBO), selection highlight shader (both renderers), metadata/properties panel for 3D selected elements (`Viewer3DPropertiesPanel`), SceneNode properties metadata (Rust→JSON→TypeScript pipeline), Three.js TransformControls gizmo for translate with orbit-camera suppression, 3D→BML sync (gizmo transforms persisted via `nudgeElement`, Cmd+Z/Shift+Z undo/redo with scene rebuild, Cmd+S save). Limitations: beams not in floorplan model (visual-only gizmo), doors/windows use wall-relative offset (not nudgeable)
- **Phase 7g** (v0.2, complete): Chatbot integration — collapsible chat panel (sidebar on desktop), LLM context builder (current BML source, storey, selected element, diagnostics, view mode), OpenAI-compatible API proxy with streaming (SSE), BML extraction from LLM responses and automatic application (lint validation, source update, floorplan + 3D rebuild), chat history per session, Zustand `chat-store`, `POST /api/chat` endpoint in both preview and serve servers. Environment variables: `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `OPENAI_MODEL`.
- **Phase 7h** (v0.2, ~30%): Compiler quality — "did you mean?" suggestions for unknown properties/element types (Levenshtein edit distance), codegen warning system (geometry approximations, skipped elements), multi-file diagnostic reporting. Remaining: fuzzy storey matching, opening overflow validation, cross-storey reference validation, coordinate bounds checking, linting rules, file path in Span/Diagnostic
- **Phase 7i** (v0.2, complete): IFC export & preview — `web-ifc` WASM IFC parser (MPL-2.0, web app only), IFC-to-SceneGraph converter (`ifc-loader.ts`), Uint32 index support in WebGL renderer, VS Code "Export to IFC" command (CLI-based), web app IFC view mode with "Export IFC" button + Three.js toggle, 11 unit tests for ifc-loader. VS Code IFC preview removed (web-ifc Emscripten violates webview CSP). Lean IIFE bundle (~26KB) via `index-iife.ts` entry point.
- **Phase 7j** (v0.2, complete): Additional IFC elements — Proxy (IfcBuildingElementProxy, generic catch-all element), Member (IfcMember, structural members like braces/struts/ties), Plate (IfcPlate, flat structural elements like panels/steel plates). Full pipeline for each: grammar, parser, semantic analysis, type checking, IFC codegen, scene graph (3D rendering), SVG floorplan renderer (2D), decompiler, IFC analyzer. LSP support (completion snippets, hover docs, property completions). Auto-ID prefixes: PX-, MB-, PL-.
- **Phase 8a** (v0.3, complete): Compiler quality — fuzzy storey matching (Levenshtein + word-prefix), opening overflow validation, Opening element (IfcOpeningElement, full pipeline), Grid element (IfcGrid with axes, full pipeline), Dimension removed from grammar, cross-storey reference validation, coordinate bounds checking (IQR-based outlier detection), linting rules (8 rules with TOML config: wall_thickness, door_width, window_sill, storey_height, column_aspect, beam_span, space_door, wall_overlap — coincident/duplicate walls, see `Specs_V0_3.md` §3.5), file path in Span/Diagnostic (file_id + FileRegistry), multi-location diagnostics (RelatedInformation with span + message), improved error messages (show provided properties, typo detection)
- **Phase 8b** (v0.3, ~85%): Refactoring & architecture — element type registry (`ElementTypeDescriptor` with centralized metadata for all element types in `semantic/registry.rs`), codegen decomposition (`step_writer.rs` extracted with `StepGeometryWriter`, geometry primitives, GUID generation), compliance checker completions (`sum()`/`min()`/`max()` aggregation functions, `forall`/`exists` quantifiers), 27 new tests (checker quantifiers, computed properties). Remaining: multi-file tests, template edge case tests, error code ranges
- **Phase 8c** (v0.3, 0%): LLM evaluation & benchmarks — token efficiency analysis (BML vs ifcJSON vs STEP), LLM generation success rate (Claude/GPT-4o/Gemini), edit distance analysis, benchmark infrastructure
- **Phase 8d** (v0.3, 0%): Spec-to-check generation — LLM reads design specifications and generates `Check:` blocks, validation pipeline, chat integration for spec upload
- **Phase 8e** (v0.3, 0%): Regulation-to-check library — curated compliance checks from MBO (German), IBC (International), Eurocode, check import mechanism, computed property extensions
- **Phase 8f** (v0.3, 0%): LLM prompt & context improvements — semantic model context (structured summary), compliance results in context, few-shot examples, auto-retry on parse errors, structured output mode
- **Phase 8g** (v0.3, 0%): VS Code & editor polish — VS Code 2D preview tab, quick fix code actions, compliance dashboard, regulation browser
- **Phase 8h** (v0.3, ~90%): Project scaffolding CLI — `buildingml init` command with templates (residential, office, mixed-use), `--flat`/`--force`/`--template` flags, interactive mode, `buildingml.toml` generation, `.gitignore`, furniture assets, compliance checks, `storeys/` subdirectory with per-storey files in nested mode, 15 tests. Multi-file reference fixture (`tests/fixtures/six_storey_residential_project/`) with 8 BML files split from single-file fixture, 9 integration tests. See `Specs_V0_3.md` §10 for remaining items.
- **Phase 8i** (v0.3, 0%): Asset library expansion — common furniture collection (25+ assets: residential, kitchen, bathroom, office), auto-include via config

## Implementation Notes

### IFC Codegen (raw STEP)
- `codegen/ifc_writer.rs` (`Gen`) writes every entity itself through `StepWriter` (`codegen/step_writer.rs`: shared points/directions, placements, profiles, extruded solids). ifc_rs is used only for `IfcValue` in `pset_mapping.rs` and for re-parsing output in tests
- `BoolPrimitive` is `pub(crate)` in ifc_rs — booleans are written as `IfcValue::Label(LabelValue("TRUE"/"FALSE"))`
- ifc_rs depends on bevy_math 0.14 (glam 0.27) — do NOT upgrade to 0.15
- Elements inside a `####` space are contained in that `IfcSpace`; `part_of:` elements go into an `IfcRelAggregates` under the named whole instead of the storey
- `material:` (any element) writes one `IfcMaterial` per name plus `IfcRelAssociatesMaterial`; a layered material (`layers:` list) writes an `IfcMaterialLayerSet`, with an `IfcMaterialLayerSetUsage` for walls, slabs, plates, flat roofs and coverings (first layer outside / on top). E306 lints layer lists and thickness vs layer sum (`Specs_V0_3.md` §18)
- `is_external`, `load_bearing`, `fire_rating`, `thermal_transmittance`, `acoustic_rating` go to `Pset_<Type>Common` (`pset_mapping.rs`)
- `predefined_type:` (snake_case) fills the class's PredefinedType (`Specs_V0_3.md` §15.1)
- Straight walls are joined at L/T junctions (`codegen/wall_joins.rs`, used by the writer and the scene; `Specs_V0_3.md` §19)
- `Covering` in a `####` space: outline and ceiling height default from the space, `IfcRelCoversSpaces` (`Specs_V0_3.md` §20)
- `Footing`: outline defaults from the walls it runs `under:` or from `position:`, body down by `depth` from `z_offset` (`Specs_V0_3.md` §21)
- A wall with `mullions:` expands into an `IfcCurtainWall` with mullion/transom `Member`s and panel `Plate`s as parts (`Specs_V0_3.md` §22)
- `Stair`: stepped flight body; an `l_shaped`/`u_shaped` stair or a `straight` one with `landing_after:` expands into two `IfcStairFlight`s and a landing slab (`Specs_V0_3.md` §23)
- `Duct`/`Pipe`/`CableTray` `path:` routes expand into segments, bends and tees (`semantic/mep.rs`); the writer connects their ports and writes `IfcDistributionSystem`s; `Terminal`/`Equipment` `type:` picks the IFC class (`codegen/mep.rs`, `Specs_V0_3.md` §24)
- Straight runs crossing walls/slabs get generated `Opening` children `{run}-{host}` and optional sleeves (`semantic/penetration.rs`, `Specs_V0_3.md` §25); `hangers:` adds trapeze hanger `Accessory`s up to the slab above (§26)
- `connects:` writes connection relationships (realizing ones for accessories/fasteners); `brackets: true` on a curtain wall generates mullion-to-slab brackets (`Specs_V0_3.md` §27)
- `smooth` points and `nurbs { ... }` items make plan curves (outlines, `path:` of walls, railings, beams, members and MEP runs), written as exact B-splines (`Specs_V0_3.md` §28)
- `representation: none` writes a product without a body (e.g. a curtain wall whose panels carry the geometry)
- A `boundary:` on a type that normally has another geometry (wall, column, proxy, ...) writes a vertical prism from `z_offset` with `height`; slabs/plates extrude down by `thickness`, flat roofs up

### IFC Roundtrip (IFC → BML → IFC)
- `buildingml decompile` = `decompile::ifc_reader` (spatial tree, names, `guid`, `ifc_name`, `ifc_class`, `predefined_type`, `part_of`, `opening:`, psets) + mesh fitting (`decompile/mesh_fit.rs`). The CLI meshes each element with ifc-lite, then `fit_meshes()` compares the BML scene bbox of each element with its mesh bbox (0.02 m). On a mismatch it first tries a parametric fit (wall axis via 2D PCA, beam/member 3D axis, rectangular column), else a prism from the mesh footprint. Spaces always take boundary and height from their mesh
- Before meshing, the CLI unsets dangling references (`ifc_scene::repair`) and resolves the length unit itself (`ifc_scene::length_unit_scale`; ifc-lite gets FOOT wrong). The reader detects SI prefixes and conversion-based units (`detect_length_unit_scale`)
- Elements without a body, with only helper representations (`Axis`, `FootPrint`, `Box`, ...) or whose required geometry neither the reader nor a mesh supplies (`mesh_fit::drop_incomplete`) get `representation: none`
- One `## Building` per `IfcBuilding`; elements and spaces contained directly in an `IfcBuilding` / `IfcSite` stay building/site-level. A door/window in another storey than its host wall keeps `opening:`, and the writer emits `IfcRelFillsElement` in a final pass (`Gen::fills`)
- `scripts/ifc_roundtrip.py model.ifc` decompiles, recompiles and compares IfcOpenShell world bboxes per GlobalId (needs ifcopenshell + numpy). All three `tests/fixtures/model_*.ifc` match 100% at 0.05 m; results on `ifc_bench_models/` in `Specs_V0_3.md` §15.3
- BML writer emits storey elements before spaces, rounds numbers to 4 decimals and escapes strings (`\"`, `\\`). Hyphenated ids are valid property values (`element_ref` grammar token)
- Limitations: `Specs_V0_3.md` §15

### IfcOpenShell IFC Validation
- External Python-based validator (`scripts/validate_ifc.py`) using IfcOpenShell (pip install ifcopenshell)
- Validates both IFC schema compliance and geometry processing (via `create_shape()`)
- Integrated into Rust tests (`test_ifcopenshell_*` in `codegen/mod.rs`) — tests compile BML, write temp IFC, call Python validator, parse JSON result
- Tests skip gracefully when ifcopenshell is not installed (CI installs it, local dev is optional)
- `scripts/validate_ifc_all.sh` validates all BML fixture files — serves as IFC writer "definition of done"
- `scripts/check.sh` runs IFC validation after `cargo test` and `cargo clippy` when ifcopenshell is available
- IFC GUID generation uses SplitMix64 hash with atomic counter for unique, valid base64-encoded 128-bit IDs
- Error categories: `guid_base64` (first char must be 0-3), `empty_set` (SET [1:?] cardinality), `inverse` (orphaned material usages), `schema` (type mismatches)

### Wall Axis Property
The BML syntax `from: (0, 0) to: (6, 0)` parses as property key="from" with an Axis value type. The property key is "from", NOT "axis".

### WASM
Tree-sitter C code requires wasi-sdk to compile to `wasm32-unknown-unknown` (missing stdlib.h without it). Native build + Rust-only WASM works fine.

### Web Build
```bash
cd web && npm run dev          # Dev server (Vite) at localhost:5173
cd web && npm run build        # Production build to web/dist/
cd web && npm run build:lib    # IIFE bundle for VS Code extension
```

## Adding a New Element Type

With the centralized `ElementTypeDescriptor` registry (`crates/bml-core/src/semantic/registry.rs`), adding a new element type now requires changes to fewer files. The registry holds property schemas, auto-ID prefixes, hover text, completion snippets, scene colors, and SVG CSS classes — all in one place.

**Required changes (≤ 5 files):**

1. **Element Type Registry** — `crates/bml-core/src/semantic/registry.rs`: Add `ElementTypeDescriptor` entry to `ELEMENT_REGISTRY` array with all metadata (properties, hover text, completion snippet, scene color, etc.)
2. **ElementType enum** — `crates/bml-core/src/semantic/model.rs`: Add variant, update `parse()`, `as_str()`, `all_names()`
3. **Tree-sitter grammar** — `crates/tree-sitter-buildingml/grammar.js`: Add to `element_type` choice list, then run `npx tree-sitter generate` and commit generated C files
4. **IFC codegen** — `crates/bml-core/src/codegen/ifc_writer.rs`: Handle the type in `Gen::shape()` (geometry as `Solid`s from `step_writer.rs`); the IFC class comes from the registry's `ifc_entity`
5. **Scene graph** — `crates/bml-core/src/scene/graph.rs`: Add mesh function, handle in `element_to_node()` (color auto-reads from registry)

**Auto-derived from registry (no changes needed):**
- Type checking (`type_check.rs`) — delegates to registry
- Auto-ID prefix (`containment.rs`) — delegates to registry
- LSP hover (`hover.rs`) — delegates to registry
- LSP completions (`completion.rs`) — delegates to registry
- Scene color (`graph.rs`) — reads from registry

**Additional files for full pipeline:**
6. **SVG renderer** — `crates/bml-core/src/render/floorplan.rs`: Add drawing function, bounds calculation
7. **Decompiler** — `crates/bml-core/src/decompile/ifc_reader.rs`: Add IFC entity match arm
8. **IFC analyzer** — `crates/bml-core/src/decompile/ifc_analyzer.rs`: Update `bim_element_mapping()`
9. **Web syntax highlighting** — `web/src/shared/codemirror/bml-language.ts`: Add type name to keyword list
10. **Test fixture** — `tests/fixtures/all_elements.bml`: Add example element
11. **Tests** — Codegen test, scene graph test, semantic test
12. **Documentation** — Update specs, CLAUDE.md, README.md

## Key Files

- `Specs.md` — Complete language specification (v0.1) with all design decisions resolved
- `Specs_V0_2.md` — V0.2 specification: IFC hardening, web platform, furniture assets, interactive editing
- `Specs_V0_3.md` — V0.3 specification: refactoring, LLM features, spec-to-check, regulation library
- `README.md` — Project overview and quick start
- `web/src/shared/` — Shared TypeScript modules (renderer, floorplan, providers, types)
- `web/src/components/` — React components (viewer, editor, floorplan, chat, layout)
- `web/src/state/` — Zustand stores (app, viewer, floorplan, chat)
- `prototypes/usd_brep/` — Exploratory USD BRep geometry kernel prototype (Python 3.12, not in build/CI; see `Specs_V0_3.md` §14)
- `scripts/validate_ifc.py` — IfcOpenShell IFC validator (schema + geometry, JSON output)
- `scripts/validate_ifc_all.sh` — IFC writer definition-of-done (validates all BML fixtures)
- `ifc_bench_models/` - gitignored local symlink to `../building_models/ifc_bench_models` (real-world IFC benchmark models; `west_riverside_hospital/` is the large multi-domain performance test). Create with `ln -s ../building_models/ifc_bench_models ifc_bench_models`
- `LICENSE` — MIT License (Copyright 2026 Stefan Fuchs)
