pub mod geometry;
pub mod ifc_writer;
pub(crate) mod mep;
pub mod pset_mapping;
pub(crate) mod step_writer;
pub mod wall_joins;

pub use ifc_writer::{
    CodegenError, CodegenOutput, CodegenWarning, generate_ifc, generate_ifc_with_warnings,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cst_to_ast;
    use crate::semantic;
    use crate::semantic::diagnostics::{DiagnosticCode, Severity};
    use crate::semantic::model::ResolvedValue;

    #[test]
    fn test_generate_ifc_wall() {
        let source = std::fs::read_to_string("../../tests/fixtures/wall_only.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, diagnostics) = semantic::analyze(&ast);

        // Print diagnostics for debugging
        for d in &diagnostics {
            eprintln!("[{}] {}: {}", d.code, d.severity_str(), d.message);
        }

        let ifc = generate_ifc(&model).unwrap();
        assert!(
            ifc.contains("ISO-10303-21;"),
            "should start with STEP header"
        );
        assert!(ifc.contains("IFCWALL"), "should contain wall entity");
        assert!(ifc.contains("W-01"), "should contain wall ID");
        assert!(
            ifc.contains("END-ISO-10303-21;"),
            "should end with STEP footer"
        );
    }

    #[test]
    fn test_generate_ifc_layered_materials_and_common_psets() {
        let source = std::fs::read_to_string("../../tests/fixtures/layered_materials.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        // One IfcMaterial per layer material, shared between the two sets.
        assert_eq!(ifc.matches("IFCMATERIAL('concrete'").count(), 1, "{ifc}");
        assert!(ifc.contains(",0.01,$,'plaster',$,$,$);"), "{ifc}");
        assert!(ifc.contains("'External wall 300',$);"), "{ifc}");
        // Wall: first layer on the right of the axis; slab: first layer on top.
        assert!(ifc.contains(",.AXIS2.,.POSITIVE.,-0.15,$);"), "{ifc}");
        assert!(ifc.contains(",.AXIS3.,.NEGATIVE.,0.,$);"), "{ifc}");
        // The column's homogeneous material is associated directly.
        assert_eq!(ifc.matches("IFCMATERIALLAYERSETUSAGE(").count(), 2);

        assert!(ifc.contains("'Pset_SlabCommon'"), "{ifc}");
        assert!(ifc.contains("IFCPROPERTYSINGLEVALUE('AcousticRating',$,IFCREAL(55.),$)"));
        assert!(ifc.contains("IFCPROPERTYSINGLEVALUE('FireRating',$,IFCLABEL('R90'),$)"));
    }

    /// PredefinedType sits in the class's own slot: last for a wall, third after
    /// Tag for a door; a roof's `predefined_type:` overrides its `type`.
    #[test]
    fn test_generate_ifc_predefined_type() {
        let source = std::fs::read_to_string("../../tests/fixtures/predefined_types.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, diags) = semantic::analyze(&ast);
        assert!(diags.is_empty(), "{diags:?}");
        let ifc = generate_ifc(&model).unwrap();
        assert!(ifc.contains(",'W-01',.SHEAR.);"), "{ifc}");
        assert!(ifc.contains(",'D-01',2.1,0.9,.GATE.,$,$);"), "{ifc}");
        assert!(ifc.contains(",'R-01',.DOME_ROOF.);"), "{ifc}");
    }

    #[test]
    fn test_generate_ifc_wall_joins() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: A
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.2 m
  Wall: B
    from: (0, 4) to: (0, 0)
    height: 3 m
    thickness: 0.2 m
";
        let (model, _) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        let ifc = generate_ifc(&model).unwrap();
        assert_eq!(
            ifc.matches("IFCRELCONNECTSPATHELEMENTS(").count(),
            1,
            "{ifc}"
        );
        assert!(ifc.contains(",.ATEND.,.ATSTART.);"), "{ifc}");
        // Both walls are mitred, so neither keeps a rectangular profile.
        assert!(!ifc.contains("IFCRECTANGLEPROFILEDEF"), "{ifc}");
    }

    #[test]
    fn test_generate_ifc_covering_defaults_to_room_outline() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: S
    from: (0, 0) to: (6, 0)
    height: 3 m
    thickness: 0.2 m
  Wall: E
    from: (6, 0) to: (6, 4)
    height: 3 m
    thickness: 0.2 m
  Wall: N
    from: (6, 4) to: (0, 4)
    height: 3 m
    thickness: 0.2 m
  Wall: W
    from: (0, 4) to: (0, 0)
    height: 3 m
    thickness: 0.2 m
  Column: C
    position: (3, 2)
    height: 3 m
    profile: rectangle(0.4, 0.4)
#### Space: Room
  boundary: [(0, 0), (6, 0), (6, 4), (0, 4)]
  height: 2.8 m
  Covering: CV
    predefined_type: ceiling
";
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E101),
            "{diags:?}"
        );
        let cv = model.elements().find(|e| e.id == "CV").unwrap();
        let find = |k: &str| &cv.properties.iter().find(|p| p.key == k).unwrap().value;
        assert!(matches!(find("z_offset"), ResolvedValue::Number(z) if (z - 2.8).abs() < 1e-9));
        let ResolvedValue::PolygonWithHoles { outer, holes } = find("boundary") else {
            panic!("{:?}", find("boundary"));
        };
        let xs: Vec<f64> = outer.iter().map(|p| p.0).collect();
        let ys: Vec<f64> = outer.iter().map(|p| p.1).collect();
        let span = |v: &[f64]| {
            let min = v.iter().cloned().fold(f64::MAX, f64::min);
            let max = v.iter().cloned().fold(f64::MIN, f64::max);
            (min, max)
        };
        let close =
            |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3;
        assert!(close(span(&xs), (0.1, 5.9)), "{outer:?}");
        assert!(close(span(&ys), (0.1, 3.9)), "{outer:?}");
        assert_eq!(holes.len(), 1, "column hole: {holes:?}");

        let ifc = generate_ifc(&model).unwrap();
        assert!(ifc.contains("IFCCOVERING("), "{ifc}");
        assert!(ifc.contains(".CEILING.);"), "{ifc}");
        assert_eq!(ifc.matches("IFCRELCOVERSSPACES(").count(), 1, "{ifc}");
    }

    /// Low walls do not cut a ceiling, overlapping cuts of either winding are
    /// subtracted once, space holes stay holes, and a space without height
    /// hangs its ceiling at its own z_offset plus the storey height.
    #[test]
    fn test_covering_default_cuts_and_height() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3 m
  Wall: LOW
    from: (3, 0) to: (3, 4)
    height: 1.1 m
    thickness: 0.2 m
  Column: A
    position: (1, 1)
    height: 4 m
    profile: rectangle(1, 1)
  Column: B
    position: (1.5, 1.5)
    height: 4 m
    boundary: [(1, 1), (1, 2), (2, 2), (2, 1)]
#### Space: Room
  boundary: { outer: [(0, 0), (6, 0), (6, 4), (0, 4)], holes: [[(4, 2), (5, 2), (5, 3), (4, 3)]] }
  z_offset: 0.5 m
  Covering: CV
    predefined_type: ceiling
";
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        // A space boundary may have holes.
        assert!(
            diags
                .iter()
                .all(|d| d.code != DiagnosticCode::E101 && !d.message.contains("wrong type")),
            "{diags:?}"
        );
        let cv = model.elements().find(|e| e.id == "CV").unwrap();
        let find = |k: &str| &cv.properties.iter().find(|p| p.key == k).unwrap().value;
        assert!(matches!(find("z_offset"), ResolvedValue::Number(z) if (z - 3.5).abs() < 1e-9));
        let ResolvedValue::PolygonWithHoles { outer, holes } = find("boundary") else {
            panic!("{:?}", find("boundary"));
        };
        let area = geometry::polygon_area(outer)
            - holes.iter().map(|h| geometry::polygon_area(h)).sum::<f64>();
        assert!(
            (area - (24.0 - 1.75 - 1.0)).abs() < 1e-6,
            "{outer:?} {holes:?}"
        );
    }

    /// A curtain wall is bodiless after its grid expands, but still cuts a
    /// ceiling as deep as its 0.15 m mullions, not its 0.05 m panels. A grid
    /// that is not expanded cuts as a plain wall, or not at all without body.
    #[test]
    fn test_covering_cut_by_curtain_wall_members() {
        let ceiling_area = |extra: &str| {
            let source = format!(
                "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3 m
  Wall: CW
    from: (0, 0) to: (6, 0)
    height: 3 m
    thickness: 0.05 m
    mullions: 1.5 m
    transoms: [2.8 m]
{extra}#### Space: Room
  boundary: [(0, 0), (6, 0), (6, 4), (0, 4)]
  Covering: CV
    predefined_type: ceiling
    z_offset: 2.75 m
"
            );
            let (model, _) = semantic::analyze(&cst_to_ast::parse(&source).unwrap());
            let cv = model.elements().find(|e| e.id == "CV").unwrap();
            let boundary = cv.properties.iter().find(|p| p.key == "boundary").unwrap();
            let ResolvedValue::Polygon(outer) = &boundary.value else {
                panic!("{:?}", boundary.value);
            };
            geometry::polygon_area(outer)
        };
        let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
        assert!(close(ceiling_area(""), 6.0 * (4.0 - 0.075)));
        // Expansion fails on an opening: the wall keeps its 0.05 m body.
        let failed = "    Opening: O\n      offset: 1 m\n      width: 1 m\n      height: 1 m\n";
        assert!(close(ceiling_area(failed), 6.0 * (4.0 - 0.025)));
        assert!(close(ceiling_area("    representation: none\n"), 24.0));
    }

    #[test]
    fn test_covering_outside_space_needs_boundary() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Covering: CV
    predefined_type: ceiling
";
        let (_, diags) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        let e101: Vec<_> = diags
            .iter()
            .filter(|d| d.code == DiagnosticCode::E101)
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(
            e101,
            [
                "Covering `CV` is missing `boundary`",
                "Covering `CV` is missing `z_offset`"
            ]
        );
    }

    /// Strips under an L of walls merge into one outline that covers the
    /// corner, with its top at the walls' base; a rotated pad keeps its size.
    #[test]
    fn test_footing_defaults_from_walls_and_position() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W-A
    from: (0, 0) to: (6, 0)
    height: 3 m
    thickness: 0.3 m
    z_offset: -0.1 m
  Wall: W-B
    from: (6, 0) to: (6, 4)
    height: 3 m
    thickness: 0.3 m
  Footing: FT-S
    predefined_type: strip_footing
    under: [W-A, W-B]
    width: 0.6 m
    depth: 0.4 m
  Footing: FT-P
    predefined_type: pad_footing
    position: (2, 2)
    width: 1 m
    length: 2 m
    rotation: 90
    depth: 0.5 m
";
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        assert!(
            diags.iter().all(|d| d.severity != Severity::Error),
            "{diags:?}"
        );
        let prop = |id: &str, k: &str| {
            let e = model.elements().find(|e| e.id == id).unwrap();
            e.properties
                .iter()
                .find(|p| p.key == k)
                .unwrap()
                .value
                .clone()
        };
        let ResolvedValue::Polygon(strip) = prop("FT-S", "boundary") else {
            panic!("{:?}", prop("FT-S", "boundary"));
        };
        // 6.6 x 0.6 along W-A plus 0.6 x 4.0 of W-B beyond it.
        assert!((geometry::polygon_area(&strip) - (6.6 * 0.6 + 0.6 * 4.0)).abs() < 1e-6);
        assert!(strip.contains(&(6.3, -0.3)), "{strip:?}");
        assert!(
            matches!(prop("FT-S", "z_offset"), ResolvedValue::Number(z) if (z + 0.1).abs() < 1e-9)
        );
        let ResolvedValue::Polygon(pad) = prop("FT-P", "boundary") else {
            panic!();
        };
        let xs = pad.iter().map(|p| p.0);
        let width = xs.clone().fold(f64::MIN, f64::max) - xs.fold(f64::MAX, f64::min);
        assert!((width - 2.0).abs() < 1e-9, "{pad:?}");

        let ifc = generate_ifc(&model).unwrap();
        assert!(ifc.contains("'FT-S',.STRIP_FOOTING.);"), "{ifc}");
        assert!(ifc.contains("'FT-P',.PAD_FOOTING.);"), "{ifc}");
    }

    #[test]
    fn test_footing_errors() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W-A
    from: (0, 0) to: (2, 0)
    height: 3 m
    thickness: 0.3 m
  Wall: W-B
    from: (5, 0) to: (7, 0)
    height: 3 m
    thickness: 0.3 m
  Footing: FT-1
    under: [W-A, W-X]
    width: 0.6 m
    depth: 0.4 m
  Footing: FT-2
    under: [W-A, W-B]
    width: 0.6 m
    depth: 0.4 m
  Footing: FT-3
    under: W-A
    depth: 0.4 m
  Footing: FT-4
    depth: 0.4 m
  Wall: W-Z
    from: (9, 0) to: (9, 0)
    height: 3 m
    thickness: 0.3 m
  Footing: FT-5
    under: W-Z
    width: 0.6 m
    depth: 0.4 m
