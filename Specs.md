# BuildingML — Building Markdown Language

**Initial Specification Draft v0.1**
**Date: 2025-02-27**
**Author: Stefan (+ Claude brainstorming)**
**Status: DRAFT — Open for discussion**

---

## 1. Vision and Goals

BuildingML is a human-readable, LLM-friendly domain-specific language (DSL) for describing Building Information Models. It is designed to be authored by both humans and AI coding agents (e.g., Claude Code), compiled to IFC, and enriched with compliance checking capabilities.

### 1.1 Core Principles

1. **LLM-native**: The syntax must be trivially generatable by LLMs. Markdown-like structure leverages LLM training data distribution.
2. **IFC-aligned but not IFC-bound**: Semantics map cleanly to IFC4, but the surface syntax is dramatically simpler. The compiler handles the mapping to IFC property sets, relationships, and geometry representations.
3. **Compliance-first**: The language and toolchain natively support defining and running compliance checks (building codes, regulations, custom rules) as first-class citizens.
4. **Round-trippable (with defined loss)**: IFC -> BuildingML -> IFC must be supported, with clearly specified acceptable information loss.
5. **Hierarchical and modular**: Projects are split across files (per storey, per system, per discipline), composed via imports.
6. **Rust-powered**: Parser, compiler, and Language Server are implemented in Rust for performance, correctness, and WASM portability.

### 1.2 Non-Goals (for v0.1)

- Full IFC schema coverage (we target a practical subset)
- BREP or NURBS geometry (only parametric/extruded)
- MEP systems (future extension)
- Direct 3D editing (the 3D viewer is read-only preview)

---

## 2. Language Design

### 2.1 File Extension

`.bml` — Building Markup Language files

### 2.2 File Structure

A BuildingML project consists of:

```
my-building/
  project.bml          # Project metadata, site, building-level info
  ground-floor.bml     # Storey definition
  first-floor.bml      # Storey definition
  roof.bml             # Roof definition
  materials.bml        # Shared material definitions
  checks/
    fire-safety.bml    # Compliance check definitions
    accessibility.bml  # Compliance check definitions
  buildingml.toml          # Project configuration (compiler settings, IFC version, etc.)
```

### 2.3 Syntax Overview

BuildingML uses a markdown-inspired syntax with heading levels for spatial hierarchy, indentation for element nesting, and key-value pairs for properties.

#### 2.3.1 Spatial Hierarchy via Headings

```markdown
# Site: Residential Complex Alpha
  location: 49.4521, 12.1803
  address: "Marktplatz 1, 92507 Nabburg, Germany"

## Building: House A
  type: residential
  year: 2025

### Storey: Ground Floor
  elevation: 0.0 m

### Storey: First Floor
  elevation: 3.2 m
```

Heading levels map directly to the IFC spatial hierarchy:

| Heading | IFC Entity          | Scope        |
|---------|---------------------|--------------|
| `#`     | IfcSite             | Project-level |
| `##`    | IfcBuilding         | Site-level    |
| `###`   | IfcBuildingStorey   | Building-level |
| `####`  | IfcSpace            | Storey-level  |

#### 2.3.2 Building Elements

Elements are declared within their spatial context using a `Type: ID` pattern, indented under their parent.

```markdown
### Storey: Ground Floor
  elevation: 0.0 m

#### Space: Living Room
  usage: living
  area: 35.0 m2

  Wall: W-GF-01
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.24 m
    material: exterior_wall_layered

    Window: WIN-01
      offset: 1.5 m
      width: 1.2 m
      height: 1.4 m
      sill_height: 0.9 m
      type: double_glazed

  Wall: W-GF-02
    from: (6, 0) to: (6, 5)
    height: 2.8 m
    thickness: 0.24 m
    material: exterior_wall_layered

    Door: D-01
      offset: 1.0 m
      width: 0.9 m
      height: 2.1 m
      type: single_leaf

  Slab: SLAB-GF-01
    boundary: [(0,0), (6,0), (6,5), (0,5)]
    thickness: 0.22 m
    material: reinforced_concrete

  Column: COL-01
    position: (3.0, 2.5)
    profile: rectangle(0.3, 0.3)
    height: 2.8 m
    material: reinforced_concrete

  Beam: B-01
    from: (0, 2.5) to: (6, 2.5)
    profile: rectangle(0.3, 0.5)
    material: reinforced_concrete
```

