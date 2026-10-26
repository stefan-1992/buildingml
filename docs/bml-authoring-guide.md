# BML Authoring Best Practices

A living guide for generating correct, maintainable BuildingML (.bml) files. Derived from the language specification (`Specs.md`), the compiler's validation rules, and lessons learned building the `six_storey_residential.bml` test fixture.

---

## 1. File Structure

Order your BML file in this sequence:

```
1. File-level comments (purpose, dimensions, design notes)
2. Material definitions
3. Site heading with properties
4. Building heading with properties
5. Storeys (bottom to top)
   - Storey properties (elevation, height)
   - Spaces within the storey
     - Elements within each space (walls, doors, windows, furniture)
   - Storey-level elements (slabs, columns, beams — shared across spaces)
6. Roof storey
7. Compliance checks and check suites
```

**Why this order matters:** Materials must be defined before any element references them. Storeys must appear before stairs reference them via `from_storey`/`to_storey`. Checks reference element types that should already be defined.

## 2. Spatial Hierarchy

BML uses markdown heading levels to encode the IFC spatial hierarchy. Each level maps to exactly one IFC entity:

| Heading | Spatial Type | IFC Entity | Example |
|---------|-------------|------------|---------|
| `#`     | Site        | IfcSite    | `# Site: Parkview Residences` |
| `##`    | Building    | IfcBuilding | `## Building: Block A` |
| `###`   | Storey      | IfcBuildingStorey | `### Storey: Ground Floor` |
| `####`  | Space       | IfcSpace   | `#### Space: Lobby` |

**Rules:**
- Headings are always at column 0 (no indentation).
- The hierarchy must be strictly nested: Site > Building > Storey > Space.
- A `##` heading cannot appear inside a `####` block — the compiler validates nesting.
- Properties of a heading are indented (2 spaces by convention).

## 3. Coordinate System

### 3.1 Plan Coordinates

All geometry is specified in a 2D (x, y) local coordinate system per storey. Establish your axes early and document them:

```
// Layout:
//   x-axis: 0 (west) to 40 (east)
//   y-axis: 0 (south) to 18 (north)
```

**Best practice:** Place the origin at the southwest corner of the building footprint. This keeps all coordinates positive and makes cardinal directions intuitive.

### 3.2 Elevation

Vertical placement comes from the storey's `elevation` property. Elements inherit their storey's elevation automatically.

```
### Storey: Ground Floor
  elevation: 0.0 m
  height: 3.50 m

### Storey: Floor 1
  elevation: 3.50 m    // = Ground Floor elevation + height
  height: 3.00 m
```

**Best practice:** Ensure each storey's elevation equals the previous storey's elevation plus its height. This keeps the building geometrically consistent.

### 3.3 Wall Axis Lines

Walls are defined by their centerline axis. The wall extends half-thickness on each side:

```
Wall: W-01
  from: (0, 0) to: (10, 0)   // 10m wall along x-axis
  height: 3.00 m
  thickness: 0.30 m            // extends 0.15m on each side of the axis
```

**Note:** The `from:` property uses the axis value syntax — `from: (x1, y1) to: (x2, y2)` is a single property, not two separate properties.

## 4. Element Types Quick Reference

### 4.1 Wall (required: `from`, `height`, `thickness`)

```
Wall: W-GF-S1
  from: (0, 0) to: (12, 0)
  height: 3.00 m
  thickness: 0.30 m
  material: exterior_insulated
  is_external: true
  load_bearing: true
  fire_rating: REI90
```

Walls are the only element type that can contain children (Door and Window).

### 4.2 Door (required: `offset`, `width`, `height`) — nested in Wall

```
Wall: W-01
  from: (0, 0) to: (10, 0)
  height: 3.00 m
  thickness: 0.30 m

  Door: D-01
    offset: 2.0 m          // distance from wall start point along axis
    width: 0.90 m
    height: 2.10 m
    type: single_leaf       // single_leaf, double_leaf, sliding, revolving, folding
    fire_rating: T30
```

### 4.3 Window (required: `offset`, `width`, `height`, `sill_height`) — nested in Wall

```
  Window: WIN-01
    offset: 1.0 m
    width: 2.0 m
    height: 1.6 m
    sill_height: 0.90 m
    type: triple_glazed     // single_glazed, double_glazed, triple_glazed, fixed, casement, sliding
    u_value: 0.7 W/m2K
```

### 4.4 Slab (required: `boundary`, `thickness`)

```
Slab: SLAB-GF
  boundary: [(0,0), (40,0), (40,18), (0,18)]
  thickness: 0.25 m
  material: reinforced_concrete
  slab_type: floor          // floor, roof, landing
```

### 4.5 Column (required: `position`, `profile`, `height`)

```
Column: COL-01
  position: (8.0, 9.0)
  height: 3.00 m
  profile: rectangle(0.35, 0.35)   // also: circle(radius), i_profile(w, h, tw, tf)
  material: reinforced_concrete
```

### 4.6 Beam (required: `from`, `profile`)