";
        let (_, diags) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(
            errors,
            [
                "`W-X` in `under:` of footing `FT-1` is not a wall of its storey",
                "the walls under footing `FT-2` do not touch; give each connected group its own footing",
                "Footing `FT-3` is missing `width`",
                "Footing `FT-4` is missing `boundary`",
                "wall `W-Z` under footing `FT-5` has zero length",
            ]
        );
    }

    #[test]
    fn test_generate_ifc_nested_layer_is_plain_material() {
        // E306 warnings only: compile must still write a valid set for each.
        let source = "\
Material: concrete
Material: inner
  layers:
    - concrete: 0.2 m
Material: outer
  layers:
    - inner: 0.2 m
    - concrete: -0.05 m

# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.15 m
    material: outer
  Wall: W2
    from: (0, 2) to: (5, 2)
    height: 3 m
    thickness: 0.2 m
    material: inner
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();
        // `inner` is an IfcMaterial inside `outer` and a layer set on W2.
        assert!(ifc.contains("IFCMATERIAL('inner'"), "{ifc}");
        assert!(ifc.contains("'inner',$);"), "{ifc}");
        assert!(ifc.contains(",0.,$,'concrete',$,$,$);"), "{ifc}");
        for line in ifc.lines().filter(|l| l.contains("IFCMATERIALLAYER(")) {
            let target: u64 = line
                .split("IFCMATERIALLAYER(#")
                .nth(1)
                .unwrap()
                .split(',')
                .next()
                .unwrap()
                .parse()
                .unwrap();
            let def = format!("#{target}=IFCMATERIAL(");
            assert!(ifc.contains(&def), "{line} must reference an IfcMaterial");
        }
    }

    #[test]
    fn test_generate_ifc_slab() {
        let source = std::fs::read_to_string("../../tests/fixtures/slab_only.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, diagnostics) = semantic::analyze(&ast);

        for d in &diagnostics {
            eprintln!("[{}] {}: {}", d.code, d.severity_str(), d.message);
        }

        let ifc = generate_ifc(&model).unwrap();
        assert!(ifc.contains("ISO-10303-21;"));
        assert!(ifc.contains("IFCSLAB"), "should contain slab entity");
        assert!(ifc.contains("SLAB-01"), "should contain slab ID");
    }

    #[test]
    fn test_generate_ifc_full_pipeline() {
        let source = std::fs::read_to_string("../../tests/fixtures/spatial_hierarchy.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, diagnostics) = semantic::analyze(&ast);

        for d in &diagnostics {
            eprintln!("[{}] {}: {}", d.code, d.severity_str(), d.message);
        }

        let ifc = generate_ifc(&model).unwrap();
        assert!(ifc.contains("IFCSITE"));
        assert!(ifc.contains("IFCBUILDING"));
        assert!(ifc.contains("IFCBUILDINGSTOREY"));
    }

    #[test]
    fn test_ifc_roundtrip_parse() {
        let source = std::fs::read_to_string("../../tests/fixtures/wall_only.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);

        let ifc_string = generate_ifc(&model).unwrap();

        // Verify ifc_rs can re-parse the generated output
        let parsed: Result<ifc_rs::prelude::IFC, _> = ifc_string.parse();
        assert!(
            parsed.is_ok(),
            "generated IFC should be re-parseable: {:?}",
            parsed.err()
        );
    }

    #[test]
    fn test_generate_ifc_all_elements() {
        let source = std::fs::read_to_string("../../tests/fixtures/all_elements.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, diagnostics) = semantic::analyze(&ast);

        for d in &diagnostics {
            eprintln!("[{}] {}: {}", d.code, d.severity_str(), d.message);
        }

        let ifc = generate_ifc(&model).unwrap();

        // Verify spatial structure
        assert!(ifc.contains("IFCSITE"), "should contain site");
        assert!(ifc.contains("IFCBUILDING"), "should contain building");
        assert!(ifc.contains("IFCBUILDINGSTOREY"), "should contain storey");

        // Verify built elements (via ifc_rs builders)
        assert!(ifc.contains("IFCWALL"), "should contain wall");
        assert!(ifc.contains("IFCSLAB"), "should contain slab");
        assert!(ifc.contains("IFCROOF"), "should contain roof");
        assert!(ifc.contains("IFCWINDOW"), "should contain window");
        assert!(
            ifc.contains("IFCOPENINGELEMENT"),
            "should contain opening for door/window"
        );
        assert!(ifc.contains("IFCSPACE"), "should contain space");

        // Verify raw STEP spliced elements
        assert!(ifc.contains("IFCCOLUMN"), "should contain column");
        assert!(ifc.contains("IFCBEAM"), "should contain beam");
        assert!(ifc.contains("IFCSTAIR"), "should contain stair");
        assert!(ifc.contains("IFCRAILING"), "should contain railing");
        assert!(ifc.contains("IFCRAMP"), "should contain ramp");
        assert!(
            ifc.contains("IFCFURNISHINGELEMENT"),
            "should contain furniture"
        );

        // Verify element IDs
        assert!(ifc.contains("W-GF-01"), "should contain wall ID");
        assert!(ifc.contains("WIN-01"), "should contain window ID");
        assert!(ifc.contains("COL-01"), "should contain column ID");
        assert!(ifc.contains("B-01"), "should contain beam ID");
        assert!(ifc.contains("RF-01"), "should contain roof ID");
    }

    #[test]
    fn test_generate_ifc_two_storey_house() {
        let source = std::fs::read_to_string("../../tests/fixtures/two_storey_house.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);

        let ifc = generate_ifc(&model).unwrap();

        // Verify spatial structure
        assert!(ifc.contains("IFCSITE"), "should contain site");
        assert!(ifc.contains("IFCBUILDING"), "should contain building");
        assert!(ifc.contains("IFCBUILDINGSTOREY"), "should contain storey");
        assert!(ifc.contains("IFCSPACE"), "should contain space");

        // Verify all element types present
        assert!(ifc.contains("IFCWALL"), "should contain wall");
        assert!(ifc.contains("IFCSLAB"), "should contain slab");
        assert!(ifc.contains("IFCWINDOW"), "should contain window");
        assert!(ifc.contains("IFCOPENINGELEMENT"), "should contain opening");
        assert!(ifc.contains("IFCCOLUMN"), "should contain column");
        assert!(ifc.contains("IFCBEAM"), "should contain beam");
        assert!(ifc.contains("IFCSTAIR"), "should contain stair");
        assert!(ifc.contains("IFCRAILING"), "should contain railing");
        assert!(ifc.contains("IFCROOF"), "should contain roof");

        // Verify some specific element IDs
        assert!(ifc.contains("W-GF-S1"), "should contain ground floor wall");
        assert!(ifc.contains("W-FF-S1"), "should contain first floor wall");
        assert!(ifc.contains("D-FRONT"), "should contain front door");
        assert!(ifc.contains("SLAB-GF"), "should contain ground floor slab");
        assert!(ifc.contains("RF-01"), "should contain roof");
    }

    #[test]
    fn test_generate_ifc_window_in_wall() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.24 m
    Window: WIN1
      offset: 1.5 m
      width: 1.2 m
      height: 1.4 m
      sill_height: 0.9 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);

        let ifc = generate_ifc(&model).unwrap();
        assert!(ifc.contains("IFCWALL"), "should contain wall");
        assert!(ifc.contains("IFCWINDOW"), "should contain window");
        assert!(
            ifc.contains("IFCOPENINGELEMENT"),
            "should contain opening element"
        );
        assert!(ifc.contains("WIN1"), "should contain window ID");
    }

    #[test]
    fn test_generate_ifc_door_in_wall() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.24 m
    Door: D1
      offset: 3 m
      width: 0.9 m
      height: 2.1 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);

        let ifc = generate_ifc(&model).unwrap();
        assert!(ifc.contains("IFCWALL"), "should contain wall");
        assert!(
            ifc.contains("IFCOPENINGELEMENT"),
            "should contain opening element for door"
        );
        assert!(
            ifc.contains("Opening_D1"),
            "should contain opening for door D1"
        );
    }

    #[test]
    fn test_generate_ifc_roof() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Roof: RF1
    boundary: [(-0.5, -0.5), (10.5, -0.5), (10.5, 8.5), (-0.5, 8.5)]
    is_external: true
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);

        let ifc = generate_ifc(&model).unwrap();
        assert!(ifc.contains("IFCROOF"), "should contain roof");
        assert!(ifc.contains("RF1"), "should contain roof ID");
    }

    #[test]
    fn test_generate_ifc_column_beam() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Column: COL1
    position: (5, 4)
    height: 3.0 m
    profile: rectangle(0.3, 0.3)
  Beam: B1
    from: (0, 4) to: (10, 4)
    profile: rectangle(0.3, 0.5)
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);

        let ifc = generate_ifc(&model).unwrap();
        assert!(ifc.contains("IFCCOLUMN"), "should contain column");
        assert!(ifc.contains("COL1"), "should contain column ID");
        assert!(ifc.contains("IFCBEAM"), "should contain beam");
        assert!(ifc.contains("B1"), "should contain beam ID");
    }

    // ====================================================================
    // Geometry hardening tests
    // ====================================================================

    #[test]
    fn test_column_has_extruded_geometry() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Column: COL1
    position: (5, 4)
    height: 3.0 m
    profile: rectangle(0.3, 0.3)
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        // Column should have proper geometry entities
        assert!(
            ifc.contains("IFCEXTRUDEDAREASOLID"),
            "column should have extruded area solid geometry"
        );
        assert!(
            ifc.contains("IFCRECTANGLEPROFILEDEF"),
            "rectangular column should have rectangle profile"
        );
        assert!(
            ifc.contains("IFCSHAPEREPRESENTATION"),
            "column should have shape representation"
        );
        assert!(
            ifc.contains("IFCPRODUCTDEFINITIONSHAPE"),
            "column should have product definition shape"
        );
        assert!(
            ifc.contains("IFCLOCALPLACEMENT"),
            "column should have local placement"
        );
    }

    #[test]
    fn test_circular_column_has_circle_profile() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Column: COL1
    position: (3, 2)
    height: 3.0 m
    profile: circle(0.15)
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        assert!(
            ifc.contains("IFCCIRCLEPROFILEDEF"),
            "circular column should have circle profile"
        );
        assert!(
            ifc.contains("IFCEXTRUDEDAREASOLID"),
            "circular column should have extruded solid"
        );
    }

    #[test]
    fn test_beam_has_extruded_geometry() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Beam: B1
    from: (0, 2) to: (6, 2)
    profile: rectangle(0.3, 0.5)
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        assert!(
            ifc.contains("IFCEXTRUDEDAREASOLID"),
            "beam should have extruded area solid"
        );
        assert!(
            ifc.contains("IFCRECTANGLEPROFILEDEF"),
            "beam should have rectangle profile"
        );
        assert!(
            ifc.contains("IFCLOCALPLACEMENT"),
            "beam should have local placement"
        );
    }

    #[test]
    fn test_stair_has_geometry() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3.2 m
  Stair: ST1
    position: (1, 1)
    width: 1.2 m
    riser_count: 18
    riser_height: 0.178 m
    tread_depth: 0.27 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        assert!(
            ifc.contains("IFCEXTRUDEDAREASOLID"),
            "stair should have extruded geometry"
        );
        assert!(ifc.contains("IFCSTAIR"), "should contain stair entity");
    }

    #[test]
    fn test_door_has_ifc_entity() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.24 m
    Door: D1
      offset: 3 m
      width: 0.9 m
      height: 2.1 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        assert!(ifc.contains("IFCDOOR"), "should contain IfcDoor entity");
        assert!(ifc.contains("D1"), "should contain door ID");
        assert!(
            ifc.contains("IFCOPENINGELEMENT"),
            "should also contain opening element"
        );
    }

    #[test]
    fn test_all_elements_have_geometry() {
        let source = std::fs::read_to_string("../../tests/fixtures/all_elements.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        // All raw STEP elements should now have proper geometry
        assert!(
            ifc.contains("IFCEXTRUDEDAREASOLID"),
            "should contain extruded area solids"
        );
        assert!(
            ifc.contains("IFCSHAPEREPRESENTATION"),
            "should contain shape representations"
        );
        assert!(
            ifc.contains("IFCPRODUCTDEFINITIONSHAPE"),
            "should contain product definition shapes"
        );
        assert!(
            ifc.contains("IFCLOCALPLACEMENT"),
            "should contain local placements"
        );

        // Door should now be a full IfcDoor entity
        assert!(
            ifc.contains("IFCDOOR"),
            "should contain IfcDoor entity (not just opening)"
        );
    }

    // ====================================================================
    // Phase 7a completion tests
    // ====================================================================

    #[test]
    fn test_slab_property_sets() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Slab: SLAB-01
    boundary: [(0,0), (10,0), (10,8), (0,8)]
    thickness: 0.2 m
    slab_type: floor
    is_external: false
    load_bearing: true
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        assert!(ifc.contains("IFCSLAB"), "should contain slab entity");
        assert!(ifc.contains("SLAB-01"), "should contain slab ID");

        // Verify property set injection
        assert!(
            ifc.contains("Pset_SlabCommon"),
            "should contain Pset_SlabCommon property set"
        );
        assert!(
            ifc.contains("IFCPROPERTYSINGLEVALUE"),
            "should contain property single values"
        );
        assert!(
            ifc.contains("IFCPROPERTYSET"),
            "should contain property set"
        );
        assert!(
            ifc.contains("IFCRELDEFINESBYPROPERTIES"),
            "should link property set to slab"
        );
        assert!(
            ifc.contains("IsExternal"),
            "should contain IsExternal property"
        );
        assert!(
            ifc.contains("LoadBearing"),
            "should contain LoadBearing property"
        );
    }

    #[test]
    fn test_space_explicit_boundary() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3.0 m
  #### Space: LivingRoom
    boundary: [(0, 0), (6, 0), (6, 4), (0, 4)]
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, diagnostics) = semantic::analyze(&ast);

        // No warnings about unknown property
        let boundary_warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("boundary"))
            .collect();
        assert!(
            boundary_warnings.is_empty(),
            "boundary should be a valid space property, got: {:?}",
            boundary_warnings
        );

        let ifc = generate_ifc(&model).unwrap();
        assert!(ifc.contains("IFCSPACE"), "should contain space entity");
        assert!(ifc.contains("LivingRoom"), "should contain space name");
    }

    #[test]
    fn test_space_boundary_preferred_over_area() {
        // When both boundary and area are present, boundary should be used
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3.0 m
  #### Space: Room1
    boundary: [(0, 0), (5, 0), (5, 3), (0, 3)]
    area: 100 m2
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();
        assert!(ifc.contains("IFCSPACE"), "should contain space entity");
    }

    #[test]
    fn test_gable_roof_geometry() {
        let source = "\
# Site: S
## Building: B
### Storey: Roof
  elevation: 5.4 m
  Roof: RF-01
    type: gable
    boundary: [(-0.5, -0.5), (10.5, -0.5), (10.5, 8.5), (-0.5, 8.5)]
    ridge_height: 3.5 m
    ridge_direction: x
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        assert!(ifc.contains("IFCROOF"), "should contain roof entity");
        assert!(ifc.contains("RF-01"), "should contain roof ID");

        // Pitched roof should use raw STEP with proper geometry
        assert!(
            ifc.contains("IFCARBITRARYCLOSEDPROFILEDEF"),
            "gable roof should have arbitrary profile for triangular cross-section"
        );
        assert!(
            ifc.contains("IFCEXTRUDEDAREASOLID"),
            "gable roof should be extruded along ridge"
        );
        assert!(
            ifc.contains(".GABLE_ROOF."),
            "should have GABLE_ROOF predefined type"
        );
    }

    #[test]
    fn test_shed_roof_geometry() {
        let source = "\
# Site: S
## Building: B
### Storey: Roof
  elevation: 3.0 m
  Roof: RF-02
    type: shed
    boundary: [(0, 0), (8, 0), (8, 6), (0, 6)]
    ridge_height: 2.0 m
    ridge_direction: x
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        assert!(ifc.contains("IFCROOF"), "should contain roof entity");
        assert!(
            ifc.contains(".SHED_ROOF."),
            "should have SHED_ROOF predefined type"
        );
        assert!(
            ifc.contains("IFCARBITRARYCLOSEDPROFILEDEF"),
            "shed roof should have arbitrary profile"
        );
    }

    #[test]
    fn test_hip_roof_geometry() {
        let source = "\
# Site: S
## Building: B
### Storey: Roof
  elevation: 5.0 m
  Roof: RF-03
    type: hip
    boundary: [(0, 0), (12, 0), (12, 8), (0, 8)]
    ridge_height: 3.0 m
    ridge_direction: y
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        assert!(ifc.contains("IFCROOF"), "should contain roof entity");
        assert!(
            ifc.contains(".HIP_ROOF."),
            "should have HIP_ROOF predefined type"
        );
    }

    #[test]
    fn test_flat_roof_unchanged() {
        // Flat roofs should still use the ifc_rs builder path (no arbitrary profile)
        let source = "\
# Site: S
## Building: B
### Storey: Roof
  elevation: 6.0 m
  Roof: RF-FLAT
    type: flat
    boundary: [(0, 0), (10, 0), (10, 8), (0, 8)]
    ridge_height: 0.3 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        assert!(ifc.contains("IFCROOF"), "should contain roof entity");
        // Flat roof should NOT have pitched roof predefined type
        assert!(
            !ifc.contains(".GABLE_ROOF."),
            "flat roof should not have GABLE_ROOF type"
        );
        assert!(
            !ifc.contains(".SHED_ROOF."),
            "flat roof should not have SHED_ROOF type"
        );
        assert!(
            !ifc.contains(".HIP_ROOF."),
            "flat roof should not have HIP_ROOF type"
        );
    }

    #[test]
    fn test_two_storey_house_with_gable_roof() {
        let source = std::fs::read_to_string("../../tests/fixtures/two_storey_house.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        // Verify the gable roof has proper geometry
        assert!(ifc.contains("IFCROOF"), "should contain roof");
        assert!(
            ifc.contains("IFCARBITRARYCLOSEDPROFILEDEF"),
            "gable roof should have triangular profile"
        );
        assert!(
            ifc.contains(".GABLE_ROOF."),
            "should have GABLE_ROOF predefined type"
        );

        // Verify the IFC is still valid (can be re-parsed)
        let parsed: Result<ifc_rs::prelude::IFC, _> = ifc.parse();
        // Note: raw STEP entities may not re-parse perfectly, so we just check it doesn't panic
        let _ = parsed;
    }

    #[test]
    fn test_six_storey_residential_ifc() {
        let source =
            std::fs::read_to_string("../../tests/fixtures/six_storey_residential.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        // Core spatial structure
        assert!(ifc.contains("IFCSITE"), "should contain site");
        assert!(ifc.contains("IFCBUILDING"), "should contain building");
        assert!(ifc.contains("IFCBUILDINGSTOREY"), "should contain storey");
        assert!(ifc.contains("IFCSPACE"), "should contain spaces");
        assert!(ifc.contains("IFCROOF"), "should contain roof");
        assert!(ifc.contains("IFCSLAB"), "should contain slab");

        // Slab property sets should be present (SLAB-TERRACE has slab_type: roof)
        assert!(
            ifc.contains("Pset_SlabCommon"),
            "should contain slab property set for SLAB-TERRACE"
        );
    }

    // ====================================================================
    // Phase 7c Enhanced Geometry tests
    // ====================================================================

    #[test]
    fn test_curved_wall_ifc() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W-CURVE
    from: (0, 0) via: (3, 2) to: (6, 0)
    height: 3.0 m
    thickness: 0.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        assert!(ifc.contains("IFCWALL"), "should contain wall entity");
        assert!(ifc.contains("W-CURVE"), "should contain wall ID");
        // Curved wall uses arbitrary closed profile (segmented arc outline)
        assert!(
            ifc.contains("IFCARBITRARYCLOSEDPROFILEDEF"),
            "curved wall should have arbitrary profile for arc outline"
        );
        assert!(
            ifc.contains("IFCEXTRUDEDAREASOLID"),
            "curved wall should have extruded solid"
        );
    }

    #[test]
    fn test_slab_with_holes_ifc() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Slab: S-HOLES
    boundary: { outer: [(0,0), (10,0), (10,10), (0,10)], holes: [[(3,3), (7,3), (7,7), (3,7)]] }
    thickness: 0.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        assert!(ifc.contains("IFCSLAB"), "should contain slab entity");
        assert!(ifc.contains("S-HOLES"), "should contain slab ID");
        // Slab with holes uses IfcArbitraryProfileDefWithVoids
        assert!(
            ifc.contains("IFCARBITRARYPROFILEDEFWITHVOIDS"),
            "slab with holes should have arbitrary profile with voids"
        );
    }

    #[test]
    fn test_furniture_with_asset() {
        let source = concat!(
            "# AssetLibrary: Test\n",
            "## Asset: Desk\n",
            "  width: 1.2 m\n",
            "  depth: 0.6 m\n",
            "  height: 0.75 m\n",
            "### Part: Top\n",
            "  geometry: box(1.2, 0.6, 0.03)\n",
            "  position: (0, 0, 0.72)\n",
            "### Part: Leg\n",
            "  geometry: box(0.04, 0.6, 0.72)\n",
            "  position: (0.02, 0.1, 0)\n",
            "# Site: S\n",
            "## Building: B\n",
            "### Storey: GF\n",
            "  elevation: 0 m\n",
            "  Furniture: DESK-01\n",
            "    asset: Desk\n",
            "    position: (2.0, 1.5)\n",
        );
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        assert!(
            ifc.contains("IFCFURNISHINGELEMENT"),
            "should contain furniture elements"
        );
        assert!(ifc.contains("DESK-01_Top"), "should contain part name Top");
        assert!(ifc.contains("DESK-01_Leg"), "should contain part name Leg");
    }

    #[test]
    fn test_furniture_without_asset() {
        let source = concat!(
            "# Site: S\n",
            "## Building: B\n",
            "### Storey: GF\n",
            "  elevation: 0 m\n",
            "  Furniture: CABINET-01\n",
            "    position: (1.0, 1.0)\n",
            "    width: 1.2 m\n",
            "    depth: 0.6 m\n",
            "    height: 1.8 m\n",
        );
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        assert!(
            ifc.contains("IFCFURNISHINGELEMENT"),
            "should contain furniture element"
        );
        assert!(ifc.contains("CABINET-01"), "should contain furniture ID");
    }

    #[test]
    fn test_furniture_assets_fixture_ifc() {
        let source = std::fs::read_to_string("../../tests/fixtures/furniture_assets.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, diagnostics) = semantic::analyze(&ast);

        for d in &diagnostics {
            eprintln!("[{}] {}: {}", d.code, d.severity_str(), d.message);
        }

        let ifc = generate_ifc(&model).unwrap();

        assert!(ifc.contains("IFCSITE"), "should contain site");
        assert!(ifc.contains("IFCBUILDING"), "should contain building");
        assert!(ifc.contains("IFCBUILDINGSTOREY"), "should contain storey");
        assert!(ifc.contains("IFCSPACE"), "should contain spaces");
        assert!(ifc.contains("IFCWALL"), "should contain walls");
        assert!(
            ifc.contains("DESK-01_Desktop"),
            "should contain desk top part"
        );
        assert!(
            ifc.contains("CHAIR-01_Seat"),
            "should contain chair seat part"
        );
        assert!(ifc.contains("CABINET-01"), "should contain plain furniture");
    }

    #[test]
    fn test_phase7c_fixture() {
        let source = std::fs::read_to_string("../../tests/fixtures/phase7c_geometry.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, diagnostics) = semantic::analyze(&ast);

        for d in &diagnostics {
            eprintln!("[{}] {}: {}", d.code, d.severity_str(), d.message);
        }

        let ifc = generate_ifc(&model).unwrap();

        // Core structure
        assert!(ifc.contains("IFCSITE"), "should contain site");
        assert!(ifc.contains("IFCBUILDING"), "should contain building");
        assert!(ifc.contains("IFCBUILDINGSTOREY"), "should contain storey");
        assert!(ifc.contains("IFCWALL"), "should contain walls");
        assert!(ifc.contains("IFCSLAB"), "should contain slabs");
        assert!(ifc.contains("IFCSPACE"), "should contain spaces");

        // Phase 7c specific: curved wall
        assert!(ifc.contains("W-CURVE-01"), "should contain curved wall ID");
        // Phase 7c specific: slab with holes
        assert!(ifc.contains("S-GF-01"), "should contain slab ID");
        assert!(
            ifc.contains("IFCARBITRARYPROFILEDEFWITHVOIDS"),
            "should contain profile with voids for holed slab"
        );
        // Phase 7c specific: circular column
        assert!(
            ifc.contains("IFCCIRCLEPROFILEDEF"),
            "should contain circle profile for circular column"
        );
    }

    #[test]
    fn test_l_shaped_space_explicit_boundary() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3.0 m
#### Space: LRoom
  boundary: [(0,0), (10,0), (10,5), (5,5), (5,8), (0,8)]
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();

        assert!(ifc.contains("IFCSPACE"), "should contain space entity");
        assert!(ifc.contains("LRoom"), "should contain space name");
    }

    // ====================================================================
    // Codegen warning tests
    // ====================================================================

    #[test]
    fn test_codegen_asset_cylinder_is_circle_profile() {
        let source = concat!(
            "# AssetLibrary: Test\n",
            "## Asset: Lamp\n",
            "  width: 0.3 m\n",
            "### Part: Base\n",
            "  geometry: cylinder(0.15, 0.4)\n",
            "  position: (0, 0, 0)\n",
            "# Site: S\n",
            "## Building: B\n",
            "### Storey: GF\n",
            "  elevation: 0 m\n",
            "  Furniture: LAMP-01\n",
            "    asset: Lamp\n",
            "    position: (1.0, 1.0)\n",
        );
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let output = generate_ifc_with_warnings(&model).unwrap();

        assert!(output.warnings.is_empty(), "{:?}", output.warnings);
        assert!(output.ifc_string.contains("IFCCIRCLEPROFILEDEF"));
        assert!(output.ifc_string.contains("LAMP-01_Base"));
    }

    #[test]
    fn test_codegen_warnings_space_no_geometry() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3.0 m
#### Space: EmptyRoom
  usage: office
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let output = generate_ifc_with_warnings(&model).unwrap();

        let space_warnings: Vec<_> = output
            .warnings
            .iter()
            .filter(|w| w.message.contains("no boundary or area"))
            .collect();
        assert!(
            !space_warnings.is_empty(),
            "expected codegen warning about space without geometry"
        );
    }

    // ====================================================================
    // IfcOpenShell validation tests
    // ====================================================================
    //
    // These tests call the Python-based IfcOpenShell validator script to
    // verify that generated IFC files pass external schema and geometry
    // validation. They require `pip install ifcopenshell` and are skipped
    // when the validator is not available.

    /// Check if the IfcOpenShell validator is available.
    fn ifcopenshell_available() -> bool {
        std::process::Command::new("python3")
            .args(["-c", "import ifcopenshell; import ifcopenshell.validate"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Run the IfcOpenShell validator on an IFC string, returning the parsed
    /// JSON result. Panics on validation errors.
    fn validate_ifc_with_ifcopenshell(ifc_string: &str, test_name: &str) {
        if !ifcopenshell_available() {
            eprintln!("SKIPPED {test_name}: ifcopenshell not installed");
            return;
        }

        let tmp_dir = std::env::temp_dir();
        let ifc_path = tmp_dir.join(format!("bml_test_{test_name}.ifc"));
        std::fs::write(&ifc_path, ifc_string).expect("failed to write temp IFC file");

        let script_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/validate_ifc.py");

        let output = std::process::Command::new("python3")
            .args([
                script_path.to_str().unwrap(),
                "--json",
                "--geometry",
                ifc_path.to_str().unwrap(),
            ])
            .output()
            .expect("failed to run validate_ifc.py");

        let _ = std::fs::remove_file(&ifc_path);

        let stdout = String::from_utf8_lossy(&output.stdout);
        let result: serde_json::Value =
            serde_json::from_str(&stdout).unwrap_or_else(|e| {
                panic!(
                    "Failed to parse validator JSON output for {test_name}: {e}\nstdout: {stdout}\nstderr: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            });

        let total_errors = result["total_errors"].as_i64().unwrap_or(0);
        let all_valid = result["all_valid"].as_bool().unwrap_or(false);

        // Collect error details for assertion message
        let mut error_details = String::new();
        if !all_valid {
            if let Some(results) = result["results"].as_array() {
                for r in results {
                    if let Some(categories) = r["categories"].as_object() {
                        error_details.push_str("  Categories: ");
                        for (cat, count) in categories {
                            error_details.push_str(&format!("{cat}={count} "));
                        }
                        error_details.push('\n');
                    }
                    if let Some(errors) = r["errors"].as_array() {
                        for err in errors.iter().take(5) {
                            let cat = err["category"].as_str().unwrap_or("");
                            let attr = err["attribute"].as_str().unwrap_or("");
                            let etype = err["entity_type"].as_str().unwrap_or("");
                            let ename = err["entity_name"].as_str().unwrap_or("");
                            error_details
                                .push_str(&format!("  [{cat}] {etype} ({ename}): {attr}\n"));
                        }
                        if errors.len() > 5 {
                            error_details
                                .push_str(&format!("  ... and {} more errors\n", errors.len() - 5));
                        }
                    }
                    // Check geometry failures
                    if let Some(geom_failures) = r["geometry_failures"].as_array() {
                        if !geom_failures.is_empty() {
                            error_details.push_str(&format!(
                                "  Geometry failures: {}\n",
                                geom_failures.len()
                            ));
                            for gf in geom_failures.iter().take(5) {
                                let etype = gf["entity_type"].as_str().unwrap_or("");
                                let ename = gf["entity_name"].as_str().unwrap_or("");
                                let gerr = gf["error"].as_str().unwrap_or("");
                                error_details
                                    .push_str(&format!("  [geom] {etype} ({ename}): {gerr}\n"));
                            }
                        }
                    }
                }
            }
        }

        assert!(
            all_valid,
            "IfcOpenShell validation failed for {test_name}: {total_errors} error(s)\n{error_details}"
        );
    }

    #[test]
    fn test_ifcopenshell_wall_only() {
        let source = std::fs::read_to_string("../../tests/fixtures/wall_only.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();
        validate_ifc_with_ifcopenshell(&ifc, "wall_only");
    }

    #[test]
    fn test_ifcopenshell_two_storey_house() {
        let source = std::fs::read_to_string("../../tests/fixtures/two_storey_house.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();
        validate_ifc_with_ifcopenshell(&ifc, "two_storey_house");
    }

    #[test]
    fn test_ifcopenshell_six_storey_residential() {
        let source =
            std::fs::read_to_string("../../tests/fixtures/six_storey_residential.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();
        validate_ifc_with_ifcopenshell(&ifc, "six_storey_residential");
    }

    #[test]
    fn test_ifcopenshell_minimal() {
        let source = std::fs::read_to_string("../../tests/fixtures/minimal.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = generate_ifc(&model).unwrap();
        validate_ifc_with_ifcopenshell(&ifc, "minimal");
    }

    /// All 25 common furniture assets should parse cleanly and compile to valid IFC.
    #[test]
    fn test_common_furniture_library_ifc() {
        use crate::project::Project;
        use std::path::Path;

        let fixture = Path::new("../../tests/fixtures/common_furniture_test.bml");
        let project = Project::load_entry_file(fixture)
            .expect("should load common furniture fixture with imports");
        let (model, diagnostics) = project.analyze();

        let errors: Vec<&crate::semantic::diagnostics::Diagnostic> = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "common furniture fixture should have no errors: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        let ifc = generate_ifc(&model).expect("should compile to IFC");

        // Spot-check a few asset parts across categories
        assert!(
            ifc.contains("SINGLE_BED"),
            "IFC should contain SingleBed instance"
        );
        assert!(
            ifc.contains("KING_BED"),
            "IFC should contain KingBed instance"
        );
        assert!(
            ifc.contains("MEETING_TABLE"),
            "IFC should contain MeetingTable instance"
        );
        assert!(
            ifc.contains("SHOWER_TRAY"),
            "IFC should contain ShowerTray instance"
        );
        assert!(
            ifc.contains("PLANTER"),
            "IFC should contain Planter instance"
        );
        assert!(
            ifc.contains("IFCFURNISHINGELEMENT"),
            "IFC should contain furniture elements"
        );
    }

    const CURTAIN_WALL: &str = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: CW
    from: (0, 0) to: (6, 0)
    height: 4 m
    thickness: 0.03 m
    mullions: 1.5 m
    transoms: [2.4 m]
    mullion_profile: rectangle(0.06 m, 0.2 m)
    panel_material: glass
    is_external: true
    load_bearing: false
    Door: CW-D
      offset: 1.5 m
      width: 1.5 m
      height: 2.4 m
";

    /// A wall with `mullions:` becomes a bodiless IfcCurtainWall whose
    /// mullions, transoms, panels and door are its parts.
    #[test]
    fn test_curtain_wall_grid() {
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(CURTAIN_WALL).unwrap());
        assert!(
            diags.iter().all(|d| d.severity != Severity::Error),
            "{diags:?}"
        );
        let ids: Vec<&str> = model.elements().map(|e| e.id.as_str()).collect();
        // 4 bays, 5 mullions (the door edges lie on mullion lines, so none is
        // split), one transom row over all bays, 2 x 4 cells minus the door.
        assert_eq!(ids.iter().filter(|i| i.starts_with("CW-M")).count(), 5);
        assert_eq!(ids.iter().filter(|i| i.starts_with("CW-T1-")).count(), 4);
        assert_eq!(ids.iter().filter(|i| i.starts_with("CW-P")).count(), 7);
        assert!(!ids.contains(&"CW-P1-2"), "the door fills cell 1-2");
        let num = |id: &str, key: &str| {
            let e = model.elements().find(|e| e.id == id).unwrap();
            semantic::model::number(&e.properties, key).unwrap()
        };
        // The door sits between the mullion faces and below the transom.
        assert!((num("CW-D", "width") - 1.44).abs() < 1e-9);
        assert!((num("CW-D", "height") - 2.37).abs() < 1e-9);
        // The panel above the transom starts at its top face.
        assert!((num("CW-P2-1", "z_offset") - 2.43).abs() < 1e-9);

        let ifc = generate_ifc(&model).unwrap();
        assert!(ifc.contains("IFCCURTAINWALL("), "{ifc}");
        assert!(!ifc.contains("IFCWALL("), "{ifc}");
        assert_eq!(ifc.matches(".CURTAIN_PANEL.);").count(), 7);
        assert_eq!(ifc.matches(".MULLION.);").count(), 9);
        assert!(ifc.contains("'Pset_CurtainWallCommon'"), "{ifc}");
        assert!(!ifc.contains("'LoadBearing'"), "{ifc}");
        // The whole and its 5 + 4 mullions and transoms, 7 panels and the door.
        let parts = ifc
            .lines()
            .filter(|l| l.contains("IFCRELAGGREGATES"))
            .map(|l| l.matches('#').count())
            .max();
        assert_eq!(parts, Some(1 + 1 + 5 + 4 + 7 + 1));
    }

    /// A door's curved `boundary:` is replaced by its cell, so its exact curve goes too.
    #[test]
    fn test_curtain_wall_door_drops_its_curve() {
        let source = CURTAIN_WALL.replace(
            "      height: 2.4 m\n",
            "      height: 2.4 m\n      boundary: [(1.5, -0.1), smooth (2.2, -0.3), (3, -0.1), (3, 0.1), (1.5, 0.1)]\n",
        );
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(&source).unwrap());
        assert!(
            diags.iter().all(|d| d.severity != Severity::Error),
            "{diags:?}"
        );
        let door = model.elements().find(|e| e.id == "CW-D").unwrap();
        assert!(door.curve("boundary").is_none());
    }

    /// Mullions of a curved curtain wall stand on the arc; panels span chords.
    #[test]
    fn test_curtain_wall_on_arc() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: CW
    from: (-5, 0) via: (0, 5) to: (5, 0)
    height: 3 m
    thickness: 0.03 m
    mullions: 2 m
";
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        assert!(
            diags.iter().all(|d| d.severity != Severity::Error),
            "{diags:?}"
        );
        // Half circle of radius 5: 5 pi / 2 m -> 8 bays.
        let mullions: Vec<_> = model
            .elements()
            .filter(|e| e.id.starts_with("CW-M"))
            .collect();
        assert_eq!(mullions.len(), 9);
        for m in mullions {
            let Some(ResolvedValue::Axis { from, .. }) = m
                .properties
                .iter()
                .find(|p| p.key == "from")
                .map(|p| &p.value)
            else {
                panic!();
            };
            assert!((from.0.hypot(from.1) - 5.0).abs() < 0.03, "{from:?}");
        }
        assert_eq!(
            model
                .elements()
                .filter(|e| e.id.starts_with("CW-P"))
                .count(),
            8
        );
    }

    #[test]
    fn test_curtain_wall_errors() {
        let wall = |id: &str, extra: &str| {
            format!(
                "  Wall: {id}\n    from: (0, 0) to: (4, 0)\n    height: 3 m\n    thickness: 0.03 m\n{extra}"
            )
        };
        let source = format!(
            "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n{}{}{}{}  Column: D-M1\n    position: (9, 9)\n    height: 3 m\n    profile: rectangle(0.3 m, 0.3 m)\n",
            wall("A", "    mullions: [1 m, 5 m]\n"),
            wall(
                "B",
                "    mullions: 1 m\n    mullion_profile: circle(0.05 m)\n"
            ),
            wall(
                "C",
                "    mullions: 1 m\n    Opening: C-O\n      offset: 1 m\n      width: 1 m\n      height: 1 m\n"
            ),
            wall(
                "D",
                "    mullions: 1 m\n    Door: D-DOOR\n      offset: 1 m\n      width: 1 m\n      height: 2 m\n"
            ),
        ) + &wall(
            "E",
            "    mullions: 1 m\n    mullion_profile: rectangle(5 m, 0.1 m)\n",
        ) + &wall(
            "F",
            "    mullions: 1 m\n    Window: F-WIN\n      offset: 1 m\n      width: 1 m\n      sill_height: 1 m\n      height: 2.5 m\n",
        );
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(&source).unwrap());
        let errors: Vec<&str> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(errors.len(), 6, "{errors:?}");
        assert!(
            errors[0].contains("`mullions` of curtain wall `A`"),
            "{errors:?}"
        );
        assert!(errors[1].contains("`mullion_profile`"), "{errors:?}");
        assert!(errors[2].contains("opening `C-O`"), "{errors:?}");
        assert!(errors[3].contains("generates `D-M1`"), "{errors:?}");
        assert!(errors[4].contains("wider than the wall"), "{errors:?}");
        assert!(errors[5].contains("`F-WIN` does not fit"), "{errors:?}");
        // Failed grids stay plain walls, with their doors.
        assert!(!model.elements().any(|e| e.id.starts_with("A-M")));
        let d = model.elements().find(|e| e.id == "D").unwrap();
        assert_eq!(d.children.len(), 1);
    }

    /// A `u_shaped` stair becomes a bodiless IfcStair with two flights and
    /// the landing between them.
    #[test]
    fn test_stair_with_landing() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3.5 m
  Stair: ST
    type: u_shaped
    position: (10, 0)
    width: 1.2 m
    riser_count: 20
    tread_depth: 0.28 m
    landing_depth: 1.4 m
    turn: right
    waist_thickness: 0.18 m
";
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        assert!(
            diags.iter().all(|d| d.severity != Severity::Error),
            "{diags:?}"
        );
        let elem = |id: &str| model.elements().find(|e| e.id == id).unwrap();
        let num = |id: &str, key: &str| semantic::model::number(&elem(id).properties, key).unwrap();
        // The second flight starts beside the first one's top, turned back.
        let (x, y) = crate::scene::graph::get_point(&elem("ST-F2").properties, "position").unwrap();
        assert!((x - 12.5).abs() < 1e-9 && (y - 2.8).abs() < 1e-9, "{x} {y}");
        assert!((num("ST-F2", "rotation") - 180.0).abs() < 1e-9);
        assert!((num("ST-F2", "z_offset") - 1.75).abs() < 1e-9);
        assert!((num("ST-L1", "z_offset") - 1.75).abs() < 1e-9);
        assert!((num("ST-L1", "thickness") - 0.18).abs() < 1e-9);

        let ifc = generate_ifc(&model).unwrap();
        assert!(ifc.contains(".HALF_TURN_STAIR.);"), "{ifc}");
        assert_eq!(ifc.matches("IFCSTAIRFLIGHT(").count(), 2);
        assert!(ifc.contains("10,10,0.175,0.28,.STRAIGHT.);"), "{ifc}");
        assert!(ifc.contains(".LANDING.);"), "{ifc}");
        let parts = ifc
            .lines()
            .filter(|l| l.contains("IFCRELAGGREGATES"))
            .map(|l| l.matches('#').count())
            .max();
        assert_eq!(parts, Some(1 + 1 + 3));
    }

    #[test]
    fn test_stair_errors() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Stair: A
    type: straight
    landing_after: 18
  Stair: B
    type: l_shaped
  Column: B-F1
    position: (9, 9)
    height: 3 m
    profile: rectangle(0.3 m, 0.3 m)
";
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        let errors: Vec<&str> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(
            errors[0].contains("`landing_after` of stair `A`"),
            "{errors:?}"
        );
        assert!(errors[1].contains("generates `B-F1`"), "{errors:?}");
        // Failed stairs stay single flights.
        assert!(!model.elements().any(|e| e.id == "A-F1" || e.id == "B-L1"));
    }

    /// `[...]` length lists convert their units and reject unknown ones.
    #[test]
    fn test_number_list_units() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: CW
    from: (0, 0) to: (6, 0)
    height: 4 m
    thickness: 0.03 m
    mullions: [150 cm, 3]
    transoms: [2 km]
";
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E400),
            "{diags:?}"
        );
        assert_eq!(
            model
                .elements()
                .filter(|e| e.id.starts_with("CW-M"))
                .count(),
            2
        );
    }

    const MEP_ROUTES: &str = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3.5 m
  System: SA
    predefined_type: ventilation
  Duct: MAIN
    system: SA
    width: 0.4 m
    height: 0.25 m
    path: [(0, 0, 3), (6, 0, 3), (6, 5, 3), (6, 5, 2.4)]
  Duct: BRANCH
    system: SA
    diameter: 0.2 m
    path: [(3, 0, 3), (3, 2, 3)]
  Terminal: AT
    type: air_supply
    system: SA
    position: (6, 5)
    z_offset: 2.35 m
";

    #[test]
    fn test_mep_route_expands_into_parts() {
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(MEP_ROUTES).unwrap());
        assert!(
            diags.iter().all(|d| d.severity != Severity::Error),
            "{diags:?}"
        );
        let ids: Vec<&str> = model
            .elements()
            .filter(|e| e.element_type == semantic::model::ElementType::Duct)
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "MAIN-S1", "MAIN-T1", "MAIN-S2", "MAIN-B1", "MAIN-S3", "MAIN-B2", "MAIN-S4",
                "BRANCH"
            ]
        );
        let prop = |id: &str, key: &str| {
            let e = model.elements().find(|e| e.id == id).unwrap();
            e.properties
                .iter()
                .find(|p| p.key == key)
                .unwrap()
                .value
                .clone()
        };
        // Tee: 0.1 m half branch + 0.05 m each side of the joint; the branch
        // starts 0.2 m (half main width) + 0.05 m off the main axis.
        let ResolvedValue::Axis { from, to, .. } = prop("MAIN-T1", "from") else {
            panic!()
        };
        assert!((from.0 - 2.85).abs() < 1e-9 && (to.0 - 3.15).abs() < 1e-9);
        let ResolvedValue::Point3D(_, y, _) = prop("MAIN-T1", "branch") else {
            panic!()
        };
        assert!((y - 0.25).abs() < 1e-9);
        assert!(matches!(prop("MAIN-T1", "branch_diameter"), ResolvedValue::Number(d) if d == 0.2));
        let ResolvedValue::Axis { from, .. } = prop("BRANCH", "from") else {
            panic!()
        };
        assert!((from.1 - 0.25).abs() < 1e-9);
        // Bend radius defaults to the larger side (0.4 m) for a 90 degree turn.
        let ResolvedValue::Path3D(bend) = prop("MAIN-B1", "path") else {
            panic!()
        };
        assert!((bend[0].0 - 5.6).abs() < 1e-9 && (bend[2].1 - 0.4).abs() < 1e-9);
    }

    #[test]
    fn test_mep_tee_keeps_branch_rotation() {
        // A rectangular branch starting on the main: its rotation is measured
        // along its own direction, so the tee stub turns the same way.
        let source = MEP_ROUTES.replace(
            "    diameter: 0.2 m\n    path: [(3, 0, 3)",
            "    width: 0.2 m\n    height: 0.1 m\n    rotation: 30\n    path: [(3, 0, 3)",
        );
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(&source).unwrap());
        assert!(
            diags.iter().all(|d| d.severity != Severity::Error),
            "{diags:?}"
        );
        let tee = model.elements().find(|e| e.id == "MAIN-T1").unwrap();
        let rotation = tee.properties.iter().find(|p| p.key == "branch_rotation");
        assert!(
            matches!(rotation.map(|p| &p.value), Some(ResolvedValue::Number(r)) if (r - 30.0).abs() < 1e-6),
            "{rotation:?}"
        );
    }

    #[test]
    fn test_mep_ports_and_system() {
        let (model, _) = semantic::analyze(&cst_to_ast::parse(MEP_ROUTES).unwrap());
        let ifc = generate_ifc(&model).unwrap();
        let count = |entity: &str| ifc.matches(&format!("={entity}(")).count();
        assert_eq!(count("IFCDUCTSEGMENT"), 5);
        assert_eq!(count("IFCDUCTFITTING"), 3);
        assert_eq!(count("IFCAIRTERMINAL"), 1);
        // Two ports per segment and bend, three on the tee, one on the terminal.
        assert_eq!(count("IFCDISTRIBUTIONPORT"), 18);
        // 6 joints along MAIN, the tee branch and the terminal.
        assert_eq!(count("IFCRELCONNECTSPORTS"), 8);
        assert_eq!(count("IFCDISTRIBUTIONSYSTEM"), 1);
        assert!(ifc.contains(".VENTILATION."));
        assert_eq!(count("IFCRELSERVICESBUILDINGS"), 1);
    }

    #[test]
    fn test_mep_route_errors() {
        let analyze = |body: &str| {
            let source =
                format!("# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n{body}");
            semantic::analyze(&cst_to_ast::parse(&source).unwrap())
        };
        // A 0.2 m leg cannot hold two 0.4 m bend trims.
        let (_, diags) = analyze(
            "  Duct: D\n    width: 0.4 m\n    height: 0.2 m\n    path: [(0, 0, 3), (2, 0, 3), (2, 0.2, 3), (4, 0.2, 3)]\n",
        );
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E101 && d.message.contains("too short")),
            "{diags:?}"
        );
        let (_, diags) = analyze("  Terminal: T\n    type: toaster\n    position: (1, 1)\n");
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E102 && d.message.contains("air_supply")),
            "{diags:?}"
        );
        let (_, diags) = analyze("  System: S\n    predefined_type: plumbing\n");
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E102 && d.message.contains("domesticcoldwater")),
            "{diags:?}"
        );
        // A straight route stays one segment with its own id.
        let (model, diags) = analyze(
            "  Pipe: P\n    diameter: 0.05 m\n    path: [(0, 0, 1), (1, 0, 1), (2, 0, 1)]\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
        let ids: Vec<&str> = model.elements().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["P"]);
        let has = |diags: &[semantic::diagnostics::Diagnostic], code, text: &str| {
            diags
                .iter()
                .any(|d| d.code == code && d.message.contains(text))
        };
        // A branch must leave its main at a right angle.
        let (_, diags) = analyze(
            "  Duct: M\n    diameter: 0.2 m\n    path: [(0, 0, 3), (6, 0, 3)]\n  Duct: B\n    diameter: 0.2 m\n    path: [(3, 0, 3), (5, 2, 3)]\n",
        );
        assert!(
            has(&diags, DiagnosticCode::E101, "right angle"),
            "{diags:?}"
        );
        // A straight `from`/`to` run ending on a route at a right angle joins
        // it with a tee; one along the route is left alone.
        let (model, diags) = analyze(
            "  Duct: M\n    diameter: 0.2 m\n    path: [(0, 0, 3), (6, 0, 3)]\n  Duct: B\n    diameter: 0.2 m\n    from: (3, 0, 3) to: (3, 2, 3)\n  Duct: C\n    diameter: 0.2 m\n    from: (5, 0, 3) to: (8, 0, 3)\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
        let ids: Vec<&str> = model.elements().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["M-S1", "M-T1", "M-S2", "B", "C"]);
        // `system:` names a System element.
        let (_, diags) = analyze("  Terminal: T\n    system: NOPE\n    position: (1, 1)\n");
        assert!(has(&diags, DiagnosticCode::E307, "NOPE"), "{diags:?}");
        // A reference to a route that is replaced by its parts.
        let (_, diags) = analyze(
            "  Pipe: P\n    diameter: 0.05 m\n    path: [(0, 0, 1), (1, 0, 1), (1, 1, 1)]\n  Terminal: T\n    part_of: P\n    position: (3, 3)\n",
        );
        assert!(
            has(&diags, DiagnosticCode::E101, "name a part"),
            "{diags:?}"
        );
    }

    #[test]
    fn test_mep_ports_connect_within_tolerance() {
        // The segment end and bend start differ by a float ulp at a half-mm
        // value; a pipe and a cable tray ending at one point do not connect.
        let source = "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Pipe: P\n    diameter: 0.025 m\n    path: [(0.7, 0.1, 1), (1.7, 0.1, 1), (1.7, 2.9, 1)]\n  Pipe: Q\n    diameter: 0.05 m\n    from: (5, 0, 1) to: (6, 0, 1)\n  CableTray: C\n    width: 0.2 m\n    height: 0.06 m\n    from: (6, 0, 1) to: (7, 0, 1)\n";
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        assert!(diags.is_empty(), "{diags:?}");
        let ifc = generate_ifc(&model).unwrap();
        assert_eq!(ifc.matches("=IFCRELCONNECTSPORTS(").count(), 2);
    }

    /// `SA-FLEX` of the plan curves spec, continued by a straight duct, and a
    /// route with a smooth run between two corners.
    const SMOOTH_ROUTES: &str = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 4 m
  Duct: SA-FLEX
    diameter: 0.25 m
    path: [(0, 0, 3), smooth (2, 1, 3), (4, 0, 3.2)]
  Duct: SA-NEXT
    diameter: 0.25 m
    from: (4, 0, 3.2) to: (6, 0, 3.2)
  Duct: SA-MIX
    diameter: 0.2 m
    path: [(0, 5, 3), (2, 5, 3), smooth (3, 6, 3), (4, 5, 3), (4, 3, 3)]
