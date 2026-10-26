# Lessons Learned: LLM-Authored BML at Scale

Notes from generating `tests/fixtures/six_storey_residential.bml` — a 2,291-line fixture describing a 6-storey, 48-unit residential building with 326 elements. Written by Claude (Opus 4.6), February 2026.

---

## What Worked Well

### The heading hierarchy is genuinely LLM-friendly

The `#`/`##`/`###`/`####` spatial nesting is the language's strongest feature. It maps directly to how an LLM reasons about buildings: a site contains buildings, buildings contain storeys, storeys contain spaces, spaces contain elements. The indentation-based containment is unambiguous and trivially generatable. Markdown heading syntax sits squarely in LLM training distributions.

### The `from: (x1, y1) to: (x2, y2)` axis syntax is compact

Defining wall geometry in two points is the right abstraction. It's more compact than separate `start_point` / `end_point` properties and reads naturally. The axis value being parsed as a single property (`from`) avoids the ambiguity of multi-property geometry.

### Lint → Compile → Check → Render pipeline catches errors progressively

The four-stage validation pipeline is excellent for iterative authoring. Lint catches structural/type problems immediately without the cost of IFC generation. Compile validates the full pipeline. Check runs domain rules. Render gives visual confirmation. Each stage is fast enough to run in a tight loop.

### Type checking on properties prevents silent errors

The compiler's property schema (`type_check.rs:55-176`) validates every property against its expected type and flags unknown properties as warnings. This caught `is_external: true` on Roof immediately — a property that exists on Wall but not Roof. For LLM authoring, this strict-but-helpful validation is essential.

---

## What Was Difficult

### 1. Massive repetition with no mitigation mechanism

**The single biggest pain point.** This building has 5 residential floors with nearly identical layouts. Each floor needs its own:
- 10 apartment spaces with `usage:` and `area:` properties
- Exterior walls (4 walls × 7 properties each = 28 lines)
- Structural elements (4 columns + 1 beam + 1 slab = ~30 lines)
- Stair cores (2 stairs × 8 properties each = ~20 lines)
- Corridor space

The fully-detailed Floor 1 is ~550 lines. Repeating that identically for floors 2-5 would produce 2,200 additional lines of pure duplication. I had to compromise by using "minimal" definitions for floors 3-5 (just spaces + exterior walls + structure), which means those floors won't render complete floorplans.

**Impact:** A realistic fully-detailed version of this building would be **~4,500 lines** — beyond what's practical for a single LLM context window turn and tedious even for multiple turns.

**Feature needed:** Floor template / repetition mechanism. See §Recommendations below.

### 2. Element IDs must be globally unique but are manually managed

With 326 elements across 7 storeys, ensuring ID uniqueness is error-prone. The naming convention `<Type>-<Floor>-<Number>` helps, but:
- No autocomplete in raw BML (the LSP helps in VS Code, but not when generating from an LLM)
- A typo like `COL-F1-05` vs `COL-F2-05` can't be caught until compile time
- Cross-referencing IDs (e.g., stair `from_storey: Ground Floor` must exactly match `### Storey: Ground Floor`) is fragile

The compiler correctly catches duplicates (`E300`) and unresolved references, but prevention would be better than detection.

### 3. No storey-level element placement

The semantic model has `ResolvedStorey.elements: Vec<ResolvedElement>` (model.rs:34), suggesting elements can belong directly to a storey. But in practice, the parser requires elements to be inside a `#### Space:` block. Structural elements like slabs, columns, and beams logically belong to the storey, not to any particular space. I had to place them inside the last space declaration on each floor — a workaround that's semantically misleading.

A slab covering the entire 40m×18m building footprint doesn't "belong" to the east stairwell — it spans all spaces. This is an abstraction mismatch.

### 4. No elevator element type

The building has two elevator shafts running the full height. BML has `Stair:` but no `Elevator:` or `Shaft:` element type. The 11 element types (model.rs:59-71) cover most architectural elements, but vertical transportation via elevators — present in nearly every multi-storey building — has no representation.

I omitted elevators entirely from the fixture because there's no clean way to model them. In real buildings, elevator shafts are critical for spatial planning, structural loading, and fire compartmentation.

### 5. No room subdivision or interior partitions within apartments

Each apartment is modeled as a single `#### Space:` with an `area:` property. In reality, a 3-bedroom apartment has ~7 distinct rooms (living, kitchen, 3 bedrooms, bathroom, WC). The heading hierarchy only goes to `####` (Space), so there's no `#####` level for rooms within a space.

I could model each room as a separate `#### Space:`, but then:
- The apartment unit grouping is lost
- There's no concept of "these 7 spaces form Unit 101"
- Door connectivity between rooms isn't expressible