#### 2.3.3 Properties

Properties are written directly as key-value pairs on elements. The compiler maps them to the correct IFC Pset.

```markdown
  Wall: W-GF-01
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.24 m
    material: exterior_wall_layered

    # These get mapped to Pset_WallCommon by the compiler
    is_external: true
    fire_rating: REI90
    thermal_transmittance: 0.28 W/m2K
    load_bearing: true
    acoustic_rating: 52 dB
```

The compiler maintains a mapping table: `(ElementType, PropertyName) -> (PsetName, PropertyName, PropertyType)`. This keeps the .bml files clean while producing correct IFC output.

#### 2.3.4 Materials

Materials can be defined inline or in a shared materials file.

```markdown
# materials.bml

Material: exterior_wall_layered
  type: layered
  layers:
    - plaster_interior: 0.015 m
    - concrete_block: 0.175 m
    - insulation_eps: 0.06 m
    - plaster_exterior: 0.01 m

Material: reinforced_concrete
  type: homogeneous
  category: concrete
  grade: C30/37
  density: 2400 kg/m3

Material: double_glazed
  type: homogeneous
  category: glass
  u_value: 1.1 W/m2K
```

#### 2.3.5 Imports and References

```markdown
# ground-floor.bml

import materials from "./materials.bml"
import checks from "./checks/fire-safety.bml"

### Storey: Ground Floor
  elevation: 0.0 m
  ...
```

Cross-file references use the element ID:

```markdown
  Door: D-01
    host: W-GF-02    # References wall by ID
    offset: 1.0 m
    ...
```

#### 2.3.6 Geometry Representations

BuildingML supports simplified parametric geometry. The compiler translates these to appropriate IFC representations (IfcExtrudedAreaSolid, IfcMappedItem, etc.).

**Walls** — defined by axis line + height + thickness:
```
Wall: W-01
  from: (0, 0) to: (6, 0)
  height: 2.8 m
  thickness: 0.24 m
```

**Slabs** — defined by boundary polygon + thickness:
```
Slab: S-01
  boundary: [(0,0), (6,0), (6,5), (0,5)]
  thickness: 0.22 m
```

**Columns** — defined by position + profile + height:
```
Column: C-01
  position: (3.0, 2.5)
  profile: rectangle(0.3, 0.3)  # or circle(0.15)
  height: 2.8 m
```

**Beams** — defined by axis + profile:
```
Beam: B-01
  from: (0, 2.5) to: (6, 2.5)
  profile: rectangle(0.3, 0.5)  # or i_profile(0.3, 0.15, 0.01, 0.015)
```

**Roofs** — defined by boundary + slope or explicit points:
```
Roof: R-01
  type: gable
  boundary: [(0,0), (10,0), (10,8), (0,8)]
  ridge_height: 3.5 m
  ridge_direction: x
  overhang: 0.5 m
```

**Openings (Doors, Windows)** — positioned relative to host element:
```
Window: WIN-01
  offset: 1.5 m        # Distance along wall axis from start point
  width: 1.2 m
  height: 1.4 m
  sill_height: 0.9 m
```

**Stairs** (simplified):
```
Stair: ST-01
  from_storey: Ground Floor
  to_storey: First Floor
  type: straight        # straight | l_shaped | u_shaped | spiral
  width: 1.2 m
  riser_count: 18
  riser_height: 0.178 m
  tread_depth: 0.27 m
  position: (4.0, 3.0)
  direction: (0, 1)
```

---

## 3. Supported Element Types (v0.1 Scope)

### 3.1 Spatial Structure
- `Site` (IfcSite)
- `Building` (IfcBuilding)
- `Storey` (IfcBuildingStorey)
- `Space` (IfcSpace)

### 3.2 Structural / Architectural Elements
- `Wall` (IfcWall / IfcWallStandardCase)
- `Slab` (IfcSlab) — floor, roof slab, landing
- `Column` (IfcColumn)
- `Beam` (IfcBeam)
- `Roof` (IfcRoof)
- `Stair` (IfcStair)
- `Ramp` (IfcRamp)
- `Railing` (IfcRailing)