```
Beam: B-01
  from: (0, 9) to: (40, 9)
  profile: rectangle(0.35, 0.55)
  material: reinforced_concrete
```

### 4.7 Stair (required: `from_storey`, `to_storey`, `type`, `width`, `riser_count`, `riser_height`, `tread_depth`)

```
Stair: ST-GF-W
  from_storey: Ground Floor     // must match exact storey name
  to_storey: Floor 1
  type: u_shaped                // straight, l_shaped, u_shaped, spiral
  position: (1.5, 2.0)
  width: 1.20 m
  riser_count: 20
  riser_height: 0.175 m
  tread_depth: 0.27 m
```

**Critical:** `from_storey` and `to_storey` must exactly match the name in the `### Storey: <Name>` heading.

### 4.8 Roof (required: `type`, `boundary`, `ridge_height`)

```
Roof: RF-01
  type: flat                    // flat, gable, hip, mansard, shed
  boundary: [(-0.3, -0.3), (40.3, -0.3), (40.3, 18.3), (-0.3, 18.3)]
  ridge_height: 0.3 m
```

### 4.9 Other Elements

```
Railing: RL-01               // optional: height, length
  height: 1.10 m
  length: 4.0 m

Ramp: RM-01                  // optional: width, slope
  width: 1.50 m
  slope: 0.05

Furniture: FUR-01             // optional: position, type
  position: (3.0, 6.0)
```

## 5. Material Definitions

Define materials at the top of the file, before the `# Site:` heading:

```
Material: reinforced_concrete
  category: concrete        // concrete, masonry, glass, steel
  grade: C30
  density: 2400 kg/m3

Material: exterior_insulated
  category: concrete
  density: 1800 kg/m3
```

Reference by ID in elements:

```
Wall: W-01
  ...
  material: reinforced_concrete
```

**Rule:** Material IDs must be unique. The compiler emits `E301` if a referenced material doesn't exist, and `E302` for duplicate material IDs.

## 6. Naming Conventions

### 6.1 Element IDs

Use a systematic scheme: `<TypeCode>-<Floor>-<Qualifier>`

| Type | Code | Example |
|------|------|---------|
| Wall | `W` | `W-GF-S1` (Ground Floor, South wall 1) |
| Door | `D` | `D-F1-101` (Floor 1, Unit 101 entry) |
| Window | `WIN` | `WIN-F1-101-01` (Floor 1, Unit 101, window 1) |
| Slab | `SLAB` | `SLAB-F2` |
| Column | `COL` | `COL-GF-05` |
| Beam | `B` | `B-F1-02` |
| Stair | `ST` | `ST-GF-W` (Ground Floor, West) |
| Railing | `RL` | `RL-F1-101` |
| Ramp | `RM` | `RM-GF-01` |
| Roof | `RF` | `RF-01` |

**Rule:** Element IDs must be globally unique across the entire file. The compiler emits `E300` for duplicates.

### 6.2 Storey Names

Use human-readable names: `Ground Floor`, `Floor 1`, `Floor 2`, ..., `Roof`. These are referenced by `from_storey`/`to_storey` in stairs, so keep them simple and consistent.

### 6.3 Space Names

For residential buildings, use unit numbers: `Unit 101`, `Unit 102`. For service spaces, use descriptive names: `Lobby`, `Plant Room`, `West Stairwell F1`.

## 7. Structural Layout

### 7.1 Column Grid

Establish a regular structural grid and document it:

```
// Structural grid: 8m spacing in x, 9m spacing in y
// Column line x: 0, 8, 16, 24, 32, 40
// Column line y: 0, 9, 18
```

Place columns at grid intersections. Columns go on every floor at the same (x, y) positions.

### 7.2 Beams

Place beams along primary structural spans, typically spanning between column lines. Beams carry the flat slab loads to the columns.

### 7.3 Slabs

One slab per storey, covering the full building footprint:

```
Slab: SLAB-F1
  boundary: [(0,0), (40,0), (40,18), (0,18)]
  thickness: 0.25 m
  material: reinforced_concrete
  slab_type: floor
```

### 7.4 Shear Walls

Model shear walls (at stair/elevator cores) as regular walls with `load_bearing: true` and appropriate `fire_rating`:

```
Wall: W-GF-SW1
  from: (3, 0) to: (3, 6)
  height: 3.50 m
  thickness: 0.25 m
  material: shear_wall_concrete
  is_external: false
  load_bearing: true
  fire_rating: REI90
```

## 8. Building Envelope

### 8.1 Exterior Walls

Every floor needs its exterior walls for a complete envelope. Mark them with:
- `is_external: true`
- `load_bearing: true`
- `fire_rating: REI90` (or appropriate rating)

### 8.2 Wall Connectivity

Walls auto-connect at shared endpoints. To form a closed room, ensure wall endpoints meet:

```
// Clockwise rectangle
Wall: W1  from: (0, 0) to: (10, 0)    // south
Wall: W2  from: (10, 0) to: (10, 8)   // east
Wall: W3  from: (10, 8) to: (0, 8)    // north
Wall: W4  from: (0, 8) to: (0, 0)     // west
```