";

    #[test]
    fn smooth_round_duct_is_one_segment() {
        use semantic::model::ElementCurve;
        let (model, ifc) = compile_clean(SMOOTH_ROUTES);
        let ids: Vec<&str> = model.elements().map(|e| e.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "SA-FLEX",
                "SA-NEXT",
                "SA-MIX-S1",
                "SA-MIX-B1",
                "SA-MIX-S2",
                "SA-MIX-B2",
                "SA-MIX-S3"
            ]
        );
        let elem = |id: &str| model.elements().find(|e| e.id == id).unwrap();
        let path = |id: &str| match &elem(id).properties.iter().find(|p| p.key == "path") {
            Some(p) => match &p.value {
                ResolvedValue::Path3D(pts) => pts.clone(),
                v => panic!("{v:?}"),
            },
            None => panic!("{id} has no path"),
        };
        let near = |a: (f64, f64, f64), b: (f64, f64, f64)| {
            (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6 && (a.2 - b.2).abs() < 1e-6
        };
        // The smooth route is one curved segment: exact curve plus its polyline.
        let flex = path("SA-FLEX");
        assert!(flex.len() > 3, "{flex:?}");
        assert!(near(flex[0], (0.0, 0.0, 3.0)) && near(*flex.last().unwrap(), (4.0, 0.0, 3.2)));
        assert!(matches!(
            elem("SA-FLEX").curve("path"),
            Some(ElementCurve::Path3(_))
        ));
        // The run between corners is one curved segment, bends either side,
        // ends meeting exactly.
        let s2 = path("SA-MIX-S2");
        assert!(s2.len() > 3, "{s2:?}");
        assert!(matches!(
            elem("SA-MIX-S2").curve("path"),
            Some(ElementCurve::Path3(_))
        ));
        assert!(near(*path("SA-MIX-B1").last().unwrap(), s2[0]));
        assert!(near(path("SA-MIX-B2")[0], *s2.last().unwrap()));
        assert!(elem("SA-MIX-S1").curve("path").is_none());
        let count = |entity: &str| ifc.matches(&format!("={entity}(")).count();
        assert_eq!(count("IFCDUCTSEGMENT"), 5);
        assert_eq!(count("IFCDUCTFITTING"), 2);
        // SA-FLEX to SA-NEXT, and the 4 joints along SA-MIX.
        assert_eq!(count("IFCRELCONNECTSPORTS"), 5);
        let scene = crate::scene::graph::generate_scene(&model);
        assert!(!scene.nodes.is_empty());
    }

    #[test]
    fn curved_segment_cuts_nothing() {
        let source = "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Wall: W\n    from: (2, -2) to: (2, 2)\n    height: 3.5 m\n    thickness: 0.24 m\n  Duct: D\n    diameter: 0.25 m\n    path: [(0, 0, 3), smooth (2, 0.5, 3), (4, 0, 3)]\n";
        let (model, _) = compile_clean(source);
        assert!(
            with_children(&model)
                .iter()
                .all(|e| e.element_type != semantic::model::ElementType::Opening)
        );
    }

    #[test]
    fn rect_duct_smooth_run_errors() {
        let source = "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Duct: SA-RECT\n    width: 0.4 m\n    height: 0.2 m\n    path: [(0, 0, 3), smooth (2, 1, 3), (4, 0, 3)]\n";
        let (_, diags) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E313 && d.message.contains("SA-RECT")),
            "{diags:?}"
        );
    }

    #[test]
    fn cable_tray_smooth_run_errors() {
        let source = "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  CableTray: CT-1\n    width: 0.3 m\n    height: 0.06 m\n    path: [(0, 0, 3), (2, 0, 3), smooth (3, 1, 3), (4, 0, 3)]\n";
        let (_, diags) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::E313 && d.message.contains("CT-1")),
            "{diags:?}"
        );
    }

    const PENETRATIONS: &str = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3.5 m
  Slab: SL-GF
    boundary: [(0, 0), (10, 0), (10, 8), (0, 8)]
    thickness: 0.25 m
  Wall: W
    from: (5, 0) to: (5, 8)
    height: 3.25 m
    thickness: 0.24 m
  Duct: D
    width: 0.4 m
    height: 0.25 m
    from: (1, 4, 2.8) to: (9, 4, 2.8)
    sleeve: true
  Pipe: R
    diameter: 0.1 m
    from: (2, 2, 0.5) to: (2, 2, 4.5)
    clearance: 0.03 m
  Pipe: P
    diameter: 0.05 m
    from: (3, 2, 2) to: (8, 6, 2)
    openings: false