### 3.3 Openings
- `Door` (IfcDoor)
- `Window` (IfcWindow)
- `Opening` (IfcOpeningElement) — generic, auto-generated for doors/windows

### 3.4 Furnishing (basic)
- `Furniture` (IfcFurnishingElement) — with predefined types

### 3.5 Annotations
- `Grid` (IfcGrid) — structural grid axes
- `Dimension` — for documentation (non-IFC, used in visualization)

---

## 4. Compiler Architecture

### 4.1 Pipeline

```
                           +------------------+
                           |  buildingml.toml    |
                           +--------+---------+
                                    |
  .bml files ──> [ Parser ] ──> [ AST ] ──> [ Semantic Analysis ] ──> [ IR ]
                                                                        |
                                              +-------------------------+----------+
                                              |                         |          |
                                        [ IFC Gen ]            [ Visualizer ] [ Checker ]
                                              |                         |          |
                                           .ifc file              3D Preview   Check Report
```

### 4.2 Stages

**Stage 1: Parsing**
- Tokenizer + PEG/tree-sitter grammar
- Produces untyped AST
- Handles multi-file resolution (imports)
- Error recovery for LSP (partial parse on invalid input)

**Stage 2: Semantic Analysis**
- Type checking (element types, property types, units)
- Reference resolution (material refs, element refs, storey refs)
- Spatial containment validation
- ID uniqueness checks
- Unit normalization (all internal representation in SI)

**Stage 3: Intermediate Representation (IR)**
- Fully resolved, typed model
- All references linked
- Geometry computed (coordinates resolved, profiles expanded)
- Properties mapped to IFC Psets

**Stage 4a: IFC Generation**
- IR -> IFC4 STEP file (ISO 10303-21)
- Uses an IFC writer (custom Rust, or binding to IfcOpenShell)
- Generates: spatial structure, geometry (IfcExtrudedAreaSolid), materials, property sets, relationships

**Stage 4b: Visualization**
- IR -> 3D scene graph (for web viewer via WASM)
- Or: Generate IFC, load in xeokit/IFC.js

**Stage 4c: Compliance Checking**
- IR -> Check engine evaluates rules
- Produces structured report (pass/fail/warning per rule per element)

### 4.3 Round-Trip: IFC -> BuildingML (Decompiler)

```
.ifc file ──> [ IFC Parser ] ──> [ IFC Model ] ──> [ Simplifier ] ──> [ BML Generator ] ──> .bml files
```

**Acceptable information loss during IFC -> BuildingML:**
- Complex BREP geometry is replaced with bounding-box or simplified parametric approximation, with a `# GEOMETRY_SIMPLIFIED` comment
- Custom property sets not in the compiler's mapping table are preserved as raw key-value pairs with a `# UNMAPPED_PSET: PsetName` annotation
- Presentation styles (colors, line styles) are dropped (or mapped to a simplified `color:` property)
- IfcOwnerHistory details are simplified
- IfcRelAssociatesClassification and other association relationships may be dropped
- Proxy elements (IfcBuildingElementProxy) are preserved as generic `Proxy: ID` elements
- Advanced structural analysis model entities are dropped entirely

**Preserved faithfully:**
- Spatial hierarchy
- Standard building elements and their parametric geometry
- Material assignments and layer sets
- Properties that are in the compiler mapping table
- Element IDs (GlobalId preserved as comment or metadata)
- Relationships: containment, aggregation, openings

---

## 5. Compliance Checking System

### 5.1 Design Philosophy

Compliance checks are defined in `.bml` files using a test-like syntax, inspired by unit testing frameworks. They are first-class citizens, not an afterthought.

### 5.2 Check Definition Syntax

```markdown
# checks/fire-safety.bml

CheckSuite: Fire Safety — German Building Code (MBO)

Check: minimum_door_width
  description: "Doors in escape routes must have minimum clear width of 0.90 m"
  reference: "MBO §36 (5)"
  severity: error
  applies_to: Door
  where:
    host.space.usage in [corridor, staircase, escape_route]
  assert:
    width >= 0.90 m

Check: maximum_escape_route_length
  description: "Maximum length of escape route to nearest exit"
  reference: "MBO §35 (2)"
  severity: error
  applies_to: Space
  assert:
    escape_route_length <= 35.0 m  # requires pathfinding computation

Check: fire_rated_walls
  description: "Walls between fire compartments must have fire rating"
  reference: "MBO §29"
  severity: error
  applies_to: Wall
  where:
    is_fire_compartment_boundary == true
  assert:
    fire_rating in [REI30, REI60, REI90, REI120]

Check: minimum_room_height
  description: "Habitable rooms must have minimum clear height of 2.40 m"
  reference: "MBO §33 (1)"
  severity: error
  applies_to: Space
  where:
    usage in [living, bedroom, kitchen, office, classroom]
  assert:
    clear_height >= 2.40 m
```

