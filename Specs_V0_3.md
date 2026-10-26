# BuildingML — Specification v0.3

**Date: 2026-03-06**
**Author: Stefan (+ Claude brainstorming)**
**Status: PLANNING — Phase 8 (Refactoring & LLM Features)**

---

## 1. V0.2 Summary & Carryover

V0.2 (Phases 7a–7j) delivered IFC codegen hardening, language improvements (templates, auto-IDs, elevator, zones), web platform (React + Vite + TypeScript), enhanced geometry (polygons, curves, holes), furniture assets, real IFC import analysis, interactive editing (2D/3D with undo/redo), chatbot integration, IFC export & preview, and three new element types (Proxy, Member, Plate).

### 1.1 Incomplete V0.2 Items (Carried Forward)

These items from v0.2 were not completed and are carried into v0.3:

| Item | Original Phase | V0.3 Phase | Notes |
|------|---------------|------------|-------|
| Fuzzy storey reference matching | 7h | 8a | Case-insensitive Levenshtein suggestions |
| Show provided properties on errors | 7h | 8a | Help diagnose typos |
| Opening overflow validation | 7h | 8a | offset + width vs wall length |
| Remove/implement ghost types | 7h | 8a | Opening, Grid, Dimension in grammar |
| Cross-storey reference validation | 7h | 8a | from_storey/to_storey on stairs, elevators |
| Material reference validation | 7h | 8a | Warn on undefined material IDs |
| Coordinate bounds checking | 7h | 8a | Warn on outlier positions |
| Multi-location diagnostic reporting | 7h | 8a | Related code across elements |
| Linting rules for suspicious values | 7h | 8a | e.g., wall thickness > 1m |
| File path in Span/Diagnostic | 7h | 8a | Multi-file error attribution |
| `sum()`, `min()`, `max()` aggregations | Phase 4 | 8b | Stubs only — return errors, need full implementation |
| `forall`, `exists` quantifiers | Phase 4 | 8b | Not implemented — missing from CheckExpression enum |
| VS Code 2D preview tab | 7b/7f | 8g | Reuse floorplan editor in iframe |
| LLM evaluation (Phase 6) | Phase 6 | 8c | Token efficiency, success rate, benchmarks |

### 1.2 Known Limitations (V0.2)

These are observable behavioral asymmetries introduced in v0.2 that require future work:

#### Member Decompiler Round-Trip Limitation

`IfcMember` elements whose geometry is a vertical extrusion (extruded along Z) cannot be faithfully round-tripped through the decompiler. When the decompiler encounters an `IfcMember` without a recognizable beam-like axis representation, it falls back to emitting `position:` + `height:` properties instead of the canonical `from:/to:` axis form. The codegen for `Member` requires `from:/to:` and will skip such elements with a codegen warning rather than produce geometrically incorrect output.

**Impact**: Decompiled BML for axis-less Members is non-compilable (missing `from` property). The member is preserved in the BML text (with position/height/profile metadata) but will not produce IFC geometry on re-compilation.

**Future fix**: Add a `RawElementData::VerticalExtrusion` variant (similar to how columns use a point + height) to support vertical member geometry without requiring a horizontal axis. See `Specs_V0_3.md §1.2`.

### 1.3 V0.2 Features Working Well

These areas are mature and stable — no changes needed unless specific bugs are found:

- IFC codegen for all 15 element types (proper geometry, property sets, spatial containment)
- Floor templates with `{variable}` substitution
- Auto-ID generation (bare `Type:` without ID)
- 2D floorplan editor with undo/redo, bi-directional BML sync
- 3D viewer with element selection, highlight, transform gizmo
- Compliance checking engine (Check/CheckSuite blocks, computed properties)
- Furniture asset system (AssetLibrary/Asset/Part hierarchy)
- Real IFC import analysis with decompiler
- Mobile responsive web app with bottom sheets and touch gestures
- CodeMirror 6 code editor with BML syntax highlighting
- ChatBot integration with OpenAI-compatible API proxy

---

## 2. V0.3 Phase Roadmap

V0.3 focuses on four themes: **Refactoring** (reduce tech debt, improve architecture), **LLM Features** (evaluation, spec-to-check generation, prompt engineering), **Compiler Quality** (carrying forward v0.2 Phase 7h), and **Developer Experience** (project scaffolding, asset library).

```
Phase 8a: Compiler Quality (v0.2 7h carryover) ──── independent
Phase 8b: Refactoring & Architecture ─────────────── independent
Phase 8c: LLM Evaluation & Benchmarks ───────────── independent
Phase 8d: Spec-to-Check Generation ──────────────── depends on 8a, 8c
Phase 8e: Regulation-to-Check Library ───────────── depends on 8d
Phase 8f: LLM Prompt & Context Improvements ─────── depends on 8c
Phase 8g: VS Code & Editor Polish ───────────────── independent
Phase 8h: Project Scaffolding CLI ───────────────── independent
Phase 8i: Asset Library Expansion ───────────────── independent
```

---

## 3. Phase 8a — Compiler Quality (V0.2 Carryover)

**Goal**: Complete the remaining compiler quality items from Phase 7h.

### 3.1 Error Message Improvements

#### Fuzzy Storey Reference Matching

When `from_storey: Ground floor` doesn't match `### Storey: Ground Floor`, suggest the closest match using case-insensitive Levenshtein distance.

**Implementation**: In semantic analysis storey reference resolution (`containment.rs`), when a storey name doesn't match exactly, compute edit distance against all known storey names and suggest the closest if distance ≤ 3.

#### Show Provided Properties on Missing-Property Errors

Current: `"Wall is missing required property 'from'"`
Improved: `"Wall W-01 is missing required property 'from'. Provided properties: height, thickness, material. Did you mean 'from' instead of one of these?"`

**Implementation**: In `type_check.rs` `check_required_properties()`, collect the provided property names and include them in the diagnostic message.

#### Opening Overflow Validation

Validate that `offset + width` fits within the host wall's length. Flag `offset: 9m` + `width: 2m` on a 10m wall as an error.

**Implementation**: In semantic analysis, after wall axis length is computed, iterate child doors/windows and validate their offset + width ≤ wall length. Emit a warning (not error) when overflow detected, since the model might still be valid for visualization.

### 3.2 Grammar Cleanup

#### Ghost Type Resolution

`Opening`, `Grid`, and `Dimension` are accepted by the tree-sitter grammar but have no implementation. Decision needed:

- **Option A**: Remove from grammar (breaking change, but never functional). Emit helpful error: "Opening is not a supported element type. Did you mean Door or Window?"
- **Option B**: Implement as proper element types (Opening → IfcOpeningElement, Grid → IfcGrid, Dimension → IfcDimensionAnnotation)
- **Recommendation**: Option A for `Opening` and `Dimension` (rarely needed in BML's use case). Option B for `Grid` if structural grid support is desired.
- **Decision** Fully implement Opening and Grid as proper element types. Remove Dimension from grammar (not relevant to building modeling, can be added later if needed).

### 3.3 Validation Enhancements

#### Cross-Storey Reference Validation

Validate that `from_storey` / `to_storey` references on stairs and elevators point to existing storeys. Currently these references are stored as strings without validation.

**Implementation**: After all storeys are collected in semantic analysis, resolve storey name references and emit errors for unresolved ones. Suggest closest match using fuzzy matching from 3.1.

#### Material Reference Validation

Warn when `material:` references a material ID that isn't defined in any `Material:` block.

**Implementation**: Collect all defined material IDs during semantic analysis, then validate all `material:` property references against this set.

#### Coordinate Bounds Checking

Warn when element coordinates are far outside the building footprint. This catches common errors like misplaced decimal points or unit confusion.

**Implementation**: After all elements are resolved, compute the bounding box of the building. Flag elements whose coordinates are > 10x outside this bounding box. This should be a warning, not an error.

### 3.4 Diagnostic Infrastructure

#### File Path in Span/Diagnostic

Add file path tracking to `Span` and `Diagnostic` types for proper multi-file error attribution. Currently, diagnostics don't know which file they came from.

**Implementation**: Add `file: Option<PathBuf>` to `Span` struct. Populate during parsing for multi-file projects. Update all diagnostic consumers (CLI, LSP, web) to display file paths.

#### Multi-Location Diagnostics

Support diagnostics that reference multiple code locations. Example: "Element A references Element B, but B doesn't exist" should show both locations.

**Implementation**: Add `related: Vec<(Span, String)>` to `Diagnostic` struct. LSP already supports related information (`DiagnosticRelatedInformation`).

### 3.5 Linting Rules

Add configurable linting rules for suspicious but technically valid values:

| Rule | Trigger | Severity |
|------|---------|----------|
| `wall-thickness` | Wall thickness > 1.0m or < 0.05m | Warning |
| `door-width` | Door width > 3.0m or < 0.5m | Warning |
| `window-sill` | Sill height > 2.0m | Warning |
| `storey-height` | Storey height > 10m or < 2.0m | Warning |
| `column-aspect` | Column width/depth ratio > 5:1 | Warning |
| `beam-span` | Beam span > 20m without intermediate support | Warning |
| `space-door` | Walled space has no door | Warning |
| `wall-overlap` | Two straight walls on the same storey overlap (E407, see below) | Warning |

**Configuration**: Enable/disable per rule in `buildingml.toml`:

```toml
[lint]
wall_thickness = "warn"    # "warn", "error", "off"
door_width = "warn"
window_sill = "warn"
wall_overlap = "warn"
```

#### Wall Overlap (`wall_overlap`, E407)

Detects coincident or duplicate walls, e.g. a party wall declared once from each neighbouring space (found in `six_storey_residential.bml` by the USD BRep prototype, §14.3: `W-{f}-{n}01-E` `(12, 0) → (12, 8)` and `W-{f}-{n}02-W` `(12, 8) → (12, 0)`).

- Pure 2D line math on wall axes, per storey (storey-level and space-level walls together).
- Two walls overlap when their axes are parallel (both endpoints of one wall at the same offset from the other's axis, within 0.01 m), the overlap along the axis is > 0.01 m, and the perpendicular offset is < half their combined thickness (or ≤ 0.01 m for collinear axes). Direction does not matter.
- The warning is reported on the later wall, with related information pointing to the earlier one.
- L-junctions, T-junctions and collinear walls that only share an endpoint are not reported.

**Fixture cleanup**: all fixtures are free of overlapping walls (regression tests in `lint.rs` and the multi-file lint test). In `six_storey_residential.bml` (and its `_project/` and `_initial` copies) each shared wall is now declared once:
- Party walls between units are kept on the first unit only (e.g. `W-{f}-{n}01-E` stays, `W-{f}-{n}02-W` is removed).
- Corridor walls `W-{f}-CS` / `W-{f}-CN` are removed in favour of the unit walls that carry the entrance doors; the corridor space gets an explicit `boundary:`. Unit `{n}10` also gets an explicit `boundary:` since only two of its walls remain (boundary inference needs ≥ 3).
- Stairwell outer walls `WS-W` / `ES-E` are shortened to the 2 m beyond the end units.
- Ground floor: `W-GF-I1` and `W-GF-I15` are merged into `W-GF-I16` (doors moved with adjusted offsets).
- Floor 5: Unit 508 overlapped Unit 502 (x 16–24); 502 is now x 16–21 and 508 x 21–24.

**Limitations**: curved walls (`via:`) are skipped; walls on different storeys are not compared; each overlapping pair produces one warning (a wall overlapping several others is reported several times).

---

## 4. Phase 8b — Refactoring & Architecture

**Goal**: Reduce tech debt and improve code architecture for long-term maintainability.

### 4.1 Element Type Registry

Currently, adding a new element type requires changes to 15+ files. Refactor to a centralized registration pattern.

#### Design

Create an `ElementTypeDescriptor` struct that holds all per-type metadata:

```rust
pub struct ElementTypeDescriptor {
    // Identity
    pub name: &'static str,              // "Wall", "Column", etc.
    pub ifc_entity: &'static str,        // "IfcWall", "IfcColumn"
    pub auto_id_prefix: &'static str,    // "W-", "COL-"

    // Type checking
    pub required_properties: &'static [&'static str],
    pub optional_properties: &'static [PropertySchema],
    pub geometry_kind: GeometryKind,

    // LSP
    pub hover_text: &'static str,
    pub completion_snippet: &'static str,
    pub completion_properties: &'static [&'static str],

    // Rendering
    pub scene_color: [f32; 4],           // RGBA
    pub svg_css_class: &'static str,
    pub floorplan_draw: Option<fn(...)>,  // Custom draw function
}

pub enum GeometryKind {
    Box,           // Simple width x depth x height box
    AxisExtrusion, // Extruded along from→to axis
    Polygon,       // Polygon extrusion
    Custom(fn()),  // Custom geometry function
}
```

Replace scattered `match` arms across:
- `type_check.rs` → property schema lookup
- `containment.rs` → auto-ID prefix
- `ifc_writer.rs` → IFC entity generation
- `graph.rs` → scene graph mesh generation
- `floorplan.rs` → SVG rendering
- `hover.rs` → LSP hover text
- `completion.rs` → LSP completion snippets

**Net effect**: Adding a simple new element type becomes adding one `ElementTypeDescriptor` entry + a grammar keyword, instead of touching 15 files.

**Scope**: Focus on the 80% case. Custom geometry (roofs, stairs) still needs per-type functions, but the registry handles metadata, property schemas, and default geometry.

### 4.2 Codegen Module Decomposition - Comment SF: Reevaluate after 4.1 - at least regarding the per element files

The `ifc_writer.rs` file has grown large. Split into focused modules:

```
codegen/
  mod.rs              → Public API (generate_ifc)
  ifc_writer.rs       → Core writer logic, spatial hierarchy
  geometry.rs         → StepGeometryWriter, geometry primitives
  elements/
    mod.rs            → Element dispatch
    wall.rs           → Wall + opening IFC generation
    slab.rs           → Slab + property sets
    column.rs         → Column geometry
    beam.rs           → Beam geometry
    roof.rs           → Roof pitch geometry
    stair.rs          → Stair geometry
    door.rs           → Door entity
    furniture.rs      → Furniture + asset expansion
    transport.rs      → Elevator
    structural.rs     → Proxy, Member, Plate
  relationships.rs    → IfcRel* entities, cleanup
  property_sets.rs    → Pset injection
```

### 4.3 Test Infrastructure

#### Test Coverage Gaps

Based on audit, the following areas need better test coverage:

- **Compliance checker**: Only basic check evaluation tested. Need tests for:
  - Quantifier functions (count, sum, min, max, forall, exists)
  - Computed properties in check expressions
  - Check error reporting formats (HTML, SARIF)
  - Parametric checks
  - Check dependencies (`depends_on`)

- **Multi-file projects**: Limited test coverage for import resolution, cross-file references, and multi-file diagnostics.

- **Edge cases**: Template expansion with nested templates, circular imports, very large files (performance).

#### Test Organization

Consider a `tests/` directory reorganization:
```
tests/
  fixtures/           → BML test files (existing)
  integration/        → Full pipeline tests (parse → compile → validate)
  regression/         → Bug-specific regression tests
  benchmarks/         → Performance benchmarks (Phase 8c)
```

### 4.4 Error Handling Consolidation - SF: Low priority

Multiple error types across the codebase (`ParseError`, `SemanticError`, `CodegenError`, `CheckError`). Consider a unified error type with error codes for programmatic consumption.

Current error codes:
- E100: Unknown element type
- E101: Missing required section
- E102: Missing required property
- E103: Unknown property
- E104: Type mismatch
- E200+: Semantic errors

Add new codes for Phase 8a validations:
- E300: Opening overflow
- E301: Cross-storey reference unresolved
- E302: Material reference unresolved
- E303: Coordinate bounds exceeded
- E400+: Lint warnings

---

## 5. Phase 8c — LLM Evaluation & Benchmarks - Sf: This phase needs interactive development with close supervision, as the design may evolve based on findings. The outlined plan is a starting point but should be adapted as insights are gained.

**Goal**: Quantify how well LLMs can author BuildingML, identify pain points, and establish baselines for improvement.

This is the previously-planned Phase 6 that was never started. It's now more valuable because the chatbot integration (Phase 7g) provides a real-world context for the findings.

### 5.1 Token Efficiency Analysis

Compare token counts for equivalent building descriptions across formats:

| Format | Tool | Metric |
|--------|------|--------|
| BuildingML (.bml) | `bml compile` | tokens (cl100k_base) |
| ifcJSON | IfcOpenShell | tokens |
| STEP (.ifc) | Direct | tokens |
| Natural language | — | tokens (baseline) |
| IfcOpenShell (or Bonsai/Blender API) | IfcOpenShell/Bonsai/Blender API | tokens |

**Test buildings**:
1. `minimal.bml` — Single wall (baseline)
2. `two_storey_house.bml` — Simple residential
3. `six_storey_residential.bml` — Complex with templates
4. `compliance_checks.bml` — With compliance rules

**Expected outcome**: BML should be 5-10x more token-efficient than raw IFC STEP, and 2-3x more efficient than ifcJSON.

### 5.2 LLM Generation Success Rate

Test multiple LLMs on BML generation tasks:

#### Task Categories

1. **Create from description**: "Create a 3-bedroom apartment on a single floor"
2. **Modify existing**: "Add a bathroom between the kitchen and bedroom" (given existing BML)
3. **Fix errors**: Given BML with errors, ask LLM to fix them
4. **Compliance**: "Make this building comply with fire safety regulations"
5. **Template authoring**: "Create a template for a typical office floor"

#### Metrics

- **Parse success rate**: % of generated BML that parses without errors
- **Semantic validity**: % that passes semantic analysis
- **IFC compilation**: % that compiles to valid IFC
- **Spatial correctness**: Manual evaluation of whether the spatial layout matches intent

#### LLMs to Test

- GLM
- Claude
- GPT
- Gemini Pro

### 5.3 Edit Distance Analysis

For each LLM generation attempt that fails, measure:
- Levenshtein distance to the nearest valid BML
- Number of properties that need correction
- Types of errors (syntax, semantic, spatial logic)
- Whether the chatbot's error feedback loop can self-correct

### 5.4 Benchmark Infrastructure

Create a benchmark suite that can be re-run as the language evolves:

```
benchmarks/
  tasks/                    → Task descriptions (JSON)
  expected/                 → Expected outputs or validation criteria
  results/                  → Generated results per LLM/date
  run_benchmarks.py         → Orchestration script
  analyze_results.py        → Analysis and reporting
```

Each task is a JSON file:
```json
{
  "id": "create-3br-apartment",
  "category": "create",
  "description": "Create a single-storey 3-bedroom apartment...",
  "context": null,
  "validation": {
    "must_parse": true,
    "must_compile": true,
    "required_elements": ["Space(bedroom)", "Space(bedroom)", "Space(bedroom)", "Space(kitchen)", "Space(bathroom)"],
    "required_relationships": ["door between spaces"]
  }
}
```

---

## 6. Phase 8d — Spec-to-Check Generation

**Goal**: Enable LLMs to read building specifications (design briefs, architectural programs) and automatically generate BuildingML compliance checks.

This is a key differentiator: specs are textual → LLMs can read them → LLMs can write `Check:` blocks that enforce them.

### 6.1 Concept

A design brief says: "All offices must have at least 10m² of floor area and natural light (window-to-floor ratio > 0.1)."

An LLM should generate:

```markdown
Check: office_min_area
  description: "All offices must have at least 10m² of floor area"
  applies_to: Space
  where: usage == office
  assert: area >= 10.0

Check: office_natural_light
  description: "Offices must have window-to-floor ratio > 0.1"
  applies_to: Space
  where: usage == office
  assert: window_to_floor_ratio > 0.1
```

### 6.2 Spec Document Parsing

Support common specification formats:

1. **Plain text / Markdown**: Design briefs, architectural programs
2. **Table format**: Room data sheets (space name, area, requirements)
3. **Regulatory text**: Building code sections with requirements

The LLM context builder should be able to include spec documents alongside BML source.

SF: Parsing must be implemented using LLMs/VLMs, standard PDF parsing libraries such as pdfplumber or PyMuPDF are not good enough for this use case. Recommended tool: [docling](https://github.com/docling-project/docling) for document pre-processing and extraction before passing content to the LLM.

### 6.3 Check Generation Pipeline

```
Spec Document → LLM (with BML check syntax examples) → Check: blocks → Validate → Integrate
```

1. **Extract requirements**: LLM identifies quantitative requirements from spec text
2. **Map to BML properties**: LLM maps spec terms to BML property paths (area, height, usage, etc.)
3. **Generate Check blocks**: LLM writes syntactically valid `Check:` blocks
4. **Validate**: Compiler lints the generated checks (syntax + type checking)
5. **Integrate**: Checks are added to the project (inline or separate file)

### 6.4 Spec-to-Check Prompt Template

Design a system prompt for the chatbot that enables spec-to-check generation:

```
You are a building compliance expert. Given a design specification,
generate BuildingML Check: blocks that enforce the requirements.

Available check syntax:
- applies_to: <ElementType> — which elements to check
- where: <condition> — filter elements (optional)
- assert: <expression> — the requirement to enforce
- description: "..." — human-readable description

Available element types: Wall, Slab, Column, Beam, Window, Door, Roof,
Stair, Railing, Ramp, Furniture, Elevator, Space, Proxy, Member, Plate

Available properties: [per-type property list]
Available computed properties: clear_height, escape_route_length,
  window_to_floor_ratio, is_fire_compartment_boundary, structural_span

Comparison operators: ==, !=, <, >, <=, >=
Set membership: in [value1, value2, ...]
Functions: count(), sum(), min(), max()
Quantifiers: forall, exists
```

SF: We will also provide few-shot examples in the prompt for common requirement types (area, height, ratios) to improve generation quality.

### 6.5 Chat Integration

Add a dedicated UI flow in the chatbot:

1. User uploads or pastes a spec document
2. Chat shows "I found N quantitative requirements. Generating checks..."
3. LLM generates Check blocks
4. User reviews and approves/modifies
5. Checks are added to the project
6. Compliance report runs automatically

### 6.6 Validation of Generated Checks

After generation, automatically:
1. Parse the generated Check blocks (syntax validation)
2. Type-check property references (do the properties exist on the target element types?)
3. Run the checks against the current model (do they pass/fail as expected?)
4. Report coverage: which spec requirements have corresponding checks?

---

## 7. Phase 8e — Regulation-to-Check Library

**Goal**: Build a curated library of compliance checks derived from real building regulations.

### 7.1 Target Regulations

Start with well-documented, commonly-referenced building codes:

#### German Building Code (MBO / LBO)

| Regulation | Section | BML Check | Computed Property Needed? |
|-----------|---------|-----------|--------------------------|
| Min room height (habitable) | MBO §44 | `clear_height >= 2.50` | Yes (exists) |
| Min door width (escape route) | MBO §36 | `width >= 0.90` | No |
| Max escape route length | MBO §35 | `escape_route_length <= 35.0` | Yes (exists) |
| Min window area (habitable rooms) | MBO §47 | `window_to_floor_ratio >= 0.125` | Yes (exists) |
| Fire-rated walls between units | MBO §29 | `fire_rating in [REI30, REI60, REI90]` | No |
| Staircase width | MBO §34 | `width >= 1.0` | No |
| Railing height | MBO §38 | `height >= 0.90` | No |
| Accessible door width | DIN 18040-2 | `width >= 0.90` | No |

#### International Building Code (IBC) — Key Sections

| Regulation | Section | BML Check |
|-----------|---------|-----------|
| Min ceiling height | IBC 1208.2 | `clear_height >= 2.286` (7'6") |
| Min habitable room area | IBC 1208.3 | `area >= 6.5` (70 sq ft) |
| Egress door width | IBC 1010.1.1 | `width >= 0.813` (32") |
| Corridor width | IBC 1020.2 | `width >= 1.118` (44") |

#### Eurocode Structural Checks

| Regulation | Section | BML Check |
|-----------|---------|-----------|
| Min column dimension | EN 1992 | Profile width/depth ≥ 0.2m |
| Max beam span/depth ratio | EN 1992 | structural_span / depth ≤ 20 |


SF: The codes must be stored as separate BML files with `CheckSuite:` blocks, so they can be imported individually by projects. This also allows the LLM to reference them when generating new checks. The code PDFs will be provided as context for the LLM to extract new checks in the future.

### 7.2 Check Library Structure

```
checks/
  mbo/
    fire-safety.bml           → MBO §§28-35 fire safety checks
    escape-routes.bml         → MBO §§33-36 escape route checks
    room-requirements.bml     → MBO §§44-47 room dimension checks
    accessibility.bml         → DIN 18040-1/2 accessibility checks
  ibc/
    means-of-egress.bml       → IBC Chapter 10
    interior-environment.bml  → IBC Chapter 12
  eurocode/
    structural-concrete.bml   → EN 1992 basic structural checks
  custom/
    project-brief.bml         → Project-specific generated checks
```

Each file is a valid BML file with `CheckSuite:` blocks:

```markdown
CheckSuite: mbo_fire_safety
  description: "German Model Building Code — Fire Safety (MBO §§28-35)"
  jurisdiction: DE
  version: "2024"
  section: "MBO Part 3"

  Check: fire_rated_compartment_walls
    description: "Walls between dwelling units must be fire-rated (MBO §29)"
    applies_to: Wall
    where: is_fire_compartment_boundary == true
    assert: fire_rating in [REI30, REI60, REI90, REI120]

  Check: escape_route_max_length
    description: "Escape route length must not exceed 35m (MBO §35)"
    applies_to: Space
    assert: escape_route_length <= 35.0

  Check: escape_door_width
    description: "Doors in escape routes must be at least 0.90m wide (MBO §36)"
    applies_to: Door
    where: is_external == true
    assert: width >= 0.90
```

### 7.3 Check Import Mechanism

Projects reference regulation check suites via import:

```markdown
import checks from "checks/mbo/fire-safety.bml"
import checks from "checks/mbo/room-requirements.bml"
```

Or in `buildingml.toml`:

```toml
[checks]
suites = ["mbo/fire-safety", "mbo/room-requirements"]
```

### 7.4 LLM-Assisted Regulation Authoring

For regulations not yet in the library, the LLM can help:

1. User pastes regulation text
2. LLM extracts quantitative requirements
3. LLM generates `Check:` blocks
4. User validates against regulation source
5. Checks are added to the library

This creates a virtuous cycle: the more regulations are encoded, the more examples the LLM has for generating new checks.

### 7.5 Computed Property Extensions

Some regulations require computed properties not yet implemented. Extend the computed property system:

| Property | Description | Regulation Need |
|----------|-------------|-----------------|
| `corridor_width` | Width of corridor space (min dimension) | MBO §36, IBC 1020 |
| `room_depth` | Max depth from window wall | Daylight regulations |
| `occupant_load` | Estimated occupancy from area + usage | IBC Chapter 10 |
| `exit_count` | Number of exits from a space | IBC 1006 |
| `fire_compartment_area` | Total area of connected fire compartment | MBO §28 |

**Implementation**: Add to `computed.rs` as new `ComputedProperty` implementations. Each requires specific spatial analysis logic.

---

## 8. Phase 8f — LLM Prompt & Context Improvements

**Goal**: Improve the chatbot's ability to understand and modify BML, based on Phase 8c evaluation findings.

### 8.1 Context Builder Enhancements

The current context builder (`web/src/shared/chat/context-builder.ts`) sends:
- Current BML source
- Current storey
- Selected element
- Diagnostics
- View mode

Enhance with:

#### Semantic Model Context
Instead of raw BML source, send a structured summary:
```
Building: Office Block A (3 storeys)
  Ground Floor: 4 spaces (Lobby, Office A, Office B, Kitchen), 12 walls, 6 windows, 4 doors
  First Floor: 6 spaces, 18 walls, 9 windows, 6 doors
  Second Floor: 6 spaces, 18 walls, 9 windows, 6 doors
```

This gives the LLM a spatial overview without consuming excessive tokens.

SF: To be tested, but probably good, since the LLM can still refer to the full BML if needed for details.

#### Compliance Results
Include the current compliance check results so the LLM can suggest fixes:
```
Compliance: 2 of 5 checks failing
  - min_door_width: FAIL on Door D-NARROW (width=0.75, required>=0.90)
  - fire_rated_walls: FAIL on Wall W-INT-01 (no fire_rating, is internal)
```

#### Available Properties
Include a compact reference of available properties per element type, so the LLM doesn't hallucinate non-existent properties.

### 8.2 Few-Shot Examples

Based on Phase 8c benchmark results, identify the most common LLM failure modes and create targeted few-shot examples:

- Wall axis syntax (`from: (x1, y1) to: (x2, y2)`)
- Door/window nesting under walls
- Space hierarchy (storey → space → elements)
- Template usage
- Check syntax

Store as reusable prompt fragments in `web/src/shared/chat/examples/`.

### 8.3 Error Recovery Loop

When LLM-generated BML has errors, improve the feedback loop:

Current: Show lint errors in chat, user asks for correction.
Improved: Automatically re-prompt the LLM with:
1. The generated BML
2. The specific errors
3. The fix suggestion (from "did you mean?" hints)
4. Request for corrected version

Limit to 2 automatic retry attempts to prevent loops.

### 8.4 Structured Output Mode

For reliability, add a structured output mode where the LLM returns JSON instead of raw BML:

```json
{
  "action": "add_element",
  "storey": "Ground Floor",
  "space": "Office A",
  "element": {
    "type": "Door",
    "id": "D-NEW-01",
    "properties": {
      "offset": "3.0 m",
      "width": "0.90 m",
      "height": "2.10 m"
    },
    "parent_wall": "W-EXT-01"
  }
}
```

This can be validated before converting to BML, reducing parse errors.

SF: This seems overkill. If we need json structure we should have just designed the language to be json from the start. Instead, we can just ask the LLM to strictly follow BML syntax and provide detailed error feedback when it doesn't, which is more in line with the current design and likely sufficient with good prompt engineering and few-shot examples.
However, in the case, this step helps, we should consider changing the language to be JSON-based in the long term, as it would simplify LLM integration and reduce parsing complexity. This would be a major change, but could be worth it for v1.0 if the benefits are significant.

---

## 9. Phase 8g — VS Code & Editor Polish

**Goal**: Complete remaining editor integration work and polish the developer experience.

### 9.1 Interactive 2D Editor in VS Code

The read-only 2D preview (pan/zoom, cursor-driven storey selection) is already complete (Phase 7f). This item is about upgrading it to a full interactive editor:

- Embed the full floorplan editor (tools, element selection, drag, context menu) in the VS Code webview
- `postMessage` bridge for bi-directional BML sync (2D edits → update editor text, editor changes → re-render 2D)
- Undo/redo integration with VS Code's native undo stack

### 9.2 LSP Enhancements

Based on Phase 8a compiler improvements, enhance LSP:

- Fuzzy completion matching (type `wal` → suggest `Wall:`)
- Quick fix code actions for "did you mean?" suggestions
- Diagnostic quick fixes for compliance check failures
- Inline check results as code lenses

### 9.3 Web Editor Improvements

- **Spec document panel**: Upload/paste spec documents for Phase 8d spec-to-check generation
- **Compliance dashboard**: Visual summary of check pass/fail status
- **Check editor**: Dedicated UI for editing Check blocks with live preview of pass/fail
- **Regulation browser**: Browse the check library (Phase 8e) and add suites to project

---

## 10. Phase 8h — Project Scaffolding CLI

**Goal**: Provide a `buildingml init` command that creates a ready-to-use project directory with sensible defaults, reducing onboarding friction.

### 10.1 Command Interface

```bash
buildingml init my-building                  # Create project with defaults (residential)
buildingml init my-building --template office # Use office template
buildingml init my-building --flat           # Flat structure (no subdirectories)
buildingml init                              # Interactive mode (prompts for name, type)
```

### 10.2 Multi-File Project Structure (Default)

The default project layout uses **light nesting** — storeys in a `storeys/` subdirectory, reusable definitions (checks, assets) in their own subdirectories, and shared definitions (materials, templates) at the root alongside `project.bml`.

```
my-building/
  buildingml.toml        # Project configuration (units, IFC version, lint rules)
  project.bml            # Site + Building hierarchy, imports all other files
  materials.bml          # Shared material definitions
  templates.bml          # Floor templates (if applicable)
  .gitignore             # Ignore *.ifc, dist/, node_modules/
  storeys/
    ground-floor.bml     # Ground floor definition
    first-floor.bml      # Upper floor(s)
  checks/
    compliance.bml       # Compliance check suite
  assets/
    furniture.bml        # Furniture asset library
```

**`project.bml`** is the entry point (configured via `buildingml.toml` `entry` field, default: `"project.bml"`). It contains the spatial hierarchy (`# Site`, `## Building`) and imports all other files:

```markdown
import materials from "./materials.bml"
import templates from "./templates.bml"
import assets from "./assets/furniture.bml"
import checks from "./checks/compliance.bml"

# Site: My Building Site

## Building: Main Building

import ground from "./storeys/ground-floor.bml"
import first from "./storeys/first-floor.bml"
```

**Rationale for light nesting:**
- `storeys/` — the most numerous files in large projects; grouping prevents root clutter
- `checks/` — separates compliance rules that may be shared across projects
- `assets/` — separates reusable asset definitions from building-specific content
- `materials.bml`, `templates.bml` — kept at root because they're referenced frequently and there are typically only 1-2 of each

### 10.3 Flat Structure (--flat)

For simple projects or when nesting isn't desired, `--flat` puts everything in the root directory:

```
my-building/
  buildingml.toml
  project.bml
  materials.bml
  ground-floor.bml
  first-floor.bml
  checks.bml
  .gitignore
```

Flat projects still work with explicit imports in `project.bml`, but also work without a `project.bml` entry point — the compiler auto-discovers all `.bml` files in the root directory via `load_all_bml_files()`.

### 10.4 Reference Multi-File Fixture

Split the existing `six_storey_residential.bml` into a multi-file project at `tests/fixtures/six_storey_residential_project/` to serve as the reference architecture:

```
tests/fixtures/six_storey_residential_project/
  buildingml.toml
  project.bml            # Site + Building + imports
  materials.bml          # 4 material definitions
  storeys/
    ground-floor.bml     # Unique ground floor (lobby, parking, plant room)
    typical-floors.bml   # Template definition + Floors 1-4 (apply_template)
    top-floor.bml        # Unique penthouse floor (Floor 5)
    roof.bml             # Roof level with railings
  checks/
    residential.bml      # Compliance check suite
  assets/
    furniture.bml        # Furniture asset library (BasicDesk, BasicChair)
```

**Implementation notes:**
- Each storey file must redeclare `# Site` / `## Building` headings because `analyze_project()` analyzes each file independently before merging.
- `analyze_project()` merges headings with the same name from different files into one site / building / storey, so services can live in one file per discipline that redeclares the storeys (properties: the file loaded first wins per key, a storey `elevation` or `height` that differs from the first file's is warned (E201); storeys sorted by elevation). Same-named headings within one file stay separate, so a decompiled IFC with two unnamed buildings keeps both. Spaces are not merged: a `####` space redeclared in another file is a second space, so services files put their elements at storey level. Coverings take their default outline and height per file (§20), so a ceiling belongs in the file that declares its storey's `height` and walls.
- Template definitions must be in the same file as `apply_template` directives because `expand_templates()` operates on a single `SourceFile`. Therefore `templates.bml` was merged into `storeys/typical-floors.bml`.
- Materials defined in `materials.bml` are available cross-file via the project merge (no per-file import needed).

The single-file `six_storey_residential.bml` is kept intact as the comprehensive regression test. The multi-file version tests import resolution, cross-file material references, and project loading. 9 integration tests verify parity between single-file and multi-file versions.

### 10.5 Template System

Provide built-in templates for common building types:

| Template | Description | Generated Files |
|----------|-------------|----------------|
| `residential` (default) | Single-storey apartment | project.bml, materials.bml, storeys/ground-floor.bml (2 rooms, kitchen, bathroom) |
| `office` | Single-storey office | project.bml, materials.bml, storeys/ground-floor.bml (reception, 2 offices, meeting room) |
| `mixed-use` | Two-storey | project.bml, materials.bml, templates.bml, storeys/ground-floor.bml (commercial), storeys/first-floor.bml (residential) |

Templates are embedded in the CLI binary as string constants — no external file dependencies.

### 10.6 Interactive Mode

When `buildingml init` is called without a project name:

1. Prompt: "Project name:" (validate: no spaces, valid directory name)
2. Prompt: "Building type:" (residential / office / mixed-use)
3. Prompt: "Unit system:" (metric / imperial)
4. Prompt: "Structure:" (nested / flat)
5. Create project with selections

### 10.7 Implementation

- Add `init` subcommand to `bml-cli/src/main.rs`
- New module `bml-cli/src/init.rs` with template strings and directory creation
- `buildingml.toml` generation from `bml-core/src/config.rs` defaults
- Validation: directory doesn't exist (or `--force` flag), valid project name
- Create `tests/fixtures/six_storey_residential_project/` as multi-file reference fixture
- Add integration tests: compile, lint, and check the multi-file fixture

---

## 11. Phase 8i — Asset Library Expansion

**Goal**: Build a curated collection of common furniture assets with realistic dimensions and multi-part geometry, leveraging the v0.2 furniture asset system.

### 11.1 Asset Categories

#### Residential Furniture

| Asset | Dimensions (L×W×H) | Parts |
|-------|-------------------|-------|
| Single Bed | 2.0 × 0.9 × 0.45m | Frame, mattress, headboard |
| Double Bed | 2.0 × 1.4 × 0.45m | Frame, mattress, headboard |
| King Bed | 2.0 × 1.8 × 0.45m | Frame, mattress, headboard |
| Wardrobe | 1.2 × 0.6 × 2.0m | Body, doors (2) |
| Nightstand | 0.5 × 0.4 × 0.5m | Body, drawer |
| Desk | 1.2 × 0.6 × 0.75m | Top, legs (4) |
| Dining Table | 1.6 × 0.9 × 0.75m | Top, legs (4) |
| Dining Chair | 0.45 × 0.45 × 0.85m | Seat, backrest, legs (4) |
| Sofa (3-seat) | 2.0 × 0.85 × 0.85m | Frame, seat cushions, back cushions |
| Coffee Table | 1.0 × 0.5 × 0.4m | Top, legs (4) |
| Bookshelf | 0.8 × 0.3 × 1.8m | Frame, shelves (5) |

#### Kitchen Fixtures

| Asset | Dimensions | Parts |
|-------|-----------|-------|
| Kitchen Counter | 0.6 × 0.6 × 0.9m | Base cabinet, countertop |
| Sink Unit | 0.8 × 0.6 × 0.9m | Base cabinet, countertop, basin |
| Stove/Oven | 0.6 × 0.6 × 0.9m | Body, cooktop, oven door |
| Refrigerator | 0.7 × 0.7 × 1.8m | Body, door |

#### Bathroom Fixtures

| Asset | Dimensions | Parts |
|-------|-----------|-------|
| Toilet | 0.7 × 0.4 × 0.4m | Bowl, tank, seat |
| Bathtub | 1.7 × 0.75 × 0.6m | Tub body |
| Shower Tray | 0.9 × 0.9 × 0.1m | Base |
| Washbasin | 0.6 × 0.45 × 0.85m | Basin, pedestal |

#### Office Furniture

| Asset | Dimensions | Parts |
|-------|-----------|-------|
| Office Desk | 1.6 × 0.8 × 0.72m | Top, legs (4), modesty panel |
| Office Chair | 0.65 × 0.65 × 1.1m | Seat, backrest, base (cylinder) |
| Meeting Table | 2.4 × 1.2 × 0.72m | Top, legs (4) |
| Filing Cabinet | 0.4 × 0.6 × 1.3m | Body, drawers (4) |

### 11.2 Asset Library File

All assets defined in `assets/common-furniture.bml` using the existing v0.2 asset syntax:

```markdown
# AssetLibrary

## Asset: SingleBed
description: "Single bed with headboard"
category: bedroom

### Part: Frame
  geometry: box(2.0, 0.9, 0.15)
  position: (0, 0, 0.15)

### Part: Mattress
  geometry: box(1.9, 0.85, 0.2)
  position: (0.05, 0.025, 0.3)

### Part: Headboard
  geometry: box(0.05, 0.9, 0.5)
  position: (0, 0, 0.15)
```

### 11.3 Integration

- **Auto-include**: `buildingml.toml` option to auto-include the asset library:
  ```toml
  [assets]
  include_common = true    # Include built-in common furniture library
  ```
- **Usage**: `Furniture: F-01` with `asset: SingleBed` references the library asset
- **Import**: Alternative explicit import: `import assets from "assets/common-furniture.bml"`

### 11.4 Testing

- Parse test: all assets parse without errors
- IFC codegen test: all assets compile to valid IFC
- Scene graph test: all assets generate correct mesh nodes
- Visual test: render each asset in 3D viewer for dimension verification

---

## 12. Success Criteria

(Renumbered from §10 to accommodate new phases.)

### Phase 8a — Compiler Quality
- All 10 remaining Phase 7h items implemented and tested
- Error messages include context (provided properties, suggestions)
- `buildingml.toml` supports lint rule configuration
- No ghost types in grammar (resolved or removed)

### Phase 8b — Refactoring
- `ElementTypeDescriptor` registry replaces 80%+ of scattered match arms
- Adding a new simple element type requires ≤ 3 file changes (registry entry + grammar + test)
- `ifc_writer.rs` split into < 500-line modules
- Test coverage for compliance checker quantifiers and computed properties

### Phase 8c — LLM Evaluation
- Token efficiency benchmarks for 4+ test buildings across 3 formats
- LLM generation success rate across 10+ tasks and 3+ LLMs
- Published results document comparing BML vs alternatives
- Benchmark infrastructure that can be re-run

### Phase 8d — Spec-to-Check
- Working spec-to-check pipeline via chatbot
- 5+ example spec documents with generated checks
- Check validation (syntax + type check) for generated output
- Chat UI flow for spec upload and check generation

### Phase 8e — Regulation Library
- MBO fire safety and room requirements check suites (10+ checks)
- IBC basic checks (5+ checks)
- Import mechanism for check suites
- Documentation for adding new regulations

### Phase 8f — LLM Improvements
- Context builder sends semantic summary (not just raw BML)
- Auto-retry on parse errors (up to 2 attempts)
- Few-shot examples for top 5 failure modes
- Measurable improvement in LLM generation success rate vs Phase 8c baseline

### Phase 8g — Editor Polish
- VS Code 2D preview tab working
- Quick fix code actions for common errors
- Compliance dashboard in web editor

### Phase 8h — Project Scaffolding
- `buildingml init <name>` creates working project directory
- 3 built-in templates (residential, office, mixed-use)
- Generated `model.bml` parses and compiles to valid IFC
- Interactive mode works when no arguments given

### Phase 8i — Asset Library
- 25+ common furniture assets with multi-part geometry
- All assets parse, compile to IFC, and render in 3D
- Auto-include via `buildingml.toml` configuration
- Assets cover residential, kitchen, bathroom, and office categories

---

## 13. Open Questions

1. **Check library distribution**: Ship as part of the npm package / crate, or separate repository? --- Discuss
2. **Spec document storage**: Where to store uploaded spec documents? Server-side or client-side only? --- Discuss
3. **LLM provider flexibility**: Currently OpenAI-compatible only. Support Anthropic API directly? --- No will be handled through OpenRouter
4. **Structured output vs text**: Should spec-to-check generation use structured output (JSON) or text generation (BML)? --- Discuss
5. **Regulation versioning**: How to handle different versions of the same regulation (MBO 2020 vs 2024)? --- Discuss

---

## 14. Research Note — NVIDIA USD BRep as a Geometry Kernel

_Status: exploratory (October 2026). Not on the v0.3 roadmap; candidate for a post-v0.3 phase._

### 14.1 What It Is

In October 2026 NVIDIA released **USD BRep** (`pip install usd-brep`, repo `NVIDIA-Omniverse/usd-brep`), an open-source solid modelling library built on **SMLib**, the commercial NURBS/BRep kernel NVIDIA acquired in 2022 (originally from Solid Modeling Solutions; ClassCAD has built on it for 20+ years).

- **License**: Apache 2.0 — compatible with BuildingML's MIT license. Bug reports accepted, external contributions not (yet).
- **Bindings**: C++ and Python. Python wheel 1.0.0 is CPython 3.12 only, Linux/Windows x86_64 + aarch64 (no macOS, no WASM).
- **Operations**: primitives, extrude/revolve/sweep/pipe/loft, booleans (incl. non-manifold and polygon-mesh booleans), fillets/chamfers, shell/offset, intersections, plane sections, healing/stitching, mass properties, tessellation.
- **OpenUSD**: exact BRep data is read/written as an `omniSolid` `BrepArray` USD schema; tessellated output is plain `UsdGeom.Mesh`. A bundled validator checks the schema.
- **Ecosystem**: a community Rust port (`solidcore`) and an MCP server (from the ClassCAD team) appeared within days of release. Neither has been evaluated here.

Sources: [DEVELOP3D](https://develop3d.com/cad/nvidia-launches-usd-brep-geometry-kernel-for-solid-modelling/), [3D Printing Industry](https://3dprintingindustry.com/news/nvidia-opens-cad-modelling-kernel-what-could-it-mean-for-ai-assisted-design-and-3d-printing-255720/), [AlphaSignal](https://alphasignal.ai/news/nvidia-opens-usd-brep-s-cad-kernel-to-developers-under-apache-2-0), PyPI `usd-brep` README.

### 14.2 Why It Is Relevant ("LLM-friendly kernel")

BuildingML deliberately has **no geometry kernel** (parametric boxes/extrusions + `IfcOpeningElement`). That keeps the compiler small and WASM-friendly but blocks exact booleans, clash detection, accurate quantities and real section cuts. USD BRep is interesting for an LLM-first tool because:

1. **The API is easy for LLMs to call.** Functions are flat, typed, and grouped by module (`primitives`, `booleans`, `sweeps`, …). Docstrings state ownership explicitly (`Consumes:` / `Pure:` / `Mutates:`), list valid ranges, and name the SMLib call each one wraps. An LLM (or an MCP tool layer) can drive it without hidden state.
2. **It keeps the "LLM writes intent, the compiler writes geometry" split.** BML stays the LLM-facing language. The kernel sits behind the compiler, the same way IFC codegen does now. LLMs never write NURBS.
3. **Exact geometry gives better feedback to the LLM.** Net volumes, overlaps and section cuts are numbers a checker or an error-recovery loop (§8.3) can report back to the model.
4. **Open format.** Exact BRep in OpenUSD instead of a proprietary kernel format, with Apache 2.0 licensing.

### 14.3 Prototype Results

`prototypes/usd_brep/bml_to_brep.py` reads `buildingml model` JSON and builds walls (door/window openings cut with real booleans), slabs (polygon sweep), columns and beams. It writes exact BRep USD, mesh USD and a volume report. See `prototypes/usd_brep/README.md`.

| Fixture | Solids | Errors / non-manifold | Wall net volume | Openings removed | Runtime |
|---|---|---|---|---|---|
| `two_storey_house.bml` | 37 | 0 / 0 | 63.0 m³ (gross 71.4) | 8.3 m³ | ~0.2 s |
| `six_storey_residential.bml` | 101 (walls unioned per storey) | 0 / 0 | 1507.5 m³ (gross 1659.6) | 152.0 m³ | ~3 s |

Mesh USD opened cleanly in an independent `usd-core` 26.x install.

**Finding**: unioning the walls of each storey showed ~87.5 m³ of overlapping wall volume per typical floor (≈33%) in `six_storey_residential.bml`. Pairwise intersections traced this to **party walls being declared twice**, once by each neighbouring unit (e.g. `W-{f}-{n}01-E` (12,0)→(12,8) and `W-{f}-{n}02-W` (12,8)→(12,0)). This is now caught by the `wall_overlap` lint rule (E407, §3.5), and the fixtures have been cleaned up. Quantity take-off and IFC output therefore double-count these walls. This is a typical LLM authoring error, and exact booleans catch it for free.

### 14.4 Constraints / Risks

- **No WASM**: native C++ only. It cannot replace the in-browser scene graph. It would have to be a server-side or CLI-side optional backend (the same position as IfcOpenShell validation today).
- **Python 3.12 only** for the wheel. Rust integration would go through the C++ API via FFI or through the community Rust port (not yet assessed).
- **IFC mapping**: exact NURBS BRep → `IfcAdvancedBrep` (IFC4) is possible, but viewer support is uneven. Tessellated `IfcTriangulatedFaceSet` or `IfcFacetedBrep` is the safer first target. Keep parametric `IfcExtrudedAreaSolid` + `IfcOpeningElement` as the primary output; IFC consumers expect it.
- **Maturity**: 1.0 release, NVIDIA-controlled roadmap, no external contributions.
- **Bundled OpenUSD 25.11**: cannot share a process with `usd-core` 25.11 (symbol clash); other versions are fine.

### 14.5 Possible Integration Paths (ranked)

1. **Geometry validation pass (low effort, high value)**: optional `buildingml geocheck` (or a `check.sh` step, like IfcOpenShell). It would detect overlapping/duplicate walls, openings that cut through wall ends, element clashes, and report net quantities. Results become diagnostics with spans.
2. **Computed properties for compliance checks**: exact `net_volume`, `net_area`, and section-based `clear_height`/`window_to_floor_ratio` via the `ComputedProperty` trait.
3. **OpenUSD export**: `buildingml compile --format usd` for Omniverse and other USD pipelines (mesh + optional exact BRep).
4. **Kernel-backed IFC geometry** for elements where parametric approximations fall short (complex roofs, curved walls, furniture assets): tessellated BRep output.
5. **LLM tool use**: expose a small set of kernel operations (section, measure, clash) as chat tools, so the assistant can check its own BML edits.

Independent of the kernel, the **`wall_overlap` lint rule** (E407, §3.5) now catches the duplicate-wall finding above with 2D line math.

---

## 15. IFC Roundtrip (IFC → BML → IFC)

Goal: `buildingml decompile` followed by `buildingml compile` reproduces every element of a real IFC file with the same class, storey, name, GlobalId and geometry extent.

### 15.1 BML Additions

| Property / syntax | Meaning |
|---|---|
| `guid: "..."` | IFC GlobalId, kept on write |
| `ifc_name: "..."`, `ifc_class: IfcCurtainWall` | Original name and class when they differ from the BML defaults |
| `predefined_type: shear` | IFC `PredefinedType` in snake_case, written uppercase in the class's own attribute slot (`.SHEAR.`); a roof without it takes it from `type`. Not validated against the class's enumeration (IfcOpenShell validation catches wrong values). The decompiler leaves out `NOTDEFINED`, `USERDEFINED` (ObjectType is not kept) and the default `FLAT_ROOF`, and reads it from IFC4 files only (IFC2x3 keeps other enumerations in that slot, e.g. a proxy's CompositionType) |
| `part_of: <id>` | Element is a part of `<id>` (`IfcRelAggregates`), e.g. curtain wall panels, stair flights |
| `representation: none` | Product without body; clears the type's required properties |
| `opening: <id>` | Door/window fills the named opening; with it, a door/window may sit under any host (e.g. a roof window under a roof) |
| `material: <id>` on any type | `IfcMaterial` + `IfcRelAssociatesMaterial`; a `Material:` definition's `ifc_name` keeps the IFC name the id had to change |
| `boundary:` + `z_offset:` + `height:` on any type | Vertical prism, used when no parametric shape fits |
| Elements under `#### Space` | Contained in the `IfcSpace` (`IfcRelContainedInSpatialStructure`) |
| `"a \"b\" c:\\d"` | String escapes `\"` and `\\`; any other backslash is kept (`"C:\dir"`). A string cannot end in a single backslash |
| `part_of: RF-41021` | Hyphenated ids are valid property values (`element_ref` token) |
| Roof `thickness`, `is_external` | Flat roof thickness (default 0.15 m) |
| Several `## Building` headings under `# Site` | One `IfcBuilding` each (ettenheim: 195 buildings) |
| Elements before the first `###` under `## Building` | Building-level elements, contained in the `IfcBuilding` (ettenheim: elements without storeys) |
| Elements before the first `##` under `# Site` | Site-level elements, contained in the `IfcSite` |
| `#### Space` under `## Building` or `# Site` | Space aggregated directly under the `IfcBuilding` / `IfcSite` (fantasy_office_building_2: 22 site spaces). A space attaches to the nearest preceding Site, Building or Storey heading |
| `ifc_name:` on a storey | Original storey name when it is not a valid heading (surrounding whitespace) |
| `boundary:` on `Grid` | Grid with a prism body (sixty5 plumbing: drain gratings exported as `IfcGrid`) |
| `opening:` across storeys | A door/window may fill an opening of a host on another storey |

### 15.2 Geometry Fitting

The reader alone keeps only the parametric geometry it can read from the IFC. The CLI then meshes each element with ifc-lite and `decompile/mesh_fit.rs` compares the BML scene bbox with the mesh bbox (tolerance 0.02 m). On a mismatch:

1. Parametric fit: wall axis + thickness (2D PCA, footprint area within 3% of length × thickness), beam/member 3D axis + rectangular section, axis-aligned rectangular column.
2. Otherwise a prism from the mesh footprint (top-face outline, else convex hull).

Spaces take boundary, `z_offset` and height from their mesh, except a space whose boundary the reader read as an exact B-spline outline (§28.4).

Reader behaviour for real-world files:

- STEP entities may span several lines (statements end at `;` outside strings)
- Length unit from `IfcProject` units (mm, cm, m, feet, inch)
- Property values that do not fit the BML property type are dropped
- Products whose only representations are helpers (Axis, FootPrint, Box, Annotation, ...) get `representation: none`; elements still missing a required property after fitting are dropped
- An element contained in two structures keeps the first
- IFC2x3-only classes are mapped to IFC4 (`IfcElectricDistributionPoint` → `IfcElectricDistributionBoard`), since the writer writes IFC4
- References to missing entities are nulled before meshing, so ifc-lite does not stop at them
- Profiles, axes and swept-disk directrices with B-spline curves are read exactly as `boundary:` / `path:` with `nurbs` items (§28.4)

### 15.3 Results

`scripts/ifc_roundtrip.py` (IfcOpenShell world bboxes per GlobalId, tolerance 0.05 m):

| Model | Units | Elements matched | IfcOpenShell geometry check |
|---|---|---|---|
| `model_1.ifc` | mm | 1484 / 1484 | valid |
| `model_2.ifc` | cm | 2148 / 2148 | valid |
| `model_3.ifc` | m | 743 / 743 | valid |

Materials (IfcOpenShell `get_material` per GlobalId, same name): 1245/1245, 1632/1878, 574/586. The rest had Revit's placeholder material `<Unnamed>` and now carry the name of their surface style (`Glass`, `Oak Flooring`), see §16.3. Elements without any IFC material but with a surface style gain one (model_3: 42).

Benchmark models (`ifc_bench_models/`, 50 files, arc/str/mep/plumbing/ventilation/heating/electrical/fire/sprinkler/kitchen/facade/city). All elements are found by GlobalId and sit in the right container in every readable file. 43 files match 100% on storey and bbox:

4351, ac20, city_house_munich, dental_clinic arc/str, digital_hub (4), duplex arc/mep, fantasy_hotel_1/2, fantasy_office_building_1/2/3, fantasy_residential_building_1, fzk_house, molio, samuel_macalister arc, schependomlaan, sixty5 (all 7), smiley_west, wbdg_office (3), west_riverside_hospital arc/elec/fire/str (IFC2x3 and IFC4), mech/plumb/sprinkle (IFC2x3).

| Model | Elements | Storey ok | Bbox ok | Cause of misses |
|---|---|---|---|---|
| hitos arc | 2839 | 2839 | 2836 | 2 slabs and 1 wall bbox off (not investigated) |
| ettenheim_gis city | 3581 | 3581 | 3575 | ifc-lite circle trimming: 4 half-circle walls meshed as full circles, 2 arched openings |
| samuel_macalister mep | 2175 | 2175 | 1995 | Duct/pipe fittings with two Body representations, ifc-lite meshes one |
| west_riverside_hospital mech IFC4 | 19670 | 19670 | 18754 | Duct/pipe fittings: ifc-lite and IfcOpenShell disagree (the IFC2x3 export matches 100%) |
| west_riverside_hospital plumb IFC4 | 9121 | 9121 | 9106 | Same |
| west_riverside_hospital sprinkle IFC4 | 13490 | 13490 | 13358 | Same |
| dental_clinic mep | - | - | - | Source corrupt (NUL bytes) in all three Hugging Face mirrors (`sylvainHellin`, `quenfly`, `SiloLink/ifc-bench`) |

`molio/arc.ifc` was replaced by the intact copy from `sylvainHellin/ifc-bench` (the earlier copy had NUL bytes). In place of dental_clinic mep, the buildingSMART sample files `Clinic_Electrical.ifc` (2384 elements), `Clinic_HVAC.ifc` (3967) and `Clinic_Plumbing.ifc` (6587) match 100%. Building- and site-level contents (§15.1) fixed the storey of all 3581 ettenheim elements and the 22 site spaces of fantasy_office_building_2.

Upgrading ifc-lite 4.1 → 4.7 raised fantasy_office_building_1 from 1779 to 1787 and west_riverside_hospital plumb IFC4 from 7650 to 9106.

### 15.4 Limitations

- Prisms approximate sloped, curved and profiled shapes (I-section columns, sloped roofs, stairs) by their extruded footprint; the bbox matches, the shape does not
- Grids without a body are not fitted
- Building- and site-level elements and spaces are placed at elevation 0 with a height of 3 m (scene, `boundary: auto`). The SVG floorplan, the web 2D editor and the VS Code 2D preview show them as extra plans after the storeys (one per building or site that has such contents). Regenerating a building or site plan in the web editor keeps the heading and the lines before its first element and rewrites the rest; a storey heading is rewritten with only name, elevation and height (other storey properties such as `guid:` and `ifc_name:` are lost). Element blocks whose id the editor did not import (beams, proxies, MEP, walls without an axis, ...) and bare `Type:` blocks of types other than Wall, Slab, Column and Furniture are kept (taken once on import): plan-level ones before the first space, space-level ones under their space heading. A bare `Wall:`, `Slab:`, `Column:` or `Furniture:` the editor could not import (e.g. a wall without `from:`), comments between elements and the original space names are lost; a storey with `apply_template:` is written out inline, so template elements the editor does not import (beams, railings) are lost. Coordinate bounds checks treat each building (storeys plus building-level contents) and each site's own contents as separate coordinate frames
- An element contained in several structures keeps only the first in file order
- Arcs and curved shapes depend on the ifc-lite mesh; wrong curve trimming there becomes a wrong prism (ettenheim: 4 half-circle walls meshed as full circles, 2 arched openings whose `IfcTrimmedCurve` arc on a rotated circle is taken the long way round)
- Only the first Body representation is meshed (samuel_macalister mep fittings)
- Only `IfcElectricDistributionPoint` is mapped from IFC2x3 to IFC4; other IFC2x3-only classes are written as is and IfcOpenShell skips them
- Sources that IfcOpenShell cannot open cannot be compared (dental_clinic mep: corrupt in all Hugging Face mirrors)
- Only the property sets mapped in `pset_mapping.rs` roundtrip
- `IfcDistributionPort` is dropped
- The project GlobalId is not kept; an empty building name becomes "Building"
- `decompile_ifc()` without the CLI mesher (e.g. WASM) cannot rebuild prism geometry
- Walls thinner than the `wall_thickness` lint minimum raise E401 warnings (model_1)
- Materials: layer sets roundtrip as layered materials (§18); constituent and profile sets are read as their first `IfcMaterial` and written as a single `IfcMaterial`, fractions and profiles are lost
- The BML writer rounds numbers to 4 decimals (0.1 mm), also small pset values

## 16. Material Appearance

The BML preview, the IFC writer and the IFC preview (`ifc-scene`) take material looks from one module, `crates/bml-core/src/scene/appearance.rs`, so a BML model and the IFC compiled from it render alike.

### 16.1 Properties

| `Material:` property | Meaning |
|---|---|
| `color: "#rrggbb"` | sRGB base colour |
| `opacity: 0.3` | 0 (invisible) to 1 (opaque) |
| `specular: 0.5`, `shininess: 64` | Blinn-Phong highlight strength (0-1) and exponent |

A look comes from, in order: these properties, a keyword match of `ifc_name` / id / `category` against the palette, the element type default (wall plaster, slab/column/beam/stair/ramp concrete, member/plate/railing steel, window glass, door panel, roof asphalt shingle, furniture wood). Window frames are aluminium, door frames oak. Furniture asset parts use their `material:`, else the element's look. An element `material:` naming an undefined id is still matched against the palette.

### 16.2 Palette

25 entries, more specific keywords first (`plywood` before `wood`, `masonry unit` before `concrete`). Colours are the defaults Autodesk Revit writes into IFC surface styles, read from `tests/fixtures/model_*.ifc` (concrete, glass, aluminium, gypsum, CMU, oak, ...). Where Revit has no single default (brick, steel, copper, tile, stone) the base colour from physicallybased.info is used, converted from linear to sRGB.

### 16.3 IFC

- Codegen writes one `IfcSurfaceStyle` (`IfcSurfaceStyleRendering` with colour, transparency, specular) per `IfcMaterial` with a look, attached via `IfcMaterialDefinitionRepresentation`
- `ifc-scene` keeps the file's own surface styles and uses the palette only for elements without one (by material name, else by IFC class)
- Revit writes pure black (`IfcColourRgb(0,0,0)`) for materials whose colour was never set (model_1 "Concrete, Cast-in-Place gray"). The style reader treats black as unset when the style name matches a palette entry and does not say "black", for both previews and `decompile`
- `decompile` sets each element's `material:` to its area-dominant surface style and writes that style's colour, opacity, specular and shininess into the `Material:` block. Colour match of the decompiled BML preview with the original IFC preview: model_1 97.0%, model_2 99.2%, model_3 100% of elements (the rest are proxies/furniture with two styles of equal area)

### 16.4 Shading and Openings

- Both renderers scale the material `specular` by 0.25 and no longer add it to glass opacity. Revit writes specular 0.5 on nearly every material, which at full strength washed floors and glazing out to white
- `Opening` elements are voids and are not drawn. In a wall, a `boundary` opening is projected onto the wall axis and cut like a door/window; in a slab or plate, `boundary` openings inside the outline become holes (triangulated with earcut, largest first, skipping any that overlap an accepted hole). Before, they were drawn as translucent prisms coplanar with the host faces (z-fighting)

### 16.5 Limitations

- No textures: the fixtures contain no `IfcSurfaceStyleWithTextures` / `IfcImageTexture`, and both renderers draw flat colours
- Window and door panels with a prism geometry (from `decompile`) are one mesh, so they take one colour (no separate frame)
- Only one look per element: a layered material shows its first layer (BML), a layer set from another tool its area-dominant style (`ifc-scene`)
- `decompile` matches styles to materials by name, so two styles with the same name share one colour
- A window's `material:` colours its pane; frames are always aluminium (door frames oak)
- `ifc-scene` uses the palette only for elements with no styled part; unstyled parts of styled elements keep the neutral default grey
- Slab holes must be disjoint: an opening overlapping an earlier one (other than fully nested) is skipped. Openings in walls without an axis, roofs and other prism elements are not cut (and no longer drawn). A wall opening always cuts the full wall thickness, also for a recess

## 17. IFC Preview of Large Files (VS Code)

Tested on the West Riverside Hospital models in `ifc_bench_models/`.

- `ifc-scene` meshes every `IfcProduct` class that can carry geometry (ifc-lite `has_geometry_by_name`), minus spatial containers, openings and voiding features, ports, annotations and structural analysis items (`SKIPPED` in `ifc_scene.rs`), so MEP classes (`IfcPipeSegment`, `IfcDuctFitting`, ...) render. References to undefined entities are replaced by `$` before meshing (`null_dangling_refs`); the West Riverside Hospital MEP files point the site placement at a missing `#19`, which failed every element. B-spline curves, which ifc-lite cannot mesh, are rewritten into `IfcIndexedPolyCurve`s first (`bsplines_to_polycurves`, §28.3)
- IFC scenes are posted packed (`SceneMesh.positions`/`normals` as `Float32Array`, `indices` as `Uint32Array`, sent as binary) in `sceneChunk` messages of ~32 MB plus a closing `updateScene`. As one JSON message the West Riverside Hospital architecture model (2.6M vertices, 260 MB) never reached the webview. Renderers read either form through `meshArrays()` (`scene-loader.ts`)
- Each of those messages holds all its meshes as views into **one** `ArrayBuffer`. VS Code's extension-host RPC writes a message's buffer count as a uint8, so a message with 255+ buffers silently drops meshes or fails
- The IFC to preview is read from the active tab (`activeIfcTab()`), not only `activeTextEditor`: VS Code does not hand files above 50 MB to extensions, so a large IFC never becomes a text editor

## 18. Layered Materials

LOD 300 build-ups: a `Material:` with a `layers:` list (or `type: layered`) is a layer set. Each item names a `Material:` and its thickness.

```markdown
Material: ext_wall
  ifc_name: "External wall 300"
  layers:
    - mineral_wool: 0.14 m
    - concrete: 0.15 m
    - plaster: 10 mm
```

### 18.1 Orientation

The first layer is the exterior or top side:

| Element | IFC | First layer |
|---|---|---|
| Wall with `from`/`to` | `IfcMaterialLayerSetUsage(.AXIS2., .POSITIVE., -thickness/2)` | right of the axis direction (outside for a counter-clockwise outline) |
| Slab, Plate | `IfcMaterialLayerSetUsage(.AXIS3., .NEGATIVE., 0)` | top (the body extrudes down) |
| Flat Roof, Covering | `IfcMaterialLayerSetUsage(.AXIS3., .NEGATIVE., thickness)` | top (the body extrudes up) |
| Others (pitched roofs, prism walls, beams, ...) | `IfcMaterialLayerSet` associated directly | - |

Each layer material is written once as an `IfcMaterial` (with its surface style) and shared between sets; `ifc_name` names the `IfcMaterialLayerSet`.

### 18.2 Lint

- E301: a layer names an undefined material
- E306 (warning): a layered material without a valid `layers:` list, a layer that is itself layered, a negative thickness (0 is allowed for membranes), or an element `thickness` that differs from the layer sum by more than 0.5 mm. Checked after merging all project files, so materials may come from imports

### 18.3 Other Uses

- Scene: the element takes the look of its first layer; the inspector lists the layers under "Material Layers"
- `decompile`: `IfcMaterialLayerSetUsage` / `IfcMaterialLayerSet` become a layered `Material:` named after `LayerSetName` (else `LayerSet_<id>`); a set with a layer lacking material or thickness falls back to the first `IfcMaterial`. The surface style pass does not replace a layered material
- Units: layer thicknesses (and all `NumberWithUnit` values in material and element bodies) are converted to metres; before, `10 mm` was read as 10 m

### 18.4 Common Property Sets

`is_external`, `load_bearing`, `fire_rating`, `thermal_transmittance`, `acoustic_rating` are written to `Pset_<Type>Common` (Wall, Slab, Column, Beam, Window, Door, Roof, Stair, Railing, Plate, Member). `LoadBearing` only for Wall, Slab, Column, Beam, Plate, Member; `ThermalTransmittance` and `AcousticRating` only for Wall, Slab, Roof, Window, Door, Plate. Window `u_value` maps to `ThermalTransmittance`. `fire_rating` accepts the EN 13501-2 classes R/EI/REI 30-120 and T30-T90 as identifiers, anything else as a string.

### 18.5 Limitations

- `AcousticRating` is written as `IfcReal` (dB); IFC4 types it as `IfcLabel`
- No constituent sets (`IfcMaterialConstituentSet`) or profile sets
- Layer `IsVentilated`, `Category` and `Priority` are not written
- Material `color` accepts only `"#rrggbb"`; a tuple like `(0.7, 0.7, 0.7)` is ignored without a warning

## 19. Wall Joins

Straight walls of one storey (or building/site level) are joined in plan by `codegen/wall_joins.rs`. The IFC writer and the 3D scene both draw walls with the joined ends; the BML axes stay unchanged.

- **L corner**: two wall ends within 1 cm of each other, at least 20° apart, are mitred. Each face stops where it meets the matching face of the other wall
- **T junction**: a wall ending inside another wall (away from its ends) is cut back to that wall's near face. The same holds where a straight run of two walls meets and a third wall ends there
- Each join writes an `IfcRelConnectsPathElements` with `.ATSTART.` / `.ATEND.` / `.ATPATH.`; a joined wall's body is a quadrilateral profile instead of `IfcRectangleProfileDef`
- Walls are not joined when they are curved (`via:`, or a `path:` that is not one straight line, §28), have a `boundary:`, `representation: none`, or are closer than 20° to parallel. At an L both walls need the same `z_offset` and `height`; at a T the through wall must span the full height of the ending wall. A wall whose mitre would run past its other end keeps square ends

### 19.1 Limitations

- Arc, spline and polyline (`path:`) walls are not joined
- Three or more wall ends meeting at one point without a straight run (Y junctions) are not joined
- The SVG floorplan and the web 2D editor draw rectangular wall ends
- A door or window reaching into a joined end: the IFC void keeps its full width, the 3D scene stops it where the end faces begin
- A wall with `height` omitted (storey height) and one with an explicit equal `height` are not joined

## 20. Coverings

`Covering:` writes an `IfcCovering`: a suspended ceiling, floor finish or cladding. Its body is the `boundary` polygon (holes allowed) extruded up from `z_offset` by `thickness` (default 0.02 m); `predefined_type:` fills the IFC enumeration (`ceiling`, `flooring`, `cladding`, ...).

```
#### Space: Seminar 1.04
  boundary: [(0, 0), (8, 0), (8, 6), (0, 6)]
  height: 3.0 m
  Covering:
    predefined_type: ceiling
    material: acoustic_panel
    thickness: 25 mm
```

Defaults for a covering inside a `####` space (`semantic/covering.rs`, resolved before lint, IFC and scene):

- **Outline**: the space boundary (with its holes) minus the plan footprints of the walls and columns of the same storey whose body passes through the covering's height range (`z_offset` to `z_offset + thickness`; a body without `height` counts as full height). Walls are centred on their axis, curves included (§28), `boundary:` prisms taken as given, elements with `representation: none` skipped. A curtain wall whose grid was expanded (`mullions:`, §22) counts as a band as deep as its panels or deepest members, so a ceiling stops clear of transoms and mullions; a grid that failed to expand cuts as a plain wall. Polygon difference via `i_overlay`. A column inside the room becomes a hole; a 1.1 m parapet does not cut a ceiling
- **Ceiling height**: `predefined_type: ceiling` without `z_offset` sits at the top of the space: its `z_offset` plus its `height`, else the storey height (3 m without one), as the IFC writer extrudes the space

A covering outside a space, or in a space without `boundary`, needs them explicitly (E101). The writer adds one `IfcRelCoversSpaces` per space listing its coverings, besides the spatial containment in the `IfcSpace`. `fire_rating`, `is_external`, `thermal_transmittance`, `acoustic_rating` go to `Pset_CoveringCommon`. The decompiler reads `IfcCovering` as `Covering` with `boundary`, `z_offset` and `thickness` from the mesh.

### 20.1 Limitations

- When walls split a space into several pieces, only the largest piece is kept
- Stairs, shafts and slab openings are not subtracted
- Wall footprints are the unjoined rectangles; this only differs at outer mitred corners, outside the room
- The SVG floorplan and the web 2D editor do not draw coverings
- The decompiler does not read `IfcRelCoversSpaces`: a covering contained in the storey and only related to its space stays storey-level, and the relation is lost on roundtrip
- The curtain wall band uses the deepest transom over the full wall height, although transoms sit only at their `transoms:` heights. On an arc curtain wall the band follows the arc while the panels are straight chords, so on tight arcs with wide bays a panel can reach into the ceiling
- The column footprint (circle, rectangle, 0.3 x 0.3 m default) repeats the rules of `column_mesh` and the writer's `section()` instead of sharing one helper

## 21. Footings

`Footing:` writes an `IfcFooting`. Its body is the `boundary` polygon (holes allowed) extruded down by `depth` (required) from `z_offset`, its top. `predefined_type:` fills the IFC enumeration (`strip_footing`, `pad_footing`, `pile_cap`, ...). Without `boundary` the outline comes from one of (`semantic/footing.rs`, resolved before lint, IFC and scene):

- **Strip**: `under:` a wall id or a list of them, with `width:`. Each wall gives a strip of `width` centred on its axis; straight strips run past both ends by half the width, so strips meeting at a corner overlap; curved walls follow their axis (§28). The strips are merged (`i_overlay` union); a closed ring of walls gives an outline with a hole. `z_offset` defaults to the lowest `z_offset` of the walls. The walls must be in the same storey (or building/site level) as the footing
- **Pad**: `position:` (centre) with `width:` along x and `length:` along y (default: `width`), turned by `rotation:`

```
Footing:
  predefined_type: strip_footing
  under: [W-AUD-S, W-AUD-W]
  width: 0.8 m
  depth: 0.5 m
Footing:
  predefined_type: pad_footing
  position: (12, 6)
  width: 1.6 m
  depth: 0.6 m
  z_offset: -0.3 m
```

`under:` uses the list syntax `[a, b]`, which accepts hyphenated element ids. Errors (E101): no `width` for a strip or pad, no `boundary`/`under`/`position`, a wall without axis, walls that do not touch (one footing per connected group); an unknown wall id is E301. The decompiler reads `IfcFooting` as `Footing` with `boundary`, `z_offset` (top) and `depth` from the mesh.

### 21.1 Limitations

- Strips under walls in another storey are not supported
- Strips do not follow wall joins: at a T junction the strips simply overlap, and free wall ends get the half-width overhang too
- Decompiled footings take the outer outline of the mesh: a ring footing comes back filled (as for slabs with holes)
- The SVG floorplan and the web 2D editor do not draw footings
- A strip's top defaults to the lowest base of its walls, which is often the top of the ground slab, so the two overlap; give `z_offset` (e.g. minus the slab thickness)

## 22. Curtain Walls

A `Wall:` with `mullions:` is a curtain wall. `semantic/curtain_wall.rs` (before lint, IFC and scene) gives it `representation: none` and `ifc_class: "IfcCurtainWall"` and writes its parts after it, each with `part_of:` the wall, so the IFC writer aggregates them under the `IfcCurtainWall` (`IfcRelAggregates`):

- **Mullions** `{wall}-M{k}`: vertical `Member`s (`predefined_type: mullion`) on the grid lines along the axis, `mullion_profile: rectangle(face, depth)` (default 50 x 150 mm), `material:` of the wall
- **Transoms** `{wall}-T{row}-{bay}`: horizontal `Member`s between the mullion faces of each bay, `transom_profile` (default: the mullion profile). IFC4 has no transom member type, so they are `mullion` too
- **Panels** `{wall}-P{row}-{bay}`: upright `Plate`s (`predefined_type: curtain_panel`) filling each cell between the member faces, `thickness:` of the wall, `material:` from `panel_material:`

`mullions:` and `transoms:` take a spacing or a list of distances (`[1.5 m, 3 m]`, from the wall start along the axis, or from the wall base up). A spacing divides the length (height) into equal cells no larger than it, with members on every cell edge including both ends. A list puts members only at the listed distances; the cells run from 0 to the wall end. Without `transoms:` there is one row. Curved walls (`via:` or a curved `path:`, §28) place mullions along the curve; transoms and panels span the chords between them.

```
Wall: CW-ENTRY
  from: (0, 0) to: (6, 0)
  height: 4 m
  thickness: 30 mm
  mullions: 1.5 m
  transoms: [2.4 m]
  material: aluminium
  panel_material: glass
  Door: D-ENTRY
    offset: 1.5 m
    width: 1.5 m
    height: 2.4 m
```

Doors and windows of a curtain wall move out of it as parts with a prism `boundary` (in the cell between the member faces on its edges, `depth` default: the wall thickness). Panels are left out of cells a door or window overlaps, transoms whose grid line runs through one; mullions whose grid line runs through one are split around it (`-M{k}-1`, `-M{k}-2`). The wall's `Pset_WallCommon` properties go to `Pset_CurtainWallCommon` (without `LoadBearing`).

An upright plate is a `Plate` with a `from:` axis, `height` and `thickness` instead of `boundary`; it has the body of a wall. Irregular grids are written by hand: a wall with `representation: none` and `ifc_class: "IfcCurtainWall"`, and `Member`s and `Plate`s with `part_of:` it.

Errors: E101 for a grid without axis or height, a list that is not increasing or leaves `0..length`, a profile other than `rectangle`, members wider (taller) than the wall, a door or window outside the wall, an `Opening` in the wall, or a generated id that is already used. A grid with errors stays a plain wall.

### 22.1 Limitations

- Doors and windows should fill whole cells: a panel is dropped for any overlap, leaving a gap beside a door narrower than its cell
- Curved panels and transoms are flat chords
- The decompiler reads `IfcCurtainWall` as a bodiless `Proxy` with its members and plates as parts; it does not rebuild `mullions:`
- The SVG floorplan draws the grid wall as a plain wall without its doors; the web 2D editor does not import it (its block is kept as written) and does not draw it
- In a project, an unknown `material:` or `panel_material:` is reported once per generated part
- Grids are rectangular: no sloped or polygonal curtain walls, no mullion offsets across the wall

## 23. Stairs

A `Stair:` is one straight flight: `riser_count` (default 18) risers of `riser_height` (default storey height / risers) and as many treads of `tread_depth` (default 0.27 m), the top tread flush with the arrival level, `width` across (default 1.2 m), running up along +y from `position` turned by `rotation`. The body is the stepped side profile extruded across the width. With `waist_thickness:` the underside runs parallel to the pitch at that depth (a cast slab); without it the flight is solid down to its base. IFC: `IfcStair` with `PredefinedType` `STRAIGHT_RUN_STAIR` (`SPIRAL_STAIR` for `type: spiral`) unless `predefined_type:` is set; `IfcStairFlight` (`ifc_class:`) gets `NumberOfRisers`, `NumberOfTreads`, `RiserHeight` and `TreadLength`.

An `l_shaped` or `u_shaped` stair, or a `straight` one with `landing_after:`, has a landing. `semantic/stair.rs` (before lint, IFC and scene) gives it `representation: none`, `predefined_type` `quarter_turn_stair`, `half_turn_stair` or `two_straight_run_stair` (unless set) and its `riser_height`, and writes its parts after it, each with `part_of:` the stair and its `material:`:

- **Flights** `{stair}-F1`, `{stair}-F2`: straight `Stair`s with `ifc_class: "IfcStairFlight"`, the first with `landing_after` risers (default: half, rounded up), the second with the rest. Both get `waist_thickness` (default 0.2 m), so the upper flight does not stand on a solid block
- **Landing** `{stair}-L1`: a `Slab` (`predefined_type: landing`) with a `boundary`, top at the first flight's arrival level, `thickness` = the waist thickness

The landing starts after the first flight's last tread. `landing_depth` (default: the width) is its depth along the first flight for `straight` and `u_shaped`; an `l_shaped` landing is square. `turn: left | right` (default left) is the side the second flight turns to; a `u_shaped` stair leaves `gap` (default 0.1 m) between its flights.

```
Stair: ST-1
  type: u_shaped
  position: (4, 2)
  width: 1.2 m
  riser_count: 20
  tread_depth: 0.28 m
  landing_depth: 1.4 m
  turn: right
  waist_thickness: 0.18 m
  material: concrete
```

Other layouts are written as explicit flights and landing slabs with `part_of:` a stair with `representation: none`.

Errors: E101 for a non-positive width, riser height, tread depth or landing depth, fewer than 2 risers, a `landing_after` outside 1 to risers - 1, or a generated id that is already used; a stair with these errors stays one straight flight. An unknown `turn` is E102 (type check); the stair still expands, turning left.

Compliance checks skip elements with `ifc_class: "IfcStairFlight"`, so `applies_to: Stair` sees each stair once.

### 23.1 Limitations

- Only one landing; no winders, no spiral geometry (a `spiral` stair is drawn as a straight flight)
- The decompiler reads `IfcStairFlight` as a `Proxy` prism (its bounding box) with `ifc_class`, not as a flight; an `IfcStair` with flights becomes a `Stair` with `representation: none` and the flights as parts
- No stringers, nosings or handrails on the generated flights
- A flight with n risers has n treads, the top one flush with the arrival level, and `NumberOfTreads` = n

## 24. Building Services (MEP)

Ducts, pipes, cable trays, their terminals and equipment, grouped into distribution systems. Code: `semantic/mep.rs` (routes), `codegen/mep.rs` (sections, bodies, ports, IFC classes), `scene/solids.rs` (meshes of extruded and revolved solids).

| Type | IFC class | Geometry |
|---|---|---|
| `Duct:` | `IfcDuctSegment` / `IfcDuctFitting` | `diameter`, or `width` x `height` (default 0.4 x 0.25 m) |
| `Pipe:` | `IfcPipeSegment` / `IfcPipeFitting` | `diameter` (default 0.05 m) |
| `CableTray:` | `IfcCableCarrierSegment` / `IfcCableCarrierFitting` | open U channel `width` x `height` (default 0.3 x 0.06 m) |
| `Terminal:` | by `type` (below), else `IfcFlowTerminal` | box `width` x `depth` x `height` (or cylinder `diameter`) at `position`, from `z_offset` up, `rotation` |
| `Equipment:` | by `type` (below), else `IfcDistributionFlowElement` | as terminal |
| `System:` | `IfcDistributionSystem` | none |

Points of runs are `(x, y, z)`, storey coordinates with z above the storey (plus `z_offset`). A run element is one of:

- **Segment**: `from: (x, y, z) to: (x, y, z)`; `rotation` (degrees) turns the section about the axis; a vertical segment has local X along world X
- **Bend** (`fitting: bend`): `path: [start, corner, end]`, the arc tangent to both legs at start and end (`IfcRevolvedAreaSolid`)
- **Tee** (`fitting: tee`): main run `from`/`to` plus `branch: (x, y, z)`, the end of a stub perpendicular to the main run; `branch_diameter`/`branch_width`/`branch_height` (default the main section) and `branch_rotation`

### 24.1 Routes

A duct, pipe or cable tray with a `path:` and no `fitting:` is a route. Before lint, IFC and scene the route is replaced by its parts, in order along it:

- Repeated points (< 1 mm apart) and straight-through points are dropped
- Every corner becomes a bend `{route}-B{k}` with centreline radius `bend_radius` (default 1.5 diameters for a round pipe, the diameter for a round duct, else the larger side); it takes `radius * tan(angle / 2)` from each leg
- A route whose end lies inside a leg of another route of the same type (1 mm tolerance, same storey and file) joins it with a tee `{main}-T{k}`: the tee is the branch's half-extent along the main + 0.05 m long on each side, and the branch route starts the main's half-extent along the branch + 0.05 m off the main axis
- A smooth run (§28) becomes one curved segment `{route}-S{k}` along it (round sections only, else E313); where it meets a straight leg tangentially no bend is needed. Tees join straight legs only
- The straight rest of each leg becomes a segment `{route}-S{k}`. A route that ends up as one segment keeps its id (and `guid`/`ifc_name`)
- The section turns with the run through every bend, so a rectangular duct keeps its orientation relative to the bend plane
- Parts copy the route's properties except `path`, `z_offset`, `rotation`, `guid` and `ifc_name`, and carry absolute coordinates

```
Duct: SA-01
  system: SA
  width: 0.4 m
  height: 0.25 m
  path: [(0, 0, 3), (6, 0, 3), (6, 5, 3), (6, 5, 2.4)]
```

Errors (route stays as written, so its parts are missing): E101 for fewer than two distinct points, a 180 degree turn, a leg too short for its bends and tees, a branch that does not leave its main at a right angle, or a generated id that is already used. A property that names a route replaced by its parts (e.g. `part_of: SA-01`) is E101; name a part instead.

A straight `from`/`to` run of the same type whose end lies on the centreline of a route leg (within 1 mm, not at its ends) and that leaves the leg at a right angle becomes a two-point route, so it joins the leg with a tee like a `path:` branch. A run along the leg, or at another angle, stays as written.

### 24.2 Terminal and Equipment Kinds

`type:` sets IFC class, PredefinedType and default size (width x depth x height, m); an unknown `type` is E102.

| Terminal `type` | IFC | Equipment `type` | IFC |
|---|---|---|---|
| `air_supply`, `air_extract` | `IfcAirTerminal` DIFFUSER / GRILLE | `air_handler` | `IfcUnitaryEquipment` AIRHANDLER |
| `sprinkler`, `hose_reel` | `IfcFireSuppressionTerminal` | `fan` | `IfcFan` TUBEAXIAL |
| `wc`, `washbasin`, `sink`, `urinal`, `shower` | `IfcSanitaryTerminal` | `pump` | `IfcPump` CIRCULATOR |
| `light` | `IfcLightFixture` POINTSOURCE | `tank` | `IfcTank` STORAGE |
| `outlet`, `data_outlet` | `IfcOutlet` | `boiler`, `chiller`, `heat_exchanger` | `IfcBoiler`, `IfcChiller`, `IfcHeatExchanger` |
| `floor_drain` | `IfcWasteTerminal` FLOORTRAP | `distribution_board`, `transformer` | `IfcElectricDistributionBoard`, `IfcTransformer` |
| `radiator` | `IfcSpaceHeater` RADIATOR | `damper`, `fire_damper` | `IfcDamper` CONTROLDAMPER / FIREDAMPER |
| `switch`, `smoke_detector`, `alarm` | `IfcSwitchingDevice`, `IfcSensor`, `IfcAlarm` | `valve` | `IfcValve` ISOLATING |

`ifc_class:` and `predefined_type:` override the kind.

### 24.3 Ports and Systems

The IFC writer gives every run element an `IfcDistributionPort` at each end (three on a tee), nested with `IfcRelNests`, and connects two ports of the same type (duct, pipe, cable carrier) within 1 mm with `IfcRelConnectsPorts`, each port at most once. A free run end inside a terminal's or equipment's body box (1 cm tolerance) gets a port on that element and is connected to it. Port `SystemType` is the run's system type.

`system: <id>` puts an element into an `IfcDistributionSystem` named `<id>` (`IfcRelAssignsToGroup`), which serves the buildings of its members (`IfcRelServicesBuildings`). A `System: <id>` element sets its `predefined_type` (`IfcDistributionSystemEnum` in lower case, else E102) and `ifc_name` (LongName); a `system:` that names no `System:` element is E307.

### 24.4 Limitations

- Tees join only routes in the same storey and file; a run across storeys is one storey's element
- No reducers, crosses (two branches at one point), wyes or offsets; a tee branch is perpendicular to the main
- No insulation, flanges or connection details on the generated parts
- The decompiler reads services elements as `Proxy` prisms with `ifc_class`; ports and systems are not read back
- SVG floorplan draws runs as centreline strokes of the section width and terminals/equipment as outlines; the web 2D editor does not show them

## 25. Penetrations

Where a straight duct, pipe or cable tray segment passes through a wall, slab or covering of the same building, the compiler cuts an opening for it. Code: `semantic/penetration.rs`, run after route expansion (§24.1) on a file, or once on the merged model of a project.

- Hosts: walls with `height` (arc and spline walls per chord of their polyline, §28; adjacent chords hit by one run are one crossing, later crossings of the same wall get `-2`, `-3`, ...), slabs with `boundary` and coverings with `boundary` (a ceiling hit by a sprinkler drop), all with a body. Coverings take their default outline (§20) before runs cut them. The crossing point (middle of the centreline inside the host) must lie on the wall axis and within its height, or inside the slab or covering outline and outside its holes (`holes:` and `Opening` children with a `boundary`). The scene cuts `Opening` children out of slabs and coverings, the IFC writer voids them (`IfcRelVoidsElement`)
- The opening is an `Opening` child `{run}-{host}` of the host: a `boundary` rectangle around the run's section where it enters and leaves the host, `clearance` (default 0.025 m, must be positive) larger on every side, reaching 0.05 m past both faces; `z_offset` is relative to the host's storey, so a riser of a lower storey cuts the slab of the upper one
- `sleeve: true` on the run adds `{run}-{host}-SL`, an element of the run's type with `fitting: sleeve`: a tube from half the clearance around the run to the opening (half the clearance thick), through the host from face to face, in the run's storey. It names its opening (`opening:`) and the writer emits `IfcRelFillsElement`; IFC class is the run's segment class with PredefinedType NOTDEFINED, no ports
- `openings: false` on a run turns its openings off
- Generated ids that already exist are skipped, so a hand-written `Opening: {run}-{host}` replaces the generated one, and a sleeve then fills it

```
Pipe: RW-01
  diameter: 0.1 m
  from: (2, 2, 0.5) to: (2, 2, 4.5)
  sleeve: true
  clearance: 0.03 m
```

The decompiler nests only fillers BML allows as children (a wall's doors and windows) under the host; others (a sleeve, a skylight) stay in their own container, or the host's if they have none, with `opening:`.

### 25.1 Limitations

- Bends, tees, curved segments (§28), terminals and equipment cut nothing; roofs, curtain walls, beams and columns are not cut
- A wall's existing openings, doors and windows are not checked; a run through one gets a second opening
- Each crossing gets its own opening; runs side by side through one wall are not merged into a shared opening
- A wall opening is a rectangle in the wall's plane spanning the run's extent; a diagonal run gets a larger opening than needed
- No fire seal or insulation material in the gap
- The scene cuts an opening out of a slab or covering only if the whole opening lies inside the outline; IFC voids it whenever the crossing point is inside, so near an edge the BML preview shows the host uncut

## 26. Hangers and Accessories

`Accessory:` is a discrete accessory: a box (or `diameter` cylinder) at `position:`, up from `z_offset`, sized by `width`, `depth`, `height` and turned by `rotation`, like a terminal (§24). `type:` sets the IFC class and default size. Code: `codegen/mep.rs` (kinds, body), `semantic/penetration.rs` (generation).

| `type` | IFC class | PredefinedType |
|---|---|---|
| `hanger` | `IfcDiscreteAccessory` | USERDEFINED, ObjectType `HANGER` |
| `bracket` | `IfcDiscreteAccessory` | BRACKET |
| `base_plate` | `IfcDiscreteAccessory` | ANCHORPLATE |
| `shoe` | `IfcDiscreteAccessory` | SHOE |
| `anchor_bolt` | `IfcMechanicalFastener` | ANCHORBOLT |

A `hanger` is a trapeze: a 41 mm strut channel across local X with its top at `z_offset`, carrying a run `width` wide, and a 12 mm rod beside each side of the run, from the strut up `height`.

`hangers: <spacing>` on a duct, pipe or cable tray (inherited by the parts of a route) generates hangers along each straight segment that slopes no more than 1:20. The spacing is the largest distance between hangers in the middle of their parts: the segment is cut into `ceil(length / spacing)` equal parts with a hanger `{segment}-H{i}` in the middle of each, added next to the run:

- Where the hanger in the middle would overlap a straight wall or another straight run of the building, it moves along its part in 5 cm steps, at most a quarter of the part, to the nearest position clear of both under the same soffit; with none free it stays in the middle. Neighbouring hangers are then at most 1.5 times the part length apart

- `z_offset` is the run's underside, `width` the run's horizontal extent across, `rotation` across the run
- `height` reaches the soffit of the lowest slab of the same building above the run whose outline holds the hanger and none of its holes, including the openings generated for risers (§25)
- A run none of whose hangers finds a slab gets no hangers and no warning; a storey file analyzed alone usually has its ceiling slab in the next file. In a project they are generated on the merged model
- Otherwise each hanger without a slab above it is left out, with one warning per run (E201)
- The spacing must be positive (E102); generated ids that already exist are skipped, as for openings (§25)

```
Duct: SA-01
  width: 0.4 m
  height: 0.25 m
  path: [(1, 4, 2.8), (9, 4, 2.8)]
  hangers: 2 m
```

### 26.1 Limitations

- Risers and sloped runs (steeper than 1:20) get no supports; bends, tees and curved segments (§28) get no hangers
- Hangers hang only from slabs, not from beams, roofs, walls or coverings; no seismic bracing
- Parallel runs each get their own hangers instead of a shared trapeze
- Hangers move clear of straight walls (by axis length, without the corner pieces of wall joins) and straight runs only; columns, beams, curved walls, fittings and other hangers are not checked
- Only the hanger's centre is tested against the slab outline and holes, so a rod of a wide run can end beside a slab edge
- The decompiler reads accessories back as `Proxy` prisms; the ObjectType `HANGER` is lost

## 27. Connections

`connects:` (any element, one id or a list) writes connection relationships. Code: `codegen/ifc_writer.rs` (`Gen::connections`), `decompile/ifc_reader.rs`, `semantic/penetration.rs` (brackets).

- On an `IfcDiscreteAccessory`, `IfcMechanicalFastener` or `IfcFastener` (by `type:` or `ifc_class:`) naming two or more elements, the element realizes the connection: one `IfcRelConnectsWithRealizingElements` from the first named element to each other one, with the accessory as realizing element. A base plate under a column: `connects: [C-01, FT-01]`
- Otherwise one `IfcRelConnectsElements` from the element to each named one; a pair named from both sides, or twice, is written once
- An id that names no element is E301 (checked after routes, curtain walls and stairs are expanded, so their parts can be named; in a project on the merged model). The writer skips it, and the element itself, with a codegen warning
- The decompiler reads both back: `IfcRelConnectsElements` (exact class, so wall joins in `IfcRelConnectsPathElements` stay out) as `connects:` on the relating element, `IfcRelConnectsWithRealizingElements` as `connects: [relating, related...]` on each realizing element

`brackets: true` on a curtain wall (§22) generates a `bracket` accessory `{mullion}-{slab}` for each vertical mullion part of it (generated, or a hand-written `Member` with `part_of:` and a vertical `from`/`to` axis or `position` + `height`, above its `z_offset`) and each slab of the same building whose z range meets the mullion's and whose outline holds a point next to the mullion:

- The bracket sits against the mullion (half the mullion depth plus half the bracket depth off its axis), on the side of the wall normal the slab is on, turned like the mullion
- On the slab top if a bracket fits above it within the mullion, else under the soffit if one fits below it (the slab at the head of a storey-high wall), else none
- None where it would cut into a straight wall (an end mullion against an abutting wall) or into a bracket already generated (the end mullions of two curtain walls meeting at a corner share the first one's bracket)
- `connects: [<slab>, <mullion>]`; generated on the merged model in a project, like hangers (§26), so a storey file alone only gets brackets to its own slabs
- A `brackets: true` wall that gets no bracket is warned (E201)

```
Wall: CW-01
  from: (0, 0) to: (12, 0)
  height: 4.5 m
  mullions: 1.5 m
  brackets: true
```

Clearance zones need no new element: a `Proxy` with `predefined_type: provisionforspace` and a translucent material (`opacity:`, §16) writes an `IfcBuildingElementProxy` PROVISIONFORSPACE.

### 27.1 Limitations

- `IfcRelConnectsElements` has no `ConnectionGeometry` and no `ConnectionType`; port connections of MEP runs are written separately (§24)
- Brackets only for vertical mullions and slabs; none at beams, roofs or transoms
- The clash test before placing a bracket covers straight walls and other brackets only; curved or `boundary:` walls, columns and beams are not checked
- Bracket size is the `bracket` default; no anchor bolts are generated with it
- A slab edge more than half the bracket depth (0.05 m) behind the mullion's back face gets no bracket; a wider gap needs hand-written brackets
- Asset furniture (one entity per part) and grids cannot be named in `connects:`, and their own `connects:` is ignored

## 28. Plan Curves

Smooth curves wherever BML takes a point list in plan: outlines (`boundary:` of slabs, spaces, flat roofs, coverings, footings, plates and boundary prisms, holes included), `path:` of walls, railings, beams and members, and MEP `path:` routes. Code: `geometry/curve.rs` (curves), `geometry/axis.rs` (`WallAxis`), `semantic/curves.rs` (resolution, errors), `codegen/step_writer.rs` (IFC curves), `decompile/ifc_curves.rs` (reading them back). Fixture: `tests/fixtures/curves.bml`.

### 28.1 Syntax

Any point of a point list may be prefixed `smooth`:

- Plain points are corners
- A run of consecutive `smooth` points becomes one cubic spline through those points (chord-length parametrisation), starting and ending at the plain points on either side. Corners stay sharp
- A closed outline whose points are all `smooth` is one closed spline without a corner (C1 across the seam)

A `nurbs { ... }` item is an exact segment from its first to its last control point:

| Key | Meaning |
|---|---|
| `degree` | Whole number, at least 1 |
| `points` | Control points; at least `degree + 1` |
| `weights` | One positive weight per point; default all 1 |
| `knots` | `points + degree + 1` non-decreasing values, clamped (first and last repeated `degree + 1` times); default clamped uniform |

```
Slab: SL-01
  boundary: [(0, 0), smooth (10, -2), (20, 0), (20, 15), smooth (10, 18), (0, 15)]
  thickness: 0.25 m

Wall: W-01
  path: [(1, 1), smooth (5, 2.5), smooth (9, 2), (13, 4)]
  height: 3 m
  thickness: 0.3 m

Duct: SA-01
  diameter: 0.25 m
  path: [(14, 3, 2.8), smooth (16, 5, 2.8), smooth (17, 8, 2.8), (19, 10, 2.8)]

#### Space: Hall
  boundary: [(14, 1), nurbs { degree: 3, points: [(14, 1), (16, -1), (18, 1), (19, 1)], weights: [1, 2, 1, 1], knots: [0, 0, 0, 0, 1, 1, 1, 1] }, (19, 6), (14, 6)]
```

- Walls, railings, beams and members take `path:` (2D) as an alternative to `from:`/`to:`; a path mixes straight pieces and smooth runs. `from: ... via: ... to:` arcs keep working
- MEP `path:` keeps its meaning (§24.1): plain corners get bends, each smooth run becomes one curved segment `{route}-S{k}` that keeps the run's curve. Only round sections (`diameter`) can follow a smooth run
- Door and window `offset:` is the distance along the wall axis curve; curtain wall `mullions:` spacing is measured along the curve, transoms and panels span flat chords between mullions (§22)
- `curve_tolerance` in `buildingml.toml`: largest chord error in metres when curves become polylines (default 0.005, must be positive)
- `smooth` and `nurbs` are keywords in the grammar, the VS Code TextMate grammar and the web editor highlighter (`bml-language.ts`)

### 28.2 Internals

- `Curve2` / `Curve3` (`geometry/curve.rs`): a list of `Line`, `Arc` and `Nurbs` segments with evaluation, tangent, arc length (Gauss-Legendre), point at distance, sub-curve and tessellation to a chord error
- A curved property keeps its place in `ResolvedElement.properties` as a point list, tessellated to `curve_tolerance`, so containment, footings, penetrations, scene, SVG, lint and checker read the polyline unchanged. The exact curve goes into `ResolvedElement.curves` (and `ResolvedSpace.curves`), keyed by property name (`ElementCurve::Path2`, `Path3`, `Region`)
- `WallAxis` (`geometry/axis.rs`) is the one plan axis of a wall or other linear element: `path:` first, else a `from`/`via`/`to` arc, else a straight `from`/`to`. It measures distances along the exact curve and serves door/window placement, wall outlines, joins (§19), footings `under:` (§21), covering cuts (§20), penetrations (§25), curtain wall expansion (§22), scene, SVG and lint. Openings in a curved wall take the wall outline over their stretch of axis
- Model JSON (`buildingml model`, WASM) carries the polyline, plus `curved`, the list of curved property names. The web and VS Code 2D views draw the polylines; the web editor leaves elements with a `curved` list unimported, so they stay in the source as written

### 28.3 IFC

- A curve of straight lines stays one `IfcPolyline`. Otherwise an `IfcCompositeCurve` of polylines (runs of lines) and `IfcTrimmedCurve`s over `IfcBSplineCurveWithKnots` (`IfcRationalBSplineCurveWithKnots` when a weight is not 1). Arcs inside such a curve are written as exact rational quadratics. Knots are written as distinct values with multiplicities, normalized to 0..1, and trimmed over 0..1 (IfcOpenShell only reads a B-spline segment this way)
- Outlines: `IfcArbitraryClosedProfileDef` / `IfcArbitraryProfileDefWithVoids` over that curve (slabs, plates, footings, coverings, flat roofs, spaces, boundary prisms)
- Walls on a curved `path:`: the outline is a closed curve through offsets of the axis, refitted as splines through face points at least 0.5 m apart (`WallAxis::faces`), sharp at axis corners, extruded up. The `Axis` representation (new `Axis` sub-context) holds the exact curve; arc walls get an exact `Axis` too while their body stays a polyline outline
- Railings, beams and members on a `path:`: a rectangular section is the plan outline of the path at the section width, extruded (railing: up by its height; beam/member: over its depth centred on `z_offset`); a circular section is an `IfcSweptDiskSolid` along the path. The exact path is the `Axis` representation
- Round MEP segments on a smooth run: `IfcSweptDiskSolid` along the 3D B-spline
- `ifc-scene`: ifc-lite cannot mesh B-spline curves, so before meshing `bsplines_to_polycurves` (`bml-cli/src/commands/ifc_scene.rs`, next to `null_dangling_refs`) rewrites each clamped B-spline into an `IfcIndexedPolyCurve` of points sampled to 5 mm, keeping its id. A trimmed curve over a B-spline becomes the polycurve of its trimmed range; a B-spline with a trim that cannot be read is kept

### 28.4 Decompiler

`decompile/ifc_curves.rs` reads products whose `Axis`, profile or swept-disk directrix holds a B-spline:

- Extruded `IfcArbitraryClosedProfileDef` / `...WithVoids` outlines come back as `boundary:` with the exact curve
- A wall, railing, beam or member with a B-spline `Axis` comes back as `path:`; wall and railing `thickness` (beam/member rectangle width) is profile area / axis length, rounded to mm
- A beam or member with a swept disk on a plan B-spline comes back as `path:` with `profile: circle(r)`
- Polylines, B-splines, trimmed curves and composite curves of them are read; an unclamped B-spline is sampled into lines
- The BML writer writes lines as corners and the rest as `nurbs` items, leaving out weights when all 1 and knots when clamped uniform; weights and knots keep 6 decimals
- A space keeps the exact outline the reader gave it instead of taking it from its mesh (§15.2)

### 28.5 Errors

| Code | Case |
|---|---|
| E308 | Curve with fewer than 2 distinct points (an outline without a `nurbs` item: 3), or a smooth run through a repeated point |
| E309 | Bad `nurbs`: degree not a whole number or below 1, fewer than `degree + 1` points, wrong knot count, decreasing or unclamped knots, weight missing or not positive |
| E310 | A curved outline, hole or 2D path crosses itself |
| E311 | Wall bends to a radius below half its thickness (faces would fold); the message gives the radius and the minimum |
| E312 | Wall, railing, beam or member with both `path:` and `from:`/`to:` |
| E313 | Rectangular duct or cable tray on a smooth run |

### 28.6 Limitations

Language and analysis:

- `smooth` and `nurbs` lex as keywords after `[`: in `tags: [smooth, rough]` the `smooth` item is dropped silently
- Outlines of plain corners that cross themselves (bowtie) pass; only curved ones get E310
- `curve_tolerance` from `buildingml.toml` is read for projects only; single-file lint, compile, LSP and WASM use the 0.005 m default. In a project it sets the polylines of curved properties, MEP curved segments and the E311 check; arc walls in the IFC writer, scene, joins, coverings, penetrations and SVG still use 0.005 m
- NURBS tessellation tests the chord error at the quarter points of each piece, so a polyline can exceed the tolerance by about 5%
- Curve evaluation allocates per call, `point_at` is O(n^2) and the self-crossing check O(n^2): slow on very long decompiled splines
- The web 2D editor shows curves as polylines; curve points cannot be edited there. Curved slabs and `path:` walls are not imported into it (kept as written), so `path:` walls are missing from the web and VS Code 2D plans
- `structural_span`, the beam span lint rule, space boundary inference from walls and E305 read `from`/`to` only and ignore `path:`
- A `boundary:` asset `extrude()` drops `smooth` and `nurbs` items

Walls and linear elements:

- Curved wall faces are splines through offset points, an approximation of the exact offset curve
- Curved walls and `path:` walls with corners are not mitre-joined (§19)
- An axis that doubles back on itself gives a degenerate 180 degree mitre; an opening spanning two chords is cut from the first chord only
- A wall with both `path:` and `boundary:` places the boundary prism at the storey origin
- Beam `rotation` is ignored on a `path:`; linear `path:` is 2D (no varying z) for walls, railings, beams and members
- Curved railings, beams and members are extruded plan outlines (rectangle) or swept disks (circle), not profile sweeps (IfcOpenShell ignores `IfcFixedReferenceSweptAreaSolid`)

MEP:

- Rectangular ducts and cable trays cannot follow smooth runs (E313)
- Curved MEP segments get no hangers (§26) and cut no openings (§25)
- A branch ending on a curved leg of another route is left unjoined without a diagnostic

`ifc-scene` B-spline rewrite:

- Cartesian or wrap-around trims take the full parameter range
- Unclamped B-splines are sampled over their valid knot range; one under a trim is left as is (ifc-lite does not mesh it)
- IFC2x3 rational B-splines written as complex entities are not detected
- An `IfcSweptDiskSolid` `StartParam`/`EndParam` over a B-spline directrix would be mis-trimmed (the BML writer writes `$`)
- B-splines on advanced BREP edges are rewritten too (model_3: 8 elements change by up to 1.7 mm)

Decompiler:

- MEP classes decompile to `Proxy` (§24.4), so curved runs come back as mesh prisms instead of `path:`
- Composite curves that mix `IfcCircle` trims, `IfcIndexedPolyCurve` or `IfcLine` with B-splines fall back to mesh fitting
- Curved wall and railing thickness is profile area / axis length; an axis offset from the wall centre shifts the `path:` (mesh fitting catches it)
- An element whose body is a B-spline prism reads back as a `boundary:` prism (or a `path:`); type-specific values such as door or window width, height and offset are not read
- Arcs written by BML come back as rational quadratic `nurbs` items, not `via:`

## Appendix A: Dependency Graph

```
Phase 8a (Compiler Quality)
  └── No dependencies — can start immediately

Phase 8b (Refactoring)
  └── No dependencies — can start immediately

Phase 8c (LLM Evaluation)
  └── No dependencies — can start immediately

Phase 8d (Spec-to-Check)
  ├── Phase 8a: Uses improved error messages for check validation
  └── Phase 8c: Uses evaluation results to inform prompt design

Phase 8e (Regulation Library)
  └── Phase 8d: Uses spec-to-check pipeline for initial generation

Phase 8f (LLM Improvements)
  └── Phase 8c: Uses evaluation findings to identify failure modes

Phase 8g (Editor Polish)
  └── No dependencies — can start immediately

Phase 8h (Project Scaffolding CLI)
  └── No dependencies — can start immediately

Phase 8i (Asset Library Expansion)
  └── No dependencies — can start immediately (builds on v0.2 Phase 7d asset system)
```

## Appendix B: Related Documents

- `Specs.md` — BuildingML v0.1 language specification
- `Specs_V0_2.md` — V0.2 specification (completed phases)
- `CLAUDE.md` — Developer guide