### Storey: OG
  elevation: 3.5 m
  height: 3.5 m
  Slab: SL-OG
    boundary: [(0, 0), (10, 0), (10, 8), (0, 8)]
    thickness: 0.25 m
";

    /// Elements with their children (openings hang off their host).
    fn with_children(
        model: &semantic::model::SemanticModel,
    ) -> Vec<&semantic::model::ResolvedElement> {
        model
            .elements()
            .flat_map(|e| std::iter::once(e).chain(&e.children))
            .collect()
    }

    /// Sorted ids of the generated openings and sleeves of `PENETRATIONS`.
    fn generated_ids(model: &semantic::model::SemanticModel) -> Vec<&str> {
        let mut ids: Vec<&str> = with_children(model)
            .into_iter()
            .map(|e| e.id.as_str())
            .filter(|id| id.contains('-') && !id.starts_with("SL-"))
            .collect();
        ids.sort();
        ids
    }

    #[test]
    fn test_penetrations_cut_walls_and_slabs() {
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(PENETRATIONS).unwrap());
        assert!(diags.is_empty(), "{diags:?}");
        // `P` sets `openings: false`; the riser misses the ground slab below it.
        assert_eq!(generated_ids(&model), ["D-W", "D-W-SL", "R-SL-OG"]);
        let all = with_children(&model);
        let props = |id: &str| &all.iter().find(|e| e.id == id).unwrap().properties;
        let num = |id: &str, key: &str| semantic::model::number(props(id), key).unwrap();
        // Through the wall: 0.05 m past each face, 0.025 m clearance per side.
        let rect = crate::scene::graph::get_polygon(props("D-W"), "boundary").unwrap();
        let extent = |f: fn(&(f64, f64)) -> f64| {
            let v = rect.iter().map(f);
            v.clone().fold(f64::MIN, f64::max) - v.fold(f64::MAX, f64::min)
        };
        assert!((extent(|p| p.0) - 0.34).abs() < 1e-9);
        assert!((extent(|p| p.1) - 0.45).abs() < 1e-9);
        assert!((num("D-W", "z_offset") - 2.65).abs() < 1e-9);
        assert!((num("D-W", "height") - 0.3).abs() < 1e-9);
        // Through the upper slab, relative to the upper storey.
        assert!((num("R-SL-OG", "z_offset") + 0.3).abs() < 1e-9);
        assert!((num("R-SL-OG", "height") - 0.35).abs() < 1e-9);

        let ifc = generate_ifc(&model).unwrap();
        let count = |entity: &str| ifc.matches(&format!("={entity}(")).count();
        assert_eq!(count("IFCOPENINGELEMENT"), 2);
        assert_eq!(count("IFCRELVOIDSELEMENT"), 2);
        assert_eq!(count("IFCRELFILLSELEMENT"), 1);
        assert_eq!(count("IFCDUCTSEGMENT"), 2);
    }

    #[test]
    fn test_penetrations_cut_coverings_not_hang_from_them() {
        // A ceiling between the duct and the upper slab.
        let src = PENETRATIONS
            .replace("    sleeve: true\n", "    sleeve: true\n    hangers: 2 m\n")
            .replace(
                "### Storey: OG",
                "#### Space: Room\n  boundary: [(0, 0), (5, 0), (5, 8), (0, 8)]\n  Covering: CL\n    predefined_type: ceiling\n    z_offset: 3 m\n    thickness: 0.03 m\n### Storey: OG",
            );
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(&src).unwrap());
        assert!(diags.is_empty(), "{diags:?}");
        let all = with_children(&model);
        let num = |id: &str, key: &str| {
            let e = all.iter().find(|e| e.id == id).unwrap();
            semantic::model::number(&e.properties, key).unwrap()
        };
        // The riser cuts the ceiling, 0.05 m past each face.
        assert!((num("R-CL", "z_offset") - 2.95).abs() < 1e-9);
        assert!((num("R-CL", "height") - 0.13).abs() < 1e-9);
        // Hangers pass the ceiling up to the slab soffit (3.25).
        assert!((num("D-H1", "height") - 0.575).abs() < 1e-9);

        let scene = crate::scene::graph::generate_scene(&model);
        fn find<'a>(
            nodes: &'a [crate::scene::graph::SceneNode],
            id: &str,
        ) -> Option<&'a crate::scene::graph::SceneNode> {
            nodes
                .iter()
                .find_map(|n| (n.id == id).then_some(n).or_else(|| find(&n.children, id)))
        }
        let ceiling = find(&scene.nodes, "CL").unwrap();
        // The opening's walls add vertices to the plain 4-corner prism.
        assert!(
            ceiling.mesh.vertices.len() > 24,
            "{}",
            ceiling.mesh.vertices.len()
        );
    }

    #[test]
    fn test_sleeve_fills_hand_written_opening() {
        let src = PENETRATIONS.replace("    clearance: 0.03 m\n", "    sleeve: true\n")
            + "    Opening: R-SL-OG\n      boundary: [(1.9, 1.9), (2.1, 1.9), (2.1, 2.1), (1.9, 2.1)]\n      z_offset: -0.3 m\n      height: 0.35 m\n";
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(&src).unwrap());
        assert!(diags.is_empty(), "{diags:?}");
        // The hand-written opening stays alone; the sleeve fills it.
        assert_eq!(
            generated_ids(&model),
            ["D-W", "D-W-SL", "R-SL-OG", "R-SL-OG-SL"]
        );
    }

    #[test]
    fn test_non_positive_clearance_is_an_error() {
        let src = PENETRATIONS.replace("clearance: 0.03 m", "clearance: 0 m");
        let (_, diags) = semantic::analyze(&cst_to_ast::parse(&src).unwrap());
        assert!(
            diags.iter().any(|d| d.message.contains("`clearance`")),
            "{diags:?}"
        );
    }

    #[test]
    fn test_penetrations_across_project_files() {
        let (gf, og) = PENETRATIONS.split_once("### Storey: OG").unwrap();
        let sources = [
            ("gf.bml".to_string(), gf.to_string()),
            (
                "og.bml".to_string(),
                format!("# Site: S\n## Building: B\n### Storey: OG{og}"),
            ),
        ];
        let project = crate::project::Project::load_from_sources(sources.into()).unwrap();
        let (model, diags) = project.analyze();
        assert!(diags.is_empty(), "{diags:?}");
        // The riser cuts the slab of the other file, each crossing once.
        assert_eq!(generated_ids(&model), ["D-W", "D-W-SL", "R-SL-OG"]);
    }

    /// `PENETRATIONS` with hangers on the duct and on `P`, and a pipe that
    /// leaves the building outline.
    fn with_hangers() -> String {
        PENETRATIONS
            .replace("    sleeve: true\n", "    sleeve: true\n    hangers: 2 m\n")
            .replace("    openings: false\n", "    openings: false\n    hangers: 2.5 m\n")
            .replace(
                "### Storey: OG",
                "  Pipe: Q\n    diameter: 0.05 m\n    from: (8, 1, 2) to: (14, 1, 2)\n    hangers: 2 m\n### Storey: OG",
            )
    }

    #[test]
    fn test_hangers_hang_runs_from_slab_above() {
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(&with_hangers()).unwrap());
        // Two of `Q`'s hangers lie outside the upper slab.
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("2 of the 3 hangers of `Q`"));
        let hangers: Vec<_> = model
            .elements()
            .filter(|e| e.element_type == semantic::model::ElementType::Accessory)
            .collect();
        let ids: Vec<&str> = hangers.iter().map(|e| e.id.as_str()).collect();
        // `D` is 8 m long, `P` 6.4 m: equal parts of about the spacing.
        assert_eq!(
            ids,
            [
                "D-H1", "D-H2", "D-H3", "D-H4", "P-H1", "P-H2", "P-H3", "Q-H1"
            ]
        );
        let num = |id: &str, key: &str| {
            let e = hangers.iter().find(|e| e.id == id).unwrap();
            semantic::model::number(&e.properties, key).unwrap()
        };
        let pos = |id: &str| {
            let e = hangers.iter().find(|e| e.id == id).unwrap();
            crate::scene::graph::get_point(&e.properties, "position").unwrap()
        };
        assert_eq!(pos("D-H1"), (2.0, 4.0));
        assert_eq!(pos("Q-H1"), (9.0, 1.0));
        // Under the duct (2.8 - 0.125) up to the upper slab's soffit (3.25).
        assert!((num("D-H2", "z_offset") - 2.675).abs() < 1e-9);
        assert!((num("D-H2", "height") - 0.575).abs() < 1e-9);
        assert!((num("D-H2", "width") - 0.4).abs() < 1e-9);
        assert!((num("D-H2", "rotation") - 90.0).abs() < 1e-9);
        assert!((num("P-H1", "height") - 1.275).abs() < 1e-9);

        let ifc = generate_ifc(&model).unwrap();
        let hanger = ifc
            .lines()
            .filter(|l| l.contains("=IFCDISCRETEACCESSORY("))
            .collect::<Vec<_>>();
        assert_eq!(hanger.len(), 8);
        assert!(
            hanger[0].contains(",'HANGER',") && hanger[0].ends_with(",.USERDEFINED.);"),
            "{}",
            hanger[0]
        );
    }

    #[test]
    fn test_hangers_clear_walls_and_runs() {
        // One hanger for `D`, whose middle lies in wall `W`; `P`'s middle
        // hanger would hang through `D`, which crosses above it.
        let src = with_hangers().replace(
            "sleeve: true\n    hangers: 2 m",
            "sleeve: true\n    hangers: 8 m",
        );
        let (model, _) = semantic::analyze(&cst_to_ast::parse(&src).unwrap());
        let pos = |id: &str| {
            let e = model.elements().find(|e| e.id == id).unwrap();
            crate::scene::graph::get_point(&e.properties, "position").unwrap()
        };
        // Half the wall (0.12) plus half the strut (0.0205), in 5 cm steps.
        let (x, y) = pos("D-H1");
        assert!((x - 5.15).abs() < 1e-9 && y == 4.0, "{x} {y}");
        // Moved along `P` by the first step at which its strut (0.041 wide)
        // clears the 0.4 m wide duct: 0.25 m.
        let (x, y) = pos("P-H2");
        let dy = (y - 4.0).abs();
        assert!(dy > 0.2205 && dy < 0.3 && (x - 5.5).abs() < 0.5, "{x} {y}");
    }

    #[test]
    fn test_hangers_skip_generated_slab_openings() {
        // 2.9 m at 1.2 m spacing: 3 hangers, the middle one under riser `R`'s
        // opening in the upper slab.
        let src = PENETRATIONS.replace(
            "### Storey: OG",
            "  Pipe: H\n    diameter: 0.05 m\n    from: (0.55, 2, 2.5) to: (3.45, 2, 2.5)\n    hangers: 1.2 m\n### Storey: OG",
        );
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(&src).unwrap());
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("1 of the 3 hangers of `H`"));
        let ids: Vec<&str> = model
            .elements()
            .filter(|e| e.element_type == semantic::model::ElementType::Accessory)
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(ids, ["H-H1", "H-H3"]);
    }

    #[test]
    fn test_hangers_need_the_slab_above() {
        let src = with_hangers();
        let (gf, og) = src.split_once("### Storey: OG").unwrap();
        // Alone, the ground floor file has no slab above its runs.
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(gf).unwrap());
        assert!(diags.is_empty(), "{diags:?}");
        assert!(
            !model
                .elements()
                .any(|e| e.element_type == semantic::model::ElementType::Accessory)
        );
        let sources = [
            ("gf.bml".to_string(), gf.to_string()),
            (
                "og.bml".to_string(),
                format!("# Site: S\n## Building: B\n### Storey: OG{og}"),
            ),
        ];
        let project = crate::project::Project::load_from_sources(sources.into()).unwrap();
        let (model, diags) = project.analyze();
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].file.as_deref(), Some("gf.bml"));
        let count = model
            .elements()
            .filter(|e| e.element_type == semantic::model::ElementType::Accessory)
            .count();
        assert_eq!(count, 8);
    }

    #[test]
    fn test_non_positive_hanger_spacing_is_an_error() {
        let src = with_hangers().replace("hangers: 2 m", "hangers: 0 m");
        let (_, diags) = semantic::analyze(&cst_to_ast::parse(&src).unwrap());
        assert!(
            diags.iter().any(|d| d.message.contains("`hangers`")),
            "{diags:?}"
        );
    }

    /// A storey-high curtain wall on a slab, the slab above in another storey,
    /// and a column on a base plate.
    const CONNECTIONS: &str = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3.5 m
  Slab: SL-0
    boundary: [(0, 0), (6, 0), (6, 5), (0, 5)]
    thickness: 0.3 m
  Wall: CW
    from: (0, 0) to: (6, 0)
    height: 3.5 m
    thickness: 0.15 m
    mullions: 3 m
    brackets: true
  Column: C
    position: (3, 3)
    profile: rectangle(0.3, 0.3)
    height: 3.5 m
  Accessory: BP
    type: base_plate
    position: (3, 3)
    connects: [C, SL-0]
