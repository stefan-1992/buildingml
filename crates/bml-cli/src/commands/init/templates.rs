use super::Template;

pub const GITIGNORE: &str = "*.ifc
dist/
node_modules/
.env
";

pub fn buildingml_toml(_display_name: &str) -> String {
    r#"buildingml_version = "0.1"
ifc_version = "IFC4"
output_format = "step"
entry = "project.bml"

# Asset libraries auto-included for all files in this project.
# Remove or comment out to manage imports manually.
assets = ["assets/furniture.bml"]

[units]
length = "m"
area = "m2"
angle = "deg"
"#
    .to_string()
}

pub fn project_bml_nested(display_name: &str, template: Template) -> String {
    let imports = imports_nested(template);

    format!(
        r#"{imports}
# Site: {display_name} Site
  location: 48.1351, 11.5820
  address: "{display_name}"

## Building: {display_name}
  type: {building_type}
"#,
        imports = imports,
        display_name = display_name,
        building_type = match template {
            Template::Residential => "residential",
            Template::Office => "commercial",
            Template::MixedUse => "mixed_use",
        },
    )
}

pub fn project_bml_flat(display_name: &str, template: Template) -> String {
    let imports = imports_flat(template);
    let building_content = flat_building_content(template);

    format!(
        r#"{imports}
# Site: {display_name} Site
  location: 48.1351, 11.5820
  address: "{display_name}"

## Building: {display_name}
  type: {building_type}

{building_content}"#,
        imports = imports,
        display_name = display_name,
        building_type = match template {
            Template::Residential => "residential",
            Template::Office => "commercial",
            Template::MixedUse => "mixed_use",
        },
        building_content = building_content,
    )
}

/// For flat mode, concatenate all storey content inline in project.bml.
fn flat_building_content(template: Template) -> String {
    let files = storey_files(template);
    let mut content = String::new();
    if let Some(tmpl) = templates_bml(template) {
        content.push_str(tmpl);
        content.push('\n');
    }
    for (_filename, storey_content) in files {
        content.push_str(storey_content);
        content.push('\n');
    }
    content
}

fn imports_nested(template: Template) -> String {
    let mut imports = String::new();
    imports.push_str("import materials from \"./materials.bml\"\n");
    if matches!(template, Template::MixedUse) {
        imports.push_str("import templates from \"./templates.bml\"\n");
    }
    // Storey imports are top-level declarations and must appear before any # headings
    imports.push_str("import ground from \"./storeys/ground-floor.bml\"\n");
    if matches!(template, Template::MixedUse) {
        imports.push_str("import first from \"./storeys/first-floor.bml\"\n");
    }
    imports.push_str("import assets from \"./assets/furniture.bml\"\n");
    imports.push_str("import checks from \"./checks/compliance.bml\"\n");
    imports
}

/// Returns (filename, content) pairs for storey files to write into storeys/.
pub fn storey_files(template: Template) -> Vec<(&'static str, &'static str)> {
    match template {
        Template::Residential => vec![("ground-floor.bml", RESIDENTIAL_GROUND_FLOOR)],
        Template::Office => vec![("ground-floor.bml", OFFICE_GROUND_FLOOR)],
        Template::MixedUse => vec![
            ("ground-floor.bml", MIXED_USE_GROUND_FLOOR),
            ("first-floor.bml", MIXED_USE_FIRST_FLOOR),
        ],
    }
}

/// Returns templates.bml content if the template uses floor templates.
pub fn templates_bml(template: Template) -> Option<&'static str> {
    match template {
        Template::MixedUse => Some(MIXED_USE_TEMPLATE),
        _ => None,
    }
}

fn imports_flat(_template: Template) -> String {
    let mut imports = String::new();
    imports.push_str("import materials from \"./materials.bml\"\n");
    imports
}