### 5.3 Check Execution

```bash
# Run all checks
buildingml check my-building/

# Run specific check suite
buildingml check my-building/ --suite fire-safety

# Output formats
buildingml check my-building/ --format json
buildingml check my-building/ --format html
buildingml check my-building/ --format sarif   # For IDE integration
```

### 5.4 Check Report Structure

```json
{
  "project": "my-building",
  "timestamp": "2025-02-27T14:30:00Z",
  "summary": { "passed": 42, "failed": 3, "warnings": 7, "skipped": 2 },
  "results": [
    {
      "check": "minimum_door_width",
      "suite": "fire-safety",
      "status": "FAIL",
      "severity": "error",
      "element": "D-01",
      "element_type": "Door",
      "file": "ground-floor.bml",
      "line": 28,
      "message": "Door D-01 has width 0.80 m, minimum required is 0.90 m",
      "reference": "MBO §36 (5)",
      "actual_value": "0.80 m",
      "expected": ">= 0.90 m"
    }
  ]
}
```

### 5.5 LSP Integration

Check results appear as VS Code diagnostics (squiggly underlines):
- `error` severity -> red underline
- `warning` severity -> yellow underline
- `info` severity -> blue underline

Hovering shows the check description, regulatory reference, and actual vs. expected values.

### 5.6 Computed Properties for Checks

Some checks require derived properties that the checker engine computes:

- `clear_height`: storey height minus slab thickness
- `escape_route_length`: shortest path from space to exit (graph-based)
- `window_to_floor_ratio`: total window area / floor area per space
- `is_fire_compartment_boundary`: derived from adjacent space fire compartment assignments
- `structural_span`: distance between supports for slabs/beams

These are defined in the compiler's built-in function library and can be extended.

---

## 6. Language Server Protocol (LSP)

### 6.1 Features

| Feature                  | Description                                                   |
|--------------------------|---------------------------------------------------------------|
| **Diagnostics**          | Parse errors, type errors, unresolved references, compliance failures |
| **Completion**           | Element types, property names, material refs, units, enum values |
| **Hover**                | Property documentation, IFC mapping info, unit conversion      |
| **Go to Definition**     | Jump to material definition, referenced element, imported file |
| **Find References**      | Find all elements using a material, all refs to an element     |
| **Rename**               | Rename element IDs across files                                |
| **Code Actions**         | "Add missing required properties", "Fix unit", "Generate ID"  |
| **Document Symbols**     | Outline view showing spatial hierarchy                         |
| **Folding**              | Fold elements, spaces, storeys                                 |
| **Formatting**           | Auto-format indentation, alignment                             |
| **Semantic Tokens**      | Syntax highlighting for element types, IDs, units, values      |
| **Live Preview**         | Side panel with 3D visualization (via WASM viewer)             |

### 6.2 Smart Completions for LLM Workflows

The LSP should be designed so that Claude Code (or similar) can:

1. Query available element types and their required/optional properties
2. Get suggestions for valid property values (enums, ranges)
3. See compliance check results in real-time as the model is authored
4. Auto-resolve spatial containment from context

---

## 7. CLI Tool

```bash
# Compile to IFC
buildingml compile my-building/ -o output.ifc

# Compile specific storey
buildingml compile my-building/ground-floor.bml -o ground-floor.ifc

# Decompile IFC to BuildingML
buildingml decompile input.ifc -o my-building/

# Run compliance checks
buildingml check my-building/ --suite fire-safety

# Validate syntax only
buildingml lint my-building/

# Start language server
buildingml lsp

# Start preview server (web-based 3D viewer)
buildingml preview my-building/ --port 8080

# Format files
buildingml fmt my-building/

# Show IFC mapping for a property
buildingml explain property thermal_transmittance --element Wall

# Initialize new project
buildingml init my-building
```