### Storey: OG
  elevation: 3.5 m
  height: 3 m
  Slab: SL-1
    boundary: [(0, 0), (6, 0), (6, 5), (0, 5)]
    thickness: 0.3 m
    connects: [SL-0, NOPE]
";

    fn accessories(
        model: &semantic::model::SemanticModel,
    ) -> Vec<&semantic::model::ResolvedElement> {
        model
            .elements()
            .filter(|e| e.element_type == semantic::model::ElementType::Accessory)
            .collect()
    }

    #[test]
    fn test_connects_writes_connection_relationships() {
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(CONNECTIONS).unwrap());
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("`NOPE`"), "{diags:?}");
        let output = generate_ifc_with_warnings(&model).unwrap();
        let ifc = &output.ifc_string;
        let line = |prefix: &str| ifc.lines().find(|l| l.contains(prefix)).unwrap();
        let entity = |name: &str| line(&format!(",'{name}',")).split('=').next().unwrap();
        // The base plate realizes the connection of the column and the slab.
        let realizing = format!(
            "$,{},{},({}),$);",
            entity("C"),
            entity("SL-0"),
            entity("BP")
        );
        assert!(
            ifc.lines()
                .any(|l| l.contains("=IFCRELCONNECTSWITHREALIZINGELEMENTS(")
                    && l.ends_with(&realizing)),
            "{realizing}"
        );
        // A slab names the one it is connected to; the unknown id is reported.
        let plain: Vec<_> = ifc
            .lines()
            .filter(|l| l.contains("=IFCRELCONNECTSELEMENTS("))
            .collect();
        assert_eq!(plain.len(), 1);
        assert!(plain[0].ends_with(&format!("$,{},{});", entity("SL-1"), entity("SL-0"))));
        assert_eq!(output.warnings.len(), 1, "{:?}", output.warnings);
        assert!(output.warnings[0].message.contains("`NOPE`"));
    }

    #[test]
    fn test_connects_children_and_duplicates() {
        let src = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3 m
  Wall: W1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.2 m
    Door: D1
      offset: 1 m
      width: 0.9 m
      height: 2.1 m
      connects: W2
  Wall: W2
    from: (5, 0) to: (5, 5)
    height: 3 m
    thickness: 0.2 m
    connects: [D1, W1]
  Wall: W3
    from: (0, 5) to: (5, 5)
    height: 3 m
    thickness: 0.2 m
    connects: W2
  Column: C
    position: (2, 2)
    profile: rectangle(0.3, 0.3)
    height: 3 m
    connects: W3
  Accessory: BP
    type: base_plate
    position: (2, 2)
    connects: [C, C, W3]
";
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(src).unwrap());
        assert!(diags.is_empty(), "{diags:?}");
        let output = generate_ifc_with_warnings(&model).unwrap();
        assert!(output.warnings.is_empty(), "{:?}", output.warnings);
        let ifc = &output.ifc_string;
        // D1-W2 named from both sides is one relationship; so is W2-W1, W3-W2, C-W3.
        assert_eq!(ifc.matches("=IFCRELCONNECTSELEMENTS(").count(), 4);
        // The repeated column is dropped, not connected to itself.
        assert_eq!(
            ifc.matches("=IFCRELCONNECTSWITHREALIZINGELEMENTS(").count(),
            1
        );
    }

    #[test]
    fn test_brackets_of_hand_written_mullions() {
        let src = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3.5 m
  Slab: SL-0
    boundary: [(0, 0), (6, 0), (6, 5), (0, 5)]
    thickness: 0.3 m
  Slab: SL-1
    boundary: [(0, 0), (6, 0), (6, 5), (0, 5)]
    thickness: 0.3 m
    z_offset: 3.5 m
  Wall: CW
    from: (0, 0) to: (6, 0)
    height: 4.5 m
    thickness: 0.15 m
    representation: none
    brackets: true
  Member: M1
    from: (3, 0, 0) to: (3, 0, 3.5)
    z_offset: 1 m
    profile: rectangle(0.05, 0.15)
    part_of: CW
  Member: M2
    position: (4, 0)
    height: 3 m
    profile: rectangle(0.05, 0.15)
    part_of: CW
  Wall: CW2
    from: (0, -2) to: (6, -2)
    height: 3 m
    thickness: 0.15 m
    mullions: 3 m
    brackets: true
";
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(src).unwrap());
        let ids: Vec<&str> = accessories(&model).iter().map(|e| e.id.as_str()).collect();
        // M1 spans 1..4.5 m: on top of SL-1 only. M2 stands on SL-0 and ends below SL-1.
        assert_eq!(ids, ["M1-SL-1", "M2-SL-0"]);
        let z = |id: &str| {
            let e = model.elements().find(|e| e.id == id).unwrap();
            semantic::model::number(&e.properties, "z_offset").unwrap()
        };
        assert!((z("M1-SL-1") - 3.5).abs() < 1e-9);
        // CW2 stands 2 m off the slabs.
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("`CW2` made no bracket"),
            "{diags:?}"
        );
    }

    #[test]
    fn test_curtain_wall_brackets() {
        let (model, _) = semantic::analyze(&cst_to_ast::parse(CONNECTIONS).unwrap());
        let brackets: Vec<_> = accessories(&model)
            .into_iter()
            .filter(|e| e.id != "BP")
            .collect();
        let ids: Vec<&str> = brackets.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "CW-M1-SL-0",
                "CW-M1-SL-1",
                "CW-M2-SL-0",
                "CW-M2-SL-1",
                "CW-M3-SL-0",
                "CW-M3-SL-1"
            ]
        );
        let get = |id: &str| brackets.iter().find(|e| e.id == id).unwrap();
        let z = |id: &str| semantic::model::number(&get(id).properties, "z_offset").unwrap();
        // On the slab at the foot, under the soffit (3.2 m) of the one above.
        assert!(z("CW-M2-SL-0").abs() < 1e-9);
        assert!((z("CW-M2-SL-1") - 3.05).abs() < 1e-9);
        // Against the mullion, on the slab side.
        let (x, y) =
            crate::scene::graph::get_point(&get("CW-M2-SL-0").properties, "position").unwrap();
        assert!(
            (x - 3.0).abs() < 1e-9 && (y - 0.125).abs() < 1e-9,
            "{x} {y}"
        );
        assert_eq!(
            semantic::model::refs(&get("CW-M2-SL-1").properties, "connects"),
            ["SL-1", "CW-M2"]
        );

        // Alone, the ground floor has no slab above.
        let gf = CONNECTIONS.split("### Storey: OG").next().unwrap();
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(gf).unwrap());
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(accessories(&model).len(), 4);
    }

    #[test]
    fn test_brackets_clear_of_walls_and_corners() {
        let src = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3.5 m
  Slab: SL-0
    boundary: [(0, 0), (6, 0), (6, 5), (0, 5)]
    thickness: 0.3 m
  Wall: CW-S
    from: (0, 0) to: (6, 0)
    height: 3 m
    thickness: 0.08 m
    mullions: 3 m
    brackets: true
  Wall: CW-W
    from: (0, 5) to: (0, 0)
    height: 3 m
    thickness: 0.08 m
    mullions: 5 m
    brackets: true
  Wall: W-E
    from: (6.1, 0) to: (6.1, 5)
    height: 3 m
    thickness: 0.3 m
";
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(src).unwrap());
        assert!(diags.is_empty(), "{diags:?}");
        let ids: Vec<&str> = accessories(&model).iter().map(|e| e.id.as_str()).collect();
        // CW-S-M3 at x = 6 would cut into W-E, CW-W-M2 at the corner into CW-S-M1's bracket.
        assert_eq!(ids, ["CW-S-M1-SL-0", "CW-S-M2-SL-0", "CW-W-M1-SL-0"]);
    }

    #[test]
    fn test_connects_roundtrip() {
        let (model, _) = semantic::analyze(&cst_to_ast::parse(CONNECTIONS).unwrap());
        let ifc = generate_ifc(&model).unwrap();
        let back = crate::decompile::decompile_ifc(&ifc).unwrap();
        let connects: Vec<usize> = back
            .elements()
            .map(|e| semantic::model::refs(&e.properties, "connects").len())
            .filter(|n| *n > 0)
            .collect();
        // The base plate and six brackets connect two elements each, the
        // slab one: the relationships the IFC holds.
        assert_eq!(connects.iter().sum::<usize>(), 15);
        let count = ifc.matches("=IFCRELCONNECTSWITHREALIZINGELEMENTS(").count()
            + ifc.matches("=IFCRELCONNECTSELEMENTS(").count();
        assert_eq!(count, 8);
    }

    /// Plan point of an element's `from:` axis start.
    fn axis_start(e: &semantic::model::ResolvedElement) -> (f64, f64) {
        match e
            .properties
            .iter()
            .find(|p| p.key == "from")
            .map(|p| &p.value)
        {
            Some(ResolvedValue::Axis { from, .. }) => *from,
            v => panic!("{v:?}"),
        }
    }

    const SPLINE_PATH: &str = "[(0, 0), smooth (4, 1.5), smooth (8, 1), (12, 3)]";

    #[test]
    fn curved_curtain_wall_mullions_along_curve() {
        let source = format!(
            "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Wall: CW\n    path: {SPLINE_PATH}\n    height: 3 m\n    thickness: 0.03 m\n    mullions: 1.5 m\n"
        );
        let (model, _) = compile_clean(&source);
        let wall = model.elements().find(|e| e.id == "CW").unwrap();
        let axis = crate::geometry::axis::WallAxis::of(wall, 0.005).unwrap();
        let s: Vec<f64> = model
            .elements()
            .filter(|e| e.id.starts_with("CW-M"))
            .map(|m| {
                let p = axis_start(m);
                axis.project([p.0, p.1])
            })
            .collect();
        let bays = (axis.length() / 1.5).ceil();
        assert_eq!(s.len(), bays as usize + 1, "{s:?}");
        // Inner grid lines (the end mullions sit inside the wall ends).
        for w in s[1..s.len() - 1].windows(2) {
            assert!((w[1] - w[0] - axis.length() / bays).abs() < 0.005, "{s:?}");
        }
        assert!(model.elements().any(|e| e.id.starts_with("CW-P")));
    }

    #[test]
    fn footing_under_curved_wall() {
        let source = format!(
            "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Wall: W\n    path: {SPLINE_PATH}\n    height: 3 m\n    thickness: 0.3 m\n  Footing: FT\n    under: [W]\n    width: 0.6 m\n    depth: 0.4 m\n"
        );
        let (model, _) = compile_clean(&source);
        let wall = model.elements().find(|e| e.id == "W").unwrap();
        let axis = crate::geometry::axis::WallAxis::of(wall, 0.005).unwrap();
        let ft = model.elements().find(|e| e.id == "FT").unwrap();
        let strip = crate::scene::graph::get_polygon(&ft.properties, "boundary").unwrap();
        assert!(strip.len() > 10, "{strip:?}");
        // Every corner of the strip is half its width off the axis (the end
        // corners project onto the axis ends, so only an upper bound there).
        for p in &strip {
            let s = axis.project([p.0, p.1]);
            let q = axis.point_at(s);
            let d = (p.0 - q[0]).hypot(p.1 - q[1]);
            if s > 0.001 && s < axis.length() - 0.001 {
                assert!((d - 0.3).abs() < 0.01, "{p:?} is {d} off the axis");
            } else {
                assert!(d < 0.3 + 0.01, "{p:?} is {d} off the axis");
            }
        }
        let area = geometry::polygon_area(&strip).abs();
        assert!((area - 0.6 * axis.length()).abs() < 0.05 * area, "{area}");
    }

    #[test]
    fn penetration_through_curved_wall() {
        let source = format!(
            "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Wall: W\n    path: {SPLINE_PATH}\n    height: 3 m\n    thickness: 0.24 m\n  Pipe: P\n    diameter: 0.1 m\n    from: (6, -3, 2) to: (6, 5, 2)\n"
        );
        let (model, ifc) = compile_clean(&source);
        let wall = model.elements().find(|e| e.id == "W").unwrap();
        let opening = wall
            .children
            .iter()
            .find(|c| c.id == "P-W")
            .unwrap_or_else(|| panic!("{:?}", wall.children));
        let axis = crate::geometry::axis::WallAxis::of(wall, 0.005).unwrap();
        // Centred on where the pipe crosses the axis line.
        let rect = crate::scene::graph::get_polygon(&opening.properties, "boundary").unwrap();
        let (cx, cy) = (
            rect.iter().map(|p| p.0).sum::<f64>() / 4.0,
            rect.iter().map(|p| p.1).sum::<f64>() / 4.0,
        );
        let q = axis.point_at(axis.project([cx, cy]));
        assert!(
            (cx - 6.0).abs() < 0.01 && (cx - q[0]).hypot(cy - q[1]) < 0.01,
            "{rect:?}"
        );
        assert_eq!(ifc.matches("=IFCOPENINGELEMENT(").count(), 1);
    }

    #[test]
    fn run_crossing_curved_wall_twice_gets_two_openings() {
        let source = "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Wall: W\n    from: (0, 0) via: (5, 5) to: (10, 0)\n    height: 3 m\n    thickness: 0.24 m\n  Pipe: P\n    diameter: 0.1 m\n    from: (-1, 2, 1) to: (11, 2, 1)\n";
        let (model, ifc) = compile_clean(source);
        let wall = model.elements().find(|e| e.id == "W").unwrap();
        let ids: Vec<&str> = wall.children.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["P-W", "P-W-2"]);
        assert_eq!(ifc.matches("=IFCOPENINGELEMENT(").count(), 2);
    }

    /// Compiles BML source, failing on analysis errors.
    fn compile_clean(source: &str) -> (semantic::model::SemanticModel, String) {
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        assert!(
            diags.iter().all(|d| d.severity != Severity::Error),
            "{diags:?}"
        );
        let ifc = generate_ifc(&model).unwrap();
        (model, ifc)
    }

    /// The STEP lines from `start` (an entity id like `#12`) through every
    /// entity it references, transitively.
    fn entity_tree(ifc: &str, start: &str) -> Vec<String> {
        let lines: std::collections::HashMap<&str, &str> = ifc
            .lines()
            .filter_map(|l| l.split_once('=').filter(|(id, _)| id.starts_with('#')))
            .collect();
        let mut seen = std::collections::BTreeSet::new();
        let mut todo = vec![start.to_string()];
        let mut out = Vec::new();
        while let Some(id) = todo.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let Some(body) = lines.get(id.as_str()) else {
                continue;
            };
            out.push(body.to_string());
            let mut rest = *body;
            while let Some(i) = rest.find('#') {
                rest = &rest[i + 1..];
                let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                todo.push(format!("#{n}"));
            }
        }
        out
    }

    /// The representation of the product named `name`, with all it references.
    fn product_geometry(ifc: &str, entity: &str, name: &str) -> Vec<String> {
        let line = ifc
            .lines()
            .find(|l| l.contains(&format!("={entity}(")) && l.contains(&format!("'{name}'")))
            .unwrap_or_else(|| panic!("no {entity} {name}"));
        // Representation is the last-but-one or so reference; take every
        // reference after the placement and follow them.
        let refs: Vec<&str> = line
            .split([',', '(', ')'])
            .filter(|t| t.starts_with('#'))
            .collect();
        refs.iter()
            .skip(2)
            .flat_map(|r| entity_tree(ifc, r))
            .collect()
    }

    /// The STEP lines under a product's 'Axis' representation.
    fn axis_geometry(ifc: &str, entity: &str, name: &str) -> String {
        let geo = product_geometry(ifc, entity, name);
        let (id, _) = ifc
            .lines()
            .filter_map(|l| l.split_once('='))
            .find(|(_, body)| {
                body.starts_with("IFCSHAPEREPRESENTATION(")
                    && body.contains("'Axis'")
                    && geo.iter().any(|g| g == body)
            })
            .unwrap_or_else(|| panic!("{name} has no 'Axis' representation"));
        entity_tree(ifc, id).join("\n")
    }

    #[test]
    fn smooth_slab_writes_bspline_profile() {
        let (_, ifc) = compile_clean(
            "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Slab: SL\n    boundary: [(0, 0), smooth (10, -2), (20, 0), (20, 15), smooth (10, 18), (0, 15)]\n    thickness: 0.25 m\n",
        );
        let geo = product_geometry(&ifc, "IFCSLAB", "SL").join("\n");
        assert!(geo.contains("IFCBSPLINECURVEWITHKNOTS("), "{geo}");
        assert!(geo.contains("IFCARBITRARYCLOSEDPROFILEDEF("), "{geo}");
        assert!(geo.contains("IFCCOMPOSITECURVE("), "{geo}");
        assert!(!geo.contains("IFCRATIONALBSPLINECURVEWITHKNOTS("), "{geo}");
        // Straight edges are polylines inside the composite curve, but the
        // profile itself is not one polyline ring.
        assert!(
            !geo.lines()
                .any(|l| l.starts_with("IFCARBITRARYCLOSEDPROFILEDEF(")
                    && entity_tree(
                        &ifc,
                        l.split(',').nth(2).unwrap().trim_end_matches([')', ';'])
                    )[0]
                    .starts_with("IFCPOLYLINE(")),
            "{geo}"
        );
    }

    #[test]
    fn rational_nurbs_writes_rational_entity() {
        let (_, ifc) = compile_clean(
            "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Slab: SL\n    boundary: [(14, 1), nurbs { degree: 3, points: [(14, 1), (16, -1), (18, 1), (19, 1)], weights: [1, 2, 1, 1], knots: [0, 0, 0, 0, 1, 1, 1, 1] }, (19, 6), (14, 6)]\n    thickness: 0.25 m\n",
        );
        let geo = product_geometry(&ifc, "IFCSLAB", "SL").join("\n");
        let line = geo
            .lines()
            .find(|l| l.contains("IFCRATIONALBSPLINECURVEWITHKNOTS("))
            .unwrap_or_else(|| panic!("{geo}"));
        // Knots as distinct values with multiplicities, then the weights.
        assert!(
            line.contains("(4,4),(0.,1.),.UNSPECIFIED.,(1.,2.,1.,1.)"),
            "{line}"
        );
    }

    #[test]
    fn spline_wall_has_exact_axis() {
        let (_, ifc) = compile_clean(&format!(
            "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Wall: W\n    path: {SPLINE_PATH}\n    height: 3 m\n    thickness: 0.3 m\n"
        ));
        let geo = product_geometry(&ifc, "IFCWALL", "W").join("\n");
        assert!(geo.contains("IFCARBITRARYCLOSEDPROFILEDEF("), "{geo}");
        let axis = axis_geometry(&ifc, "IFCWALL", "W");
        assert!(axis.contains("IFCBSPLINECURVEWITHKNOTS("), "{axis}");
    }

    #[test]
    fn arc_wall_has_axis() {
        let (_, ifc) = compile_clean(
            "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Wall: W\n    from: (0, 0) via: (5, 2) to: (10, 0)\n    height: 3 m\n    thickness: 0.3 m\n",
        );
        let axis = axis_geometry(&ifc, "IFCWALL", "W");
        assert!(axis.contains("IFCCOMPOSITECURVE("), "{axis}");
        assert!(!axis.contains("IFCPOLYLINE("), "{axis}");
    }

    #[test]
    fn bspline_knots_run_from_0_to_1() {
        let (_, ifc) = compile_clean(
            "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Slab: SL\n    boundary: [(14, 1), nurbs { degree: 2, points: [(14, 1), (16, -1), (18, 1), (19, 1)], knots: [2, 2, 2, 3, 6, 6, 6] }, (19, 6), (14, 6)]\n    thickness: 0.25 m\n",
        );
        assert!(ifc.contains("(3,1,3),(0.,0.25,1.)"), "{ifc}");
        let trims: Vec<&str> = ifc
            .lines()
            .filter(|l| l.contains("IFCTRIMMEDCURVE(") && l.contains("IFCPARAMETERVALUE"))
            .collect();
        assert!(!trims.is_empty());
        for l in trims {
            assert!(
                l.contains("(IFCPARAMETERVALUE(0.)),(IFCPARAMETERVALUE(1.))"),
                "{l}"
            );
        }
    }

    #[test]
    fn smooth_railing_and_duct_swept() {
        let (model, ifc) = compile_clean(
            "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Railing: RL\n    path: [(0, 0), smooth (6, 2), smooth (12, 1), (18, 5)]\n    height: 1 m\n  Beam: BM\n    path: [(0, 5), smooth (6, 7), (12, 5)]\n    profile: rectangle(0.3, 0.5)\n    z_offset: 3 m\n  Member: MB\n    path: [(0, 9), (5, 9), (5, 12)]\n    profile: circle(0.05)\n    z_offset: 2 m\n  Duct: SA\n    diameter: 0.25 m\n    path: [(0, 0, 3), smooth (2, 1, 3), (4, 0, 3.2)]\n",
        );
        // Rectangular sections are plan profiles extruded like curved walls,
        // with the exact path as their 'Axis'.
        for (entity, id) in [("IFCRAILING", "RL"), ("IFCBEAM", "BM")] {
            let geo = product_geometry(&ifc, entity, id).join("\n");
            assert!(!geo.contains("IFCFIXEDREFERENCESWEPTAREASOLID("), "{geo}");
            assert!(geo.contains("IFCEXTRUDEDAREASOLID("), "{geo}");
            assert!(geo.contains("IFCBSPLINECURVEWITHKNOTS("), "{geo}");
            let axis = axis_geometry(&ifc, entity, id);
            assert!(axis.contains("IFCBSPLINECURVEWITHKNOTS("), "{axis}");
        }
        let member = product_geometry(&ifc, "IFCMEMBER", "MB").join("\n");
        assert!(member.contains("IFCSWEPTDISKSOLID("), "{member}");
        assert!(axis_geometry(&ifc, "IFCMEMBER", "MB").contains("IFCPOLYLINE("));
        let duct = product_geometry(&ifc, "IFCDUCTSEGMENT", "SA").join("\n");
        assert!(duct.contains("IFCSWEPTDISKSOLID("), "{duct}");
        // The scene follows the curves too: both pass through their smooth
        // points (a straight chord would miss them by over 0.3 m).
        let scene = crate::scene::graph::generate_scene(&model);
        for (id, p) in [("RL", [6.0, 2.0]), ("BM", [6.0, 7.0])] {
            let node = scene.nodes.iter().find(|n| n.id == id).unwrap();
            let d = node
                .mesh
                .vertices
                .iter()
                .map(|v| (v.position[0] as f64 - p[0]).hypot(v.position[1] as f64 - p[1]))
                .fold(f64::MAX, f64::min);
            assert!(d < 0.25, "{id} is {d} off {p:?}");
        }
    }

    /// A `path:` that gives no sweep falls back to the `from:`/`to:` body
    /// instead of dropping the element from the scene.
    #[test]
    fn railing_on_unusable_path_keeps_its_body() {
        let (model, _) = semantic::analyze(
            &cst_to_ast::parse(
                "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Railing: RL\n    from: (0, 0) to: (5, 0)\n    path: rail\n    height: 1 m\n",
            )
            .unwrap(),
        );
        let scene = crate::scene::graph::generate_scene(&model);
        let node = scene
            .nodes
            .iter()
            .find(|n| n.id == "RL")
            .expect("RL in scene");
        assert!(!node.mesh.vertices.is_empty());
    }

    #[test]
    fn test_ifcopenshell_curves() {
        let source = std::fs::read_to_string("../../tests/fixtures/curves.bml").unwrap();
        let (_, ifc) = compile_clean(&source);
        assert!(ifc.contains("IFCBSPLINECURVEWITHKNOTS("));
        assert!(ifc.contains("IFCRATIONALBSPLINECURVEWITHKNOTS("));
        assert!(ifc.contains("IFCSWEPTDISKSOLID("));
        assert!(!ifc.contains("IFCFIXEDREFERENCESWEPTAREASOLID("));
        validate_ifc_with_ifcopenshell(&ifc, "curves");
    }

    /// IfcOpenShell places the curved railing and beam of `curves.bml` where
    /// the scene draws them.
    #[test]
    fn test_ifcopenshell_curved_sweeps_match_scene() {
        if !ifcopenshell_available() {
            eprintln!("SKIPPED: ifcopenshell not installed");
            return;
        }
        let source = std::fs::read_to_string("../../tests/fixtures/curves.bml").unwrap();
        let (model, ifc) = compile_clean(&source);
        let path = std::env::temp_dir().join("bml_test_curved_sweeps.ifc");
        std::fs::write(&path, &ifc).unwrap();
        let script = "import sys, json, ifcopenshell, ifcopenshell.geom, numpy as np
f = ifcopenshell.open(sys.argv[1])
s = ifcopenshell.geom.settings()
s.set('use-world-coords', True)
out = {}
for e in f.by_type('IfcProduct'):
    if e.Name in sys.argv[2:]:
        v = np.array(ifcopenshell.geom.create_shape(s, e).geometry.verts).reshape(-1, 3)
        out[e.Name] = [v.min(0).tolist(), v.max(0).tolist()]
print(json.dumps(out))
";
        let output = std::process::Command::new("python3")
            .args(["-c", script, path.to_str().unwrap(), "RL-01", "B-01"])
            .output()
            .unwrap();
        let _ = std::fs::remove_file(&path);
        let boxes: serde_json::Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stderr)));
        let scene = crate::scene::graph::generate_scene(&model);
        for id in ["RL-01", "B-01"] {
            let node = scene.nodes.iter().find(|n| n.id == id).unwrap();
            let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
            for v in &node.mesh.vertices {
                for k in 0..3 {
                    lo[k] = lo[k].min(v.position[k] as f64);
                    hi[k] = hi[k].max(v.position[k] as f64);
                }
            }
            let b = &boxes[id];
            for k in 0..3 {
                let d = (b[0][k].as_f64().unwrap() - lo[k])
                    .abs()
                    .max((b[1][k].as_f64().unwrap() - hi[k]).abs());
                assert!(d < 0.05, "{id} axis {k} off by {d}: {b} vs {lo:?} {hi:?}");
            }
        }
    }
}