pub fn materials_bml(template: Template) -> &'static str {
    match template {
        Template::Residential => RESIDENTIAL_MATERIALS,
        Template::Office => OFFICE_MATERIALS,
        Template::MixedUse => MIXED_USE_MATERIALS,
    }
}

pub fn compliance_bml(template: Template) -> &'static str {
    match template {
        Template::Residential => RESIDENTIAL_CHECKS,
        Template::Office => OFFICE_CHECKS,
        Template::MixedUse => MIXED_USE_CHECKS,
    }
}

// ---------------------------------------------------------------------------
// Material definitions
// ---------------------------------------------------------------------------

const RESIDENTIAL_MATERIALS: &str = r#"// Material definitions for residential project

Material: reinforced_concrete
  category: concrete
  grade: C30
  density: 2400 kg/m3

Material: exterior_masonry
  category: masonry
  density: 1800 kg/m3

Material: interior_partition
  category: concrete
  density: 1200 kg/m3

Material: timber_frame
  category: wood
  density: 500 kg/m3
"#;

const OFFICE_MATERIALS: &str = r#"// Material definitions for office project

Material: reinforced_concrete
  category: concrete
  grade: C30
  density: 2400 kg/m3

Material: curtain_wall_glass
  category: glass
  density: 2500 kg/m3

Material: interior_partition
  category: concrete
  density: 1200 kg/m3

Material: steel_frame
  category: steel
  grade: S355
  density: 7850 kg/m3
"#;

const MIXED_USE_MATERIALS: &str = r#"// Material definitions for mixed-use project

Material: reinforced_concrete
  category: concrete
  grade: C30
  density: 2400 kg/m3

Material: exterior_masonry
  category: masonry
  density: 1800 kg/m3

Material: curtain_wall_glass
  category: glass
  density: 2500 kg/m3

Material: interior_partition
  category: concrete
  density: 1200 kg/m3
"#;

// ---------------------------------------------------------------------------
// Storey files — written to storeys/ subdirectory in nested mode,
// or used as building_content() for flat mode.
// ---------------------------------------------------------------------------

const RESIDENTIAL_GROUND_FLOOR: &str = r#"### Storey: Ground Floor
  elevation: 0.0 m
  height: 2.80 m

  Slab: SLAB-GF
    boundary: [(0,0), (10,0), (10,8), (0,8)]
    thickness: 0.25 m
    material: reinforced_concrete
    slab_type: floor

#### Space: Living Room
  usage: living
  area: 32.0 m2

  Wall: W-GF-S1
    from: (0, 0) to: (10, 0)
    height: 2.80 m
    thickness: 0.30 m
    material: exterior_masonry
    is_external: true
    load_bearing: true

    Window: WIN-GF-01
      offset: 2.0 m
      width: 2.0 m
      height: 1.6 m
      sill_height: 0.80 m
      type: triple_glazed

    Door: D-FRONT
      offset: 7.0 m
      width: 1.10 m
      height: 2.10 m
      type: single_leaf
      is_external: true

  Wall: W-GF-E1
    from: (10, 0) to: (10, 4)
    height: 2.80 m
    thickness: 0.30 m
    material: exterior_masonry
    is_external: true
    load_bearing: true

    Window: WIN-GF-02
      offset: 0.5 m
      width: 1.2 m
      height: 1.4 m
      sill_height: 0.80 m
      type: triple_glazed

  Wall: W-GF-I1
    from: (0, 4) to: (10, 4)
    height: 2.80 m
    thickness: 0.115 m
    material: interior_partition
    is_external: false
    load_bearing: false

    Door: D-GF-01
      offset: 4.0 m
      width: 0.90 m
      height: 2.10 m
      type: single_leaf

  Wall: W-GF-W1
    from: (0, 4) to: (0, 0)
    height: 2.80 m
    thickness: 0.30 m
    material: exterior_masonry
    is_external: true
    load_bearing: true