### 6. Railing and Ramp have no geometry

`Railing:` accepts only `height` and `length` — both optional. There's no `from:` axis, `position:`, or `boundary:` property. This means railings can't be placed in the coordinate system and don't render on floorplans. The fixture's 10 railings (balcony railings, terrace railings) exist in the model but are geometrically invisible.

Similarly, `Ramp:` accepts only `width` and `slope` — no position or axis. The parking ramp can't be located in the building geometry.

### 7. No way to model openings between walls (connectivity)

The spec mentions wall connectivity via endpoint proximity for L/T/X junctions, but there's no way to explicitly model:
- A doorway connecting two spaces (the door belongs to one wall, but which side?)
- Corridor access (which wall face is the "corridor side"?)
- Pass-through openings without a door

Doors are children of walls, but there's no spatial relationship linking a door to the space it opens into.

### 8. Space `usage:` enum is limited

The allowed values (type_check.rs:314-328) are: `living`, `bedroom`, `kitchen`, `bathroom`, `office`, `classroom`, `corridor`, `staircase`, `escape_route`, `circulation`, `storage`, `garage`, `utility`.

Missing for residential buildings: `laundry`, `lobby` (I used `circulation`), `parking` (I used `garage`), `plant_room` or `mechanical` (I used `utility`), `mail_room` (I used `utility`), `waste_management` (I used `utility`), `terrace`, `balcony`.

This leads to information loss — the compiler accepts the file, but a mail room and a plant room both show as `utility`, losing their distinct function.

---

## Recommendations: Features That Would Make LLM Authoring Practical

### R1: Floor Templates (High Priority)

A mechanism to define a floor layout once and stamp it across multiple storeys:

```
Template: TypicalResidentialFloor
  #### Space: Corridor
    usage: corridor
    area: 72.0 m2

  #### Space: Unit {N}01
    usage: living
    area: 95.0 m2
    Wall: W-{F}-{N}01-S
      from: (0, 0) to: (12, 0)
      ...

### Storey: Floor 2
  elevation: 6.50 m
  height: 3.00 m
  apply_template: TypicalResidentialFloor
    N: 2
    F: F2
```

This would reduce the fixture from ~2,300 lines to ~800 lines and eliminate the entire class of copy-paste ID errors. It directly addresses the LLM token efficiency goal in §1.1 of the spec.

**Alternative (simpler):** A `same_as: Floor 1` directive on a storey that copies all spaces and elements, auto-prefixing IDs with the new floor number.

### R2: Storey-Level Elements (Medium Priority)

Allow elements at `###` (storey) level without requiring a `####` Space wrapper:

```
### Storey: Floor 1
  elevation: 3.50 m

  Slab: SLAB-F1
    boundary: [(0,0), (40,0), (40,18), (0,18)]
    thickness: 0.25 m

  Column: COL-F1-01
    position: (8.0, 9.0)
    ...

  #### Space: Unit 101
    ...
```

The semantic model already supports this (`ResolvedStorey.elements`). The parser just needs to accept element declarations between `###` and the first `####`.

### R3: Elevator / Shaft Element Type (Medium Priority)

```
Elevator: EL-01
  position: (19.5, 8.5)
  shaft_width: 1.80 m
  shaft_depth: 2.00 m
  from_storey: Ground Floor
  to_storey: Roof
  capacity: 8 persons
  type: passenger    // passenger, freight, service
```

This fills the vertical transportation gap and is essential for realistic multi-storey buildings.

### R4: Richer Railing/Ramp Geometry (Low-Medium Priority)

Add `from:` axis to Railing and Ramp so they can be positioned:

```
Railing: RL-TERRACE-S
  from: (0, 0) to: (40, 0)
  height: 1.10 m

Ramp: RM-GF-01
  from: (16, 18) to: (19, 12)
  width: 5.50 m
  slope: 0.05
```

### R5: Extended Space Usage Enum (Low Priority)

Add at minimum: `lobby`, `laundry`, `mail_room`, `plant_room`, `waste`, `terrace`, `balcony`, `parking`, `mechanical`, `electrical`, `telecom`.

These are standard building functions that appear in virtually every multi-storey project. The current 13 values force lossy categorization.

### R6: Space Grouping / Zone Concept (Low Priority, Larger Design)

A mechanism to group spaces into logical units:

```
Zone: Unit 101
  type: dwelling
  spaces: [Living 101, Kitchen 101, Bedroom-1 101, Bedroom-2 101, Bath 101]
```

Or a simpler approach — allow `####` spaces to have a `zone:` or `unit:` property:

```
#### Space: Living Room 101
  usage: living
  area: 25.0 m2
  unit: 101
```

This preserves apartment-level identity when rooms are modeled individually.

### R7: Auto-ID Generation (Low Priority)

Allow the compiler to auto-generate IDs when not specified, or provide a shorthand:

```
Wall:                               // auto-generates W-F1-001
  from: (0, 0) to: (12, 0)
  ...

Wall: _                             // explicit "generate for me"
  from: (12, 0) to: (19, 0)
  ...
```

For LLM authoring, managing 326 unique IDs across a file is a significant cognitive (token) overhead. Many elements don't need human-readable IDs.

---

## Quantitative Analysis

| Metric | Value |
|--------|-------|
| Total lines | 2,291 |
| Lines per fully-detailed 3-bed apartment | 93 |
| Lines per fully-detailed 1-bed apartment | 68 |
| Lines per minimal apartment (space only) | 4 |
| Lines for structural elements per floor | ~30 |
| Lines for exterior walls per floor | ~28 |
| Estimated lines if all 48 units fully detailed | ~4,500 |
| Element count | 326 |
| Unique element IDs managed | 326 |
| Token estimate (input, ~3.5 chars/token) | ~19,000 |
| Token estimate if fully detailed | ~37,000 |

A fully detailed 48-unit building would consume roughly 37K tokens — feasible in a single Claude context window but approaching the limits of what can be generated in one turn without error accumulation.

For comparison, the same building in IFC STEP format compiled to 305KB (~87K tokens). BML achieves roughly **2.3× token compression** over IFC even in its current verbose form. With floor templates (R1), this could improve to **5-8× compression**.

---

## IFC Codegen: What Gets Lost

Some BML properties don't survive the trip to IFC:

| Issue | Detail |
|-------|--------|
| Slab property sets skipped | `IfcSlabBuilder` lacks `add_properties()` in ifc_rs. Even if you add `load_bearing:` to a Slab, it won't appear in the IFC output. |
| Window/Door type enums lost | `single_glazed`, `double_leaf`, etc. are validated by the compiler but have no IFC property set mapping. They exist only in the BML model. |
| Boolean properties as Labels | `ifc_rs::BoolPrimitive` is `pub(crate)`, so booleans are serialized as `IfcValue::Label("TRUE"/"FALSE")` — string labels, not proper IFC booleans. |
| Raw STEP splicing for 5 types | Columns, Beams, Stairs, Ramps, Railings are generated via raw STEP entity text injection (`ifc_writer.rs`), not through ifc_rs builders. These elements appear in the IFC but with minimal metadata and may not round-trip through other IFC tools. |
| Gable/hip roofs rendered flat | Only flat roofs have correct IFC geometry. Other roof types (`gable`, `hip`, `mansard`, `shed`) are accepted by the type checker but rendered as flat slabs at ridge height. |

**Practical impact:** For the 6-storey fixture, the IFC output is structurally complete (all 326 elements present), but property fidelity varies by element type. Walls and Windows have the richest IFC representation; Stairs and Railings have the thinnest.

## Grammar Ghosts: Parsed but Non-Functional Types

The tree-sitter grammar (`grammar.js:80-101`) accepts `Opening`, `Grid`, and `Dimension` as element types, but these have no entries in `semantic/model.rs::ElementType`, no codegen path, and no scene graph representation. They parse without error but produce no output — a potential trap for LLM generators that might discover them in the grammar and try to use them.

---

## Compiler Observations

### Things that could improve error messages

1. **Storey reference mismatches** — when `from_storey: Ground floor` doesn't match `### Storey: Ground Floor` (case difference), the error message could suggest the closest match.

2. **Missing required properties** — the error says "Wall is missing required property `from`" but doesn't show what properties *were* provided, making it hard to spot typos (e.g., `axis:` instead of `from:`).

3. **Opening overflow** — no validation that `offset + width` fits within the host wall's length. An LLM could place a 2m window at offset 9m in a 10m wall (offset 9 + width 2 = 11 > 10) and the compiler wouldn't flag it.

### Things that work well

1. **Unknown property warnings** (not errors) — this is the right severity. It allows forward-compatible files while still flagging likely typos.

2. **Fire rating enum shared across Wall and Door** — the `REI30`...`REI120` and `T30`...`T90` values in a single enum keeps cross-type references consistent.

3. **Unit normalization** — writing `thickness: 300 mm` or `thickness: 0.30 m` both work and normalize to SI internally. This is genuinely useful for LLM generation where the "natural" unit varies by property.

4. **Fast iteration** — lint + compile + check + render for a 2,291-line file takes ~2 seconds total. This enables tight generate-validate loops.