### 8.3 Opening Placement

`offset` is measured from the wall's `from` point along the axis. Ensure:
- `offset + width < wall_length` — the opening fits within the wall
- `sill_height + height < wall_height` — the window fits vertically
- Openings don't overlap each other

## 9. Large Buildings: Managing Complexity

### 9.1 Detailed vs. Minimal Floors

For repetitive buildings, fully detail one or two representative floors and use minimal definitions for the rest:

**Fully detailed floor** — all walls, openings, partitions, furniture:
```
#### Space: Unit 101
  usage: living
  area: 95.0 m2

  Wall: W-F1-101-S
    from: (0, 0) to: (12, 0)
    ...
    Window: WIN-F1-101-01
      ...
```

**Minimal floor** — spaces with areas, exterior walls, structural elements:
```
#### Space: Unit 301
  usage: living
  area: 95.0 m2

// Just the exterior shell + structure
Wall: W-F3-EXT-S
  from: (0, 0) to: (40, 0)
  ...
```

This keeps files manageable while maintaining the spatial model's correctness.

### 9.2 Section Dividers

Use comment blocks to delineate major sections:

```
// =========================================================================
// Floor 1 — Typical Residential Floor (10 apartments)
// =========================================================================
```

Within a floor, group by function:

```
//// South Apartments ////
//// North Apartments ////
//// Stair Cores ////
```

### 9.3 Structural Element Placement

Place structural elements (slabs, columns, beams) at the storey level rather than inside individual spaces. They span across spaces and logically belong to the storey:

```
### Storey: Floor 1
  elevation: 3.50 m
  height: 3.00 m

#### Space: Unit 101
  ...

#### Space: Unit 102
  ...

  // Storey-level structural elements (placed in last space for parser convenience,
  // but logically belonging to the storey)
  Slab: SLAB-F1
    boundary: [(0,0), (40,0), (40,18), (0,18)]
    ...
```

**Note:** In the current BML parser, storey-level elements are placed within a Space block at the indentation level. Place them in the last space declaration on each storey.

## 10. Compliance Checks

### 10.1 Standalone Checks

For simple assertions that apply across the whole building:

```
Check: min_door_width
  description: "All doors must be at least 0.90 m wide"
  applies_to: Door
  assert: width >= 0.90
```

### 10.2 Check Suites with Filtering

Group related checks with regulatory metadata:

```
CheckSuite: fire_safety
  description: "Fire safety compliance"
  jurisdiction: DE
  version: "2024"
  section: "Section 4.2"

  Check: ext_wall_fire_rating
    description: "External walls require fire rating"
    applies_to: Wall
    where: is_external == true
    assert: fire_rating in [REI30, REI60, REI90, REI120]
```

### 10.3 Writing Effective Assertions

- `assert: <property> >= <value>` — numeric comparison
- `assert: <property> in [<val1>, <val2>, ...]` — enum membership
- `where:` — pre-filter which elements the check applies to
- Available operators: `>=`, `<=`, `==`, `!=`, `>`, `<`

## 11. Validation Workflow

Always validate your BML file through these steps:

```bash
# 1. Lint: catches syntax errors and semantic issues
buildingml lint my-building.bml

# 2. Compile: produces IFC output, verifies full pipeline
buildingml compile my-building.bml -o output.ifc

# 3. Check: runs compliance checks
buildingml check my-building.bml --format json -o report.json

# 4. Render: generates visual output for verification
buildingml render my-building.bml -o renders/ --all
```

**Expected outcomes:**
- Lint: zero errors, zero warnings
- Compile: successful IFC generation
- Check: all applicable checks pass (0 failures)
- Render: SVG floorplans for each storey + 6 view angles

## 12. Common Pitfalls

| Pitfall | Fix |
|---------|-----|
| Duplicate element IDs | Use the `<Type>-<Floor>-<Number>` scheme consistently |
| `is_external` on Roof elements | Remove it — Roof doesn't support this property |
| Mismatched storey references in Stair | Copy-paste the exact storey name from the heading |
| Opening wider than host wall | Check that `offset + width < wall_length` |
| Missing elevation on storey | Always specify `elevation: X.X m` — it controls vertical placement |
| Heading at wrong level | Site=`#`, Building=`##`, Storey=`###`, Space=`####` |
| Material referenced before defined | Move material definitions above the `# Site:` heading |
| Wall endpoints don't form closed rooms | Sketch the coordinate grid before coding walls |

## 13. Reference Fixture

See `tests/fixtures/six_storey_residential.bml` for a complete example of a 6-storey, 48-unit residential building demonstrating all the patterns in this guide:
- 4 material definitions
- 7 storeys (Ground + 5 residential + Roof)
- Detailed ground floor with 9 spaces (lobby, services, stairwells, parking)
- Fully detailed Floor 1 with 10 apartments (all unit types: studio, 1-bed, 2-bed, 3-bed corner)
- Structural grid (columns, beams, slabs) on every floor
- Stairs connecting all floors
- Flat roof with rooftop terrace and railings
- 3 compliance checks including a fire safety suite