#### Space: Kitchen
  usage: kitchen
  area: 20.0 m2

  Wall: W-GF-N1
    from: (5, 8) to: (0, 8)
    height: 2.80 m
    thickness: 0.30 m
    material: exterior_masonry
    is_external: true
    load_bearing: true

    Window: WIN-GF-03
      offset: 1.0 m
      width: 1.8 m
      height: 1.4 m
      sill_height: 0.90 m
      type: triple_glazed

  Wall: W-GF-W2
    from: (0, 8) to: (0, 4)
    height: 2.80 m
    thickness: 0.30 m
    material: exterior_masonry
    is_external: true
    load_bearing: true

  Wall: W-GF-I2
    from: (5, 4) to: (5, 8)
    height: 2.80 m
    thickness: 0.115 m
    material: interior_partition
    is_external: false
    load_bearing: false

    Door: D-GF-02
      offset: 0.5 m
      width: 0.90 m
      height: 2.10 m
      type: single_leaf

#### Space: Bathroom
  usage: bathroom
  area: 8.0 m2

  Wall: W-GF-N2
    from: (8, 8) to: (5, 8)
    height: 2.80 m
    thickness: 0.30 m
    material: exterior_masonry
    is_external: true
    load_bearing: true

    Window: WIN-GF-04
      offset: 0.5 m
      width: 0.6 m
      height: 0.6 m
      sill_height: 1.60 m
      type: double_glazed

  Wall: W-GF-I3
    from: (5, 4) to: (8, 4)
    height: 2.80 m
    thickness: 0.115 m
    material: interior_partition
    is_external: false
    load_bearing: false

    Door: D-GF-03
      offset: 0.5 m
      width: 0.80 m
      height: 2.10 m
      type: single_leaf

  Wall: W-GF-I4
    from: (8, 4) to: (8, 8)
    height: 2.80 m
    thickness: 0.115 m
    material: interior_partition
    is_external: false
    load_bearing: false

#### Space: Bedroom
  usage: bedroom
  area: 16.0 m2

  Wall: W-GF-E2
    from: (10, 4) to: (10, 8)
    height: 2.80 m
    thickness: 0.30 m
    material: exterior_masonry
    is_external: true
    load_bearing: true

    Window: WIN-GF-05
      offset: 0.5 m
      width: 1.5 m
      height: 1.4 m
      sill_height: 0.85 m
      type: triple_glazed

  Wall: W-GF-N3
    from: (10, 8) to: (8, 8)
    height: 2.80 m
    thickness: 0.30 m
    material: exterior_masonry
    is_external: true
    load_bearing: true
"#;

const OFFICE_GROUND_FLOOR: &str = r#"### Storey: Ground Floor
  elevation: 0.0 m
  height: 3.00 m

  Slab: SLAB-GF
    boundary: [(0,0), (15,0), (15,10), (0,10)]
    thickness: 0.25 m
    material: reinforced_concrete
    slab_type: floor

  Column: COL-GF-01
    position: (5.0, 5.0)
    height: 3.00 m
    profile: rectangle(0.30, 0.30)
    material: reinforced_concrete

  Column: COL-GF-02
    position: (10.0, 5.0)
    height: 3.00 m
    profile: rectangle(0.30, 0.30)
    material: reinforced_concrete