---

## 8. Configuration (buildingml.toml)

```toml
[project]
name = "Sample Residential Building"
author = "Stefan"
ifc_version = "IFC4"       # IFC4 | IFC4x3
schema_version = "0.1"

[compiler]
output_format = "step"      # step | ifcjson | ifcxml
coordinate_system = "right_hand"
default_unit = "metric"     # metric | imperial
precision = 4               # decimal places for coordinates

[checks]
suites = ["checks/fire-safety.bml", "checks/accessibility.bml"]
fail_on_error = true        # Exit with error code if any check fails
report_format = "json"      # json | html | sarif

[decompiler]
geometry_simplification = "parametric"  # parametric | bounding_box | skip
preserve_guids = true
split_by_storey = true
unmapped_pset_handling = "preserve"     # preserve | drop | comment

[preview]
port = 8080
auto_reload = true
```

---

## 9. Implementation Plan

### Phase 1: Foundation (Months 1-2)
- [ ] Define formal grammar (tree-sitter or PEG)
- [ ] Implement Rust parser producing AST
- [ ] Basic semantic analysis (type checking, reference resolution)
- [ ] Single-file compilation to IFC (walls, slabs only)
- [ ] CLI: `buildingml compile`, `buildingml lint`

### Phase 2: Round-Trip (Months 2-3)
- [ ] IFC decompiler (using IfcOpenShell or ifc-rs)
- [ ] Define and document acceptable information loss
- [ ] Multi-file support with imports
- [ ] Material definitions
- [ ] All v0.1 element types

### Phase 3: Language Server (Months 3-4)
- [ ] LSP server in Rust (tower-lsp crate)
- [ ] VS Code extension (syntax highlighting, diagnostics, completion)
- [ ] Document symbols, hover, go-to-definition
- [ ] Real-time compliance check diagnostics

### Phase 4: Compliance System (Months 4-5)
- [ ] Check definition parser
- [ ] Check evaluation engine
- [ ] Report generation (JSON, HTML, SARIF)
- [ ] Built-in computed property functions
- [ ] Example suites: German MBO, accessibility

### Phase 5: Visualization (Month 5-6)
- [ ] WASM build of viewer component
- [ ] VS Code preview panel
- [ ] Web-based standalone viewer
- [ ] CLI `buildingml preview`

### Phase 6: Polish and Research (Months 6-8)
- [ ] LLM evaluation: accuracy of Claude Code generating valid BML
- [ ] Token efficiency benchmarks (BML vs IFC vs ifcJSON)
- [ ] User study with architects/engineers
- [ ] Paper submission

---

## 10. Design Decisions (Resolved)

### 10.1 Syntax Decisions

- **RESOLVED: Comment syntax** — `//` for line comments, `///` for doc comments (attach to next element). `#` is exclusively for headings.
- **RESOLVED: Multi-line values** — Indentation continuation. Values continue on the next line if indented further than the key. Example:

  ```buildingml
  boundary:
    (0,0), (6,0), (6,4),
    (3,4), (3,6), (0,6), (0,0)
  ```
- **RESOLVED: Inline vs. block geometry** — Block only. All element properties use key-value pairs on separate lines. One canonical way to write geometry. Simpler grammar, simpler for LLMs.
- **RESOLVED: Units** — Default from `buildingml.toml` config (`default_unit = "metric"`) with explicit per-value override allowed. Mixed units permitted. The LSP/VS Code extension shows inline hints for resolved units when omitted.
- **RESOLVED: Enumeration values** — Freeform `snake_case` identifiers (e.g., `type: double_glazed`). Compiler validates against known enum values per element/property combination. LSP provides autocomplete.

### 10.2 Semantic Decisions