#### Space: Reception
  usage: lobby
  area: 30.0 m2

  Wall: W-GF-S1
    from: (0, 0) to: (15, 0)
    height: 3.00 m
    thickness: 0.30 m
    material: curtain_wall_glass
    is_external: true
    load_bearing: true

    Door: D-MAIN
      offset: 6.0 m
      width: 1.80 m
      height: 2.20 m
      type: double_leaf
      is_external: true

    Window: WIN-GF-01
      offset: 1.0 m
      width: 3.0 m
      height: 2.0 m
      sill_height: 0.50 m
      type: triple_glazed

    Window: WIN-GF-02
      offset: 10.0 m
      width: 3.0 m
      height: 2.0 m
      sill_height: 0.50 m
      type: triple_glazed

  Wall: W-GF-I1
    from: (0, 4) to: (15, 4)
    height: 3.00 m
    thickness: 0.115 m
    material: interior_partition
    is_external: false
    load_bearing: false

    Door: D-GF-01
      offset: 2.0 m
      width: 0.90 m
      height: 2.10 m
      type: single_leaf

    Door: D-GF-02
      offset: 7.0 m
      width: 0.90 m
      height: 2.10 m
      type: single_leaf

    Door: D-GF-03
      offset: 11.5 m
      width: 1.20 m
      height: 2.10 m
      type: double_leaf

  Wall: W-GF-W1
    from: (0, 4) to: (0, 0)
    height: 3.00 m
    thickness: 0.30 m
    material: reinforced_concrete
    is_external: true
    load_bearing: true

  Wall: W-GF-E1
    from: (15, 0) to: (15, 4)
    height: 3.00 m
    thickness: 0.30 m
    material: reinforced_concrete
    is_external: true
    load_bearing: true

#### Space: Office A
  usage: office
  area: 25.0 m2

  Wall: W-GF-W2
    from: (0, 10) to: (0, 4)
    height: 3.00 m
    thickness: 0.30 m
    material: reinforced_concrete
    is_external: true
    load_bearing: true

    Window: WIN-GF-03
      offset: 1.5 m
      width: 2.0 m
      height: 1.5 m
      sill_height: 0.80 m
      type: triple_glazed

  Wall: W-GF-N1
    from: (5, 10) to: (0, 10)
    height: 3.00 m
    thickness: 0.30 m
    material: reinforced_concrete
    is_external: true
    load_bearing: true

  Wall: W-GF-I2
    from: (5, 4) to: (5, 10)
    height: 3.00 m
    thickness: 0.115 m
    material: interior_partition
    is_external: false
    load_bearing: false

#### Space: Office B
  usage: office
  area: 25.0 m2

  Wall: W-GF-I3
    from: (5, 10) to: (10, 10)
    height: 3.00 m
    thickness: 0.30 m
    material: reinforced_concrete
    is_external: true
    load_bearing: true

  Wall: W-GF-I4
    from: (10, 4) to: (10, 10)
    height: 3.00 m
    thickness: 0.115 m
    material: interior_partition
    is_external: false
    load_bearing: false

#### Space: Meeting Room
  usage: office
  area: 25.0 m2

  Wall: W-GF-E2
    from: (15, 4) to: (15, 10)
    height: 3.00 m
    thickness: 0.30 m
    material: reinforced_concrete
    is_external: true
    load_bearing: true

    Window: WIN-GF-04
      offset: 1.5 m
      width: 2.0 m
      height: 1.5 m
      sill_height: 0.80 m
      type: triple_glazed

  Wall: W-GF-N2
    from: (15, 10) to: (10, 10)
    height: 3.00 m
    thickness: 0.30 m
    material: reinforced_concrete
    is_external: true
    load_bearing: true
"#;

const MIXED_USE_TEMPLATE: &str = r#"// Floor template for residential storeys

Template: TypicalResidentialFloor

#### Space: Apartment {n}A
  usage: living
  area: 45.0 m2

  Wall: W-F{f}-S1
    from: (0, 0) to: (12, 0)
    height: 2.70 m
    thickness: 0.30 m
    material: exterior_masonry
    is_external: true
    load_bearing: true

    Window: WIN-F{f}-01
      offset: 2.0 m
      width: 2.0 m
      height: 1.5 m
      sill_height: 0.80 m
      type: triple_glazed

  Wall: W-F{f}-W1
    from: (0, 7.5) to: (0, 0)
    height: 2.70 m
    thickness: 0.30 m
    material: exterior_masonry
    is_external: true
    load_bearing: true

  Wall: W-F{f}-I1
    from: (0, 7.5) to: (12, 7.5)
    height: 2.70 m
    thickness: 0.115 m
    material: interior_partition
    is_external: false
    load_bearing: false

    Door: D-F{f}-01
      offset: 5.0 m
      width: 0.90 m
      height: 2.10 m
      type: single_leaf

  Wall: W-F{f}-I2
    from: (12, 0) to: (12, 7.5)
    height: 2.70 m
    thickness: 0.115 m
    material: interior_partition
    is_external: false
    load_bearing: false

#### Space: Apartment {n}B
  usage: living
  area: 45.0 m2

  Wall: W-F{f}-N1
    from: (24, 15) to: (12, 15)
    height: 2.70 m
    thickness: 0.30 m
    material: exterior_masonry
    is_external: true
    load_bearing: true

    Window: WIN-F{f}-02
      offset: 2.0 m
      width: 2.0 m
      height: 1.5 m
      sill_height: 0.80 m
      type: triple_glazed

  Wall: W-F{f}-E1
    from: (24, 0) to: (24, 15)
    height: 2.70 m
    thickness: 0.30 m
    material: exterior_masonry
    is_external: true
    load_bearing: true

  Wall: W-F{f}-I3
    from: (12, 7.5) to: (24, 7.5)
    height: 2.70 m
    thickness: 0.115 m
    material: interior_partition
    is_external: false
    load_bearing: false

    Door: D-F{f}-02
      offset: 5.0 m
      width: 0.90 m
      height: 2.10 m
      type: single_leaf
"#;

const MIXED_USE_GROUND_FLOOR: &str = r#"### Storey: Ground Floor
  elevation: 0.0 m
  height: 3.50 m

  Slab: SLAB-GF
    boundary: [(0,0), (24,0), (24,15), (0,15)]
    thickness: 0.30 m
    material: reinforced_concrete
    slab_type: floor

  Column: COL-GF-01
    position: (8.0, 7.5)
    height: 3.50 m
    profile: rectangle(0.40, 0.40)
    material: reinforced_concrete

  Column: COL-GF-02
    position: (16.0, 7.5)
    height: 3.50 m
    profile: rectangle(0.40, 0.40)
    material: reinforced_concrete

#### Space: Retail Space
  usage: lobby
  area: 180.0 m2

  Wall: W-GF-S1
    from: (0, 0) to: (24, 0)
    height: 3.50 m
    thickness: 0.30 m
    material: curtain_wall_glass
    is_external: true
    load_bearing: true

    Door: D-SHOP
      offset: 10.0 m
      width: 2.00 m
      height: 2.50 m
      type: double_leaf
      is_external: true

    Window: WIN-GF-01
      offset: 1.0 m
      width: 4.0 m
      height: 2.5 m
      sill_height: 0.30 m
      type: triple_glazed

    Window: WIN-GF-02
      offset: 15.0 m
      width: 4.0 m
      height: 2.5 m
      sill_height: 0.30 m
      type: triple_glazed

  Wall: W-GF-W1
    from: (0, 15) to: (0, 0)
    height: 3.50 m
    thickness: 0.30 m
    material: reinforced_concrete
    is_external: true
    load_bearing: true

  Wall: W-GF-E1
    from: (24, 0) to: (24, 15)
    height: 3.50 m
    thickness: 0.30 m
    material: reinforced_concrete
    is_external: true
    load_bearing: true

  Wall: W-GF-N1
    from: (24, 15) to: (0, 15)
    height: 3.50 m
    thickness: 0.30 m
    material: reinforced_concrete
    is_external: true
    load_bearing: true

    Door: D-BACK
      offset: 10.0 m
      width: 1.20 m
      height: 2.10 m
      type: single_leaf
      is_external: true

  Stair: ST-GF-01
    from_storey: Ground Floor
    to_storey: First Floor
    type: straight
    position: (22.0, 13.0)
    width: 1.2 m
    riser_count: 20
    riser_height: 0.175 m
    tread_depth: 0.27 m