- **RESOLVED: Coordinate system** — Local per storey. Each storey has its own 2D origin at (0,0). The `elevation` property places it vertically. Buildings have site-relative `position` for multi-building sites. Compiler transforms to IFC's global placement hierarchy.
- **RESOLVED: Wall connectivity** — Implicit from endpoint proximity. Walls sharing endpoints (within tolerance) are auto-connected. Compiler detects L/T/X junctions and generates `IfcRelConnectsPathElements`. Future extension: hybrid with explicit override.
- **RESOLVED: Space boundaries** — Auto-derived from enclosing walls. Optional explicit `boundary:` polygon. Compiler infers 2nd-level space boundaries from wall geometry.
- **RESOLVED: Element relationships beyond containment** — Explicit property references using `supports:` / `supported_by:` (one direction suffices). Compiler infers the inverse and maps to correct IFC relationship entities. LSP displays inverse relationships via VS Code hints.
- **RESOLVED: Versioning** — Semantic versioning in `buildingml.toml`: `buildingml_version = "0.1"`. Compiler checks compatibility. Breaking changes bump major, grammar additions bump minor.

### 10.3 Compiler Decisions

- **RESOLVED: IFC writer strategy** — Use `ifc_rs` crate where it covers BuildingML's needs, extend with custom Rust code where it falls short. Contribute back to the crate. Avoids reinventing STEP serialization while maintaining control.
- **RESOLVED: Geometry kernel** — No geometry kernel. Parametric geometry + basic 2D line math for wall junction trimming + `IfcOpeningElement` for door/window openings. Future BREP/OBJ support is data transcription (not geometric computation). Compliance checker uses property arithmetic and graph traversal, not geometric operations.
- **RESOLVED: Round-trip fidelity testing** — Semantic comparison on IR. Parse both original and round-tripped IFC files to the intermediate representation, then diff semantically. Ignores GUIDs, ordering, formatting, and out-of-subset properties. Checks same spatial hierarchy, elements, geometry, relationships, and property values.
- **RESOLVED: Error recovery in parser** — Handled natively by tree-sitter's built-in GLR error recovery. Produces partial parse trees with ERROR nodes for the LSP.

### 10.4 Compliance System Decisions

- **RESOLVED: Check language expressiveness** — Basic `applies_to` / `where` / `assert` pattern extended with quantifiers and aggregations: `count()`, `sum()`, `min()`, `max()`, `forall`, `exists`. Extensible in the future via Rust/Python functions.
- **RESOLVED: Computed properties** — Rust built-ins for v0.1 (`clear_height`, `escape_route_length`, `window_to_floor_ratio`, `is_fire_compartment_boundary`, `structural_span`). Extensible via a `ComputedProperty` trait for future Rust plugins and Python FFI.
- **RESOLVED: Check composability** — Yes, with `depends_on`. Checks can declare dependencies on other checks. Dependent checks are skipped if prerequisites fail. Avoids cascading errors.
- **RESOLVED: Regulatory code modeling** — Structured regulations from the start. Regulations are first-class entities with jurisdiction, version, sections, and paragraphs. Checks reference them structurally. Connects to DBP research direction.
- **RESOLVED: Parametric checks** — Yes. Checks can define parameters with defaults. Check suites instantiate them with jurisdiction-specific values. Enables reuse across German MBO, Austrian OIB, Swiss SIA, etc. Example:

  ```buildingml
  Check: minimum_door_width(min_width: 0.90 m)
    applies_to: Door
    where: host.space.usage in [corridor, staircase]
    assert: width >= min_width
  ```

### 10.5 Tooling Decisions

- **RESOLVED: Parser technology** — tree-sitter. Provides built-in error recovery for LSP, incremental parsing, and free syntax highlighting for VS Code, Neovim, Helix, Zed, and GitHub. Grammar defined in `grammar.js`, CST converted to typed AST in a second pass. Indentation handled via external scanner (like tree-sitter-python).
- **RESOLVED: WASM strategy** — Full compiler to WASM from Phase 1. Compile the entire pipeline (parser, semantic analysis, checker, IFC writer) to WASM early to validate browser compatibility continuously. All dependencies must be WASM-compatible.
- **RESOLVED: Claude Code integration** — LSP only, no separate MCP server. Claude Code uses the CLI directly (`buildingml compile`, `buildingml check`, etc.).
- **RESOLVED: Package manager** — Deferred to post-v0.1. Check suites are local `.bml` files or Git repos for now. Format designed to be packageable later.

### 10.6 Research Decisions