"#;

const MIXED_USE_FIRST_FLOOR: &str = r#"### Storey: First Floor
  elevation: 3.50 m
  height: 2.70 m
  apply_template: TypicalResidentialFloor

  Slab: SLAB-F1
    boundary: [(0,0), (24,0), (24,15), (0,15)]
    thickness: 0.25 m
    material: reinforced_concrete
    slab_type: floor
"#;

// ---------------------------------------------------------------------------
// Furniture assets
// ---------------------------------------------------------------------------

pub const FURNITURE_ASSETS: &str = r#"// Reusable furniture asset definitions

# AssetLibrary: Furniture

## Asset: BasicDesk
  description: "Simple office desk"

### Part: Desktop
  geometry: box(1.2, 0.6, 0.03)
  position: (0, 0, 0.72)

### Part: LeftLeg
  geometry: box(0.05, 0.05, 0.72)
  position: (0.05, 0.05, 0)

### Part: RightLeg
  geometry: box(0.05, 0.05, 0.72)
  position: (1.1, 0.05, 0)

## Asset: BasicChair
  description: "Simple office chair"

### Part: Seat
  geometry: box(0.45, 0.45, 0.03)
  position: (0, 0, 0.45)

### Part: Backrest
  geometry: box(0.45, 0.03, 0.40)
  position: (0, 0.42, 0.48)
"#;

// ---------------------------------------------------------------------------
// Compliance checks
// ---------------------------------------------------------------------------

const RESIDENTIAL_CHECKS: &str = r#"// Compliance checks for residential building

CheckSuite: ResidentialCompliance
  jurisdiction: general
  description: "Basic residential building compliance checks"

Check: MinimumDoorWidth
  applies_to: Door
  assert: width >= 0.80
  severity: error
  message: "Door width must be at least 0.80m for accessibility"

Check: MinimumRoomHeight
  applies_to: Space
  where: usage in [living, bedroom, kitchen]
  assert: clear_height >= 2.40
  severity: error
  message: "Habitable rooms must have a minimum clear height of 2.40m"

Check: MaximumStoreyHeight
  applies_to: Storey
  assert: height <= 4.0
  severity: warning
  message: "Storey height exceeds 4.0m — verify structural design"
"#;

const OFFICE_CHECKS: &str = r#"// Compliance checks for office building

CheckSuite: OfficeCompliance
  jurisdiction: general
  description: "Basic office building compliance checks"

Check: MinimumDoorWidth
  applies_to: Door
  assert: width >= 0.80
  severity: error
  message: "Door width must be at least 0.80m for accessibility"

Check: MinimumOfficeArea
  applies_to: Space
  where: usage in [office]
  assert: area >= 8.0
  severity: warning
  message: "Office spaces should be at least 8.0 m2"

Check: MeetingRoomDoorWidth
  applies_to: Door
  where: is_external == false
  assert: width >= 0.90
  severity: warning
  message: "Interior doors should be at least 0.90m wide"
"#;

const MIXED_USE_CHECKS: &str = r#"// Compliance checks for mixed-use building

CheckSuite: MixedUseCompliance
  jurisdiction: general
  description: "Basic mixed-use building compliance checks"

Check: MinimumDoorWidth
  applies_to: Door
  assert: width >= 0.80
  severity: error
  message: "Door width must be at least 0.80m for accessibility"

Check: RetailStoreyHeight
  applies_to: Storey
  assert: height >= 3.0
  severity: warning
  message: "Commercial storeys should have at least 3.0m height"

Check: MinimumRoomHeight
  applies_to: Space
  where: usage in [living, bedroom, kitchen]
  assert: clear_height >= 2.40
  severity: error
  message: "Habitable rooms must have a minimum clear height of 2.40m"
"#;