- **RESOLVED: Evaluation metrics** — Multi-metric approach: (1) token efficiency (BuildingML vs ifcJSON vs IFC), (2) first-attempt compilation success rate, (3) semantic accuracy of LLM-generated models, (4) edit distance to correct LLM output.
- **RESOLVED: Baseline comparisons** — Compare against ifcJSON and IfcOpenShell Python scripting as the two most practical alternatives.
- **RESOLVED: User study design** — Deferred until tool is functional enough to test.
- **RESOLVED: Publication venue** — Decide later based on results.

### 10.7 Editor Features

- **RESOLVED: 2D Editor** — Core feature from the start. Visual floor plan editor alongside text-based authoring, implemented as a VS Code panel or web component.
- **RESOLVED: Snapping mode** — Implicit from wall connectivity model. Elements snap to nearby endpoints/edges within configurable tolerance. Settings in `buildingml.toml`: `snap_tolerance`, `grid_size`, `snap_to_grid`, `snap_to_elements`.

---

## 11. Inspirations and References

- **BAML** (Boundary AI Markup Language): DSL for structured LLM outputs, Rust compiler, VS Code playground, type-safe code generation. Key inspiration for the compiler + LSP + developer experience approach.
- **ifcJSON**: JSON serialization of IFC. Useful reference for IFC-to-simpler-format mapping, but too verbose and not human-friendly enough.
- **IFC4**: The target schema. BuildingML maps to IFC4 (and later IFC4x3).
- **ifcopenshell**: Python library for IFC manipulation. Potential backend for IFC generation, or reference for understanding IFC semantics.
- **ifc-rs**: Rust IFC library, potential dependency.
- **tree-sitter**: Parser generator ideal for LSP integration with error recovery.
- **tower-lsp**: Rust crate for implementing Language Servers.
- **xeokit-sdk / IFC.js (web-ifc)**: Web-based IFC viewers for the preview component.
- **SARIF**: Static Analysis Results Interchange Format, for IDE-compatible compliance reports.

---

## 12. Example: Complete Small Building

```markdown
# Site: Example Residence
  location: 49.4521, 12.1803
  address: "Gartenweg 5, 92507 Nabburg"

import materials from "./materials.bml"

## Building: Single Family House
  type: residential
  gross_floor_area: 120.0 m2

### Storey: Ground Floor
  elevation: 0.0 m
  height: 2.80 m

#### Space: Entrance Hall
  usage: circulation
  area: 8.0 m2

  Wall: W-GF-01
    from: (0, 0) to: (4, 0)
    height: 2.80 m
    thickness: 0.365 m
    material: exterior_wall_layered
    is_external: true
    load_bearing: true
    fire_rating: REI90

    Door: D-MAIN
      offset: 1.0 m
      width: 1.10 m
      height: 2.10 m
      type: single_leaf
      is_external: true
      fire_rating: T30

  Wall: W-GF-02
    from: (4, 0) to: (4, 3)
    height: 2.80 m
    thickness: 0.115 m
    material: interior_partition
    is_external: false
    load_bearing: false

    Door: D-01
      offset: 0.5 m
      width: 0.90 m
      height: 2.10 m
      type: single_leaf

#### Space: Living Room
  usage: living
  area: 28.0 m2

  Wall: W-GF-03
    from: (4, 0) to: (10, 0)
    height: 2.80 m
    thickness: 0.365 m
    material: exterior_wall_layered
    is_external: true
    load_bearing: true

    Window: WIN-01
      offset: 1.0 m
      width: 2.0 m
      height: 1.6 m
      sill_height: 0.80 m
      type: triple_glazed
      u_value: 0.7 W/m2K

    Window: WIN-02
      offset: 4.0 m
      width: 1.2 m
      height: 1.4 m
      sill_height: 0.90 m
      type: triple_glazed

  // Floor slab for entire ground floor
  Slab: SLAB-GF
    boundary: [(0,0), (10,0), (10,8), (0,8)]
    thickness: 0.25 m
    material: reinforced_concrete
    slab_type: floor

  Stair: ST-01
    from_storey: Ground Floor
    to_storey: First Floor
    type: l_shaped
    width: 1.0 m
    riser_count: 17
    riser_height: 0.182 m
    tread_depth: 0.27 m
    position: (1.0, 6.0)
    direction: (0, 1)
```

---

*This spec is a living document. All design decisions for v0.1 have been resolved. Next step: start with the tree-sitter grammar definition and Phase 1 implementation in Rust.*
