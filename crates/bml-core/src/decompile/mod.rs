pub mod bml_writer;
pub mod ifc_analyzer;
mod ifc_curves;
pub mod ifc_reader;
pub mod mesh_fit;
pub mod styles;

use crate::semantic::model::SemanticModel;
use ifc_reader::IfcReadError;

/// Decompile an IFC STEP file to a SemanticModel.
pub fn decompile_ifc(ifc_content: &str) -> Result<SemanticModel, IfcReadError> {
    let model = ifc_reader::read_ifc(ifc_content)?;
    Ok(model)
}

/// Decompile an IFC STEP file to BML source text.
pub fn decompile_to_bml(ifc_content: &str) -> Result<String, IfcReadError> {
    let mut model = decompile_ifc(ifc_content)?;
    mesh_fit::drop_all_incomplete(&mut model);
    Ok(bml_writer::generate_bml(&model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen;
    use crate::parser::cst_to_ast;
    use crate::semantic;

    #[test]
    fn test_decompile_roundtrip() {
        // Compile BML -> IFC -> decompile to SemanticModel
        let source = std::fs::read_to_string("../../tests/fixtures/wall_only.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);

        let ifc_string = codegen::generate_ifc(&model).unwrap();

        // Decompile IFC back to a model
        let decompiled_model = decompile_ifc(&ifc_string).unwrap();

        // Verify the decompiled model has the expected structure
        assert_eq!(decompiled_model.sites.len(), 1);
        assert_eq!(decompiled_model.sites[0].buildings.len(), 1);
        assert!(!decompiled_model.sites[0].buildings[0].storeys.is_empty());
    }

    #[test]
    fn test_decompile_to_bml_output() {
        let source = std::fs::read_to_string("../../tests/fixtures/wall_only.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);

        let ifc_string = codegen::generate_ifc(&model).unwrap();

        // Decompile to BML text
        let bml_output = decompile_to_bml(&ifc_string).unwrap();

        // Verify the BML output contains expected structure
        assert!(bml_output.contains("# Site:"), "should have site heading");
        assert!(
            bml_output.contains("## Building:"),
            "should have building heading"
        );
        assert!(
            bml_output.contains("### Storey:"),
            "should have storey heading"
        );
        assert!(
            bml_output.contains("elevation:"),
            "should have elevation property"
        );
    }

    #[test]
    fn test_decompile_preserves_names() {
        let source = std::fs::read_to_string("../../tests/fixtures/spatial_hierarchy.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);

        let ifc_string = codegen::generate_ifc(&model).unwrap();
        let decompiled_model = decompile_ifc(&ifc_string).unwrap();

        // Site and building names should be preserved from IFC
        assert!(!decompiled_model.sites[0].name.is_empty());
        assert!(!decompiled_model.sites[0].buildings[0].name.is_empty());
    }

    #[test]
    fn test_bml_writer_generates_valid_output() {
        // Create a model programmatically and write it
        use crate::ast::span::{Position, Span};

        let dummy_span = Span::new(
            Position {
                line: 1,
                column: 1,
                byte_offset: 0,
            },
            Position {
                line: 1,
                column: 1,
                byte_offset: 0,
            },
        );

        let model = SemanticModel {
            asset_libraries: Vec::new(),
            sites: vec![crate::semantic::model::ResolvedSite {
                name: "TestSite".to_string(),
                implicit: false,
                properties: Vec::new(),
                spaces: Vec::new(),
                elements: Vec::new(),
                buildings: vec![crate::semantic::model::ResolvedBuilding {
                    name: "TestBuilding".to_string(),
                    properties: Vec::new(),
                    spaces: Vec::new(),
                    elements: Vec::new(),
                    storeys: vec![crate::semantic::model::ResolvedStorey {
                        name: "Ground Floor".to_string(),
                        properties: vec![crate::semantic::model::ResolvedProperty {
                            key: "elevation".to_string(),
                            value: crate::semantic::model::ResolvedValue::Number(0.0),
                            span: dummy_span,
                        }],
                        spaces: Vec::new(),
                        elements: vec![crate::semantic::model::ResolvedElement {
                            element_type: crate::semantic::model::ElementType::Wall,
                            id: "W-01".to_string(),
                            properties: vec![
                                crate::semantic::model::ResolvedProperty {
                                    key: "from".to_string(),
                                    value: crate::semantic::model::ResolvedValue::Axis {
                                        from: (0.0, 0.0),
                                        to: (6.0, 0.0),
                                        via: None,
                                        z: None,
                                    },
                                    span: dummy_span,
                                },
                                crate::semantic::model::ResolvedProperty {
                                    key: "height".to_string(),
                                    value: crate::semantic::model::ResolvedValue::Number(2.8),
                                    span: dummy_span,
                                },
                            ],
                            children: Vec::new(),
                            span: dummy_span,
                            curves: Default::default(),
                        }],
                        span: dummy_span,
                    }],
                    span: dummy_span,
                }],
                span: dummy_span,
            }],
            materials: Vec::new(),
        };

        let bml = bml_writer::generate_bml(&model);
        assert!(bml.contains("# Site: TestSite"));
        assert!(bml.contains("## Building: TestBuilding"));
        assert!(bml.contains("### Storey: Ground Floor"));
        assert!(bml.contains("Wall: W-01"));
        assert!(bml.contains("height:"));
    }

    #[test]
    fn test_extract_numeric_field() {
        use super::ifc_reader::extract_numeric_field;
        let entity = "IFCWINDOW('guid',#18,'name',$,'type',#100,#200,'tag',1350.0,1550.0,.WINDOW.,.NOTDEFINED.,$)";
        assert!((extract_numeric_field(entity, 8).unwrap() - 1350.0).abs() < 0.001);
        assert!((extract_numeric_field(entity, 9).unwrap() - 1550.0).abs() < 0.001);
        assert!(extract_numeric_field(entity, 2).is_none()); // string field
        assert!(extract_numeric_field(entity, 99).is_none()); // out of bounds
    }

    #[test]
    fn test_decompile_extracts_boolean_properties() {
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let ifc = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&ifc).unwrap();
        let all_elements: Vec<_> = model
            .sites
            .iter()
            .flat_map(|s| &s.buildings)
            .flat_map(|b| &b.storeys)
            .flat_map(|st| &st.elements)
            .collect();
        let wall = all_elements
            .iter()
            .find(|e| e.element_type == crate::semantic::model::ElementType::Wall)
            .unwrap();
        let has_is_external = wall.properties.iter().any(|p| p.key == "is_external");
        let has_load_bearing = wall.properties.iter().any(|p| p.key == "load_bearing");
        assert!(
            has_is_external || has_load_bearing,
            "Expected boolean properties to be extracted"
        );
    }

    #[test]
    fn test_decompile_no_duplicate_materials() {
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let ifc = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&ifc).unwrap();
        let mut ids: Vec<&str> = model.materials.iter().map(|m| m.id.as_str()).collect();
        let len_before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), len_before, "Found duplicate material IDs");
    }

    // ─── Real-world IFC regression tests ─────────────────────────────────

    #[test]
    fn test_decompile_model_1_ifc() {
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let content = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&content).unwrap();

        // Spatial hierarchy
        assert_eq!(model.sites.len(), 1);
        assert_eq!(model.sites[0].name, "Default");
        assert_eq!(model.sites[0].buildings.len(), 1);
        assert_eq!(model.sites[0].buildings[0].name, "Building Name");

        let storeys = &model.sites[0].buildings[0].storeys;
        assert_eq!(storeys.len(), 6);
        assert_eq!(storeys[0].name, "GROUND FLOOR");
        assert_eq!(storeys[1].name, "FIRST FLOOR");

        // Ground floor should have spaces
        assert!(!storeys[0].spaces.is_empty(), "should have spaces");

        // Check total element count across all storeys
        let total_elements: usize = storeys.iter().map(|s| count_elements(s)).sum();
        assert!(
            total_elements > 100,
            "should have many elements, got {}",
            total_elements
        );

        // Check that walls have children (doors/windows)
        let walls_with_children: usize = storeys
            .iter()
            .flat_map(|s| &s.elements)
            .filter(|e| {
                e.element_type == crate::semantic::model::ElementType::Wall
                    && !e.children.is_empty()
            })
            .count();
        assert!(
            walls_with_children > 0,
            "some walls should have doors/windows as children"
        );
    }

    #[test]
    fn test_decompile_model_2_ifc() {
        let path = "../../tests/fixtures/model_2.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let content = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&content).unwrap();

        assert_eq!(model.sites.len(), 1);
        let storeys = &model.sites[0].buildings[0].storeys;
        assert_eq!(storeys.len(), 6);

        // model_2 uses centimetres → elevations should be in metres
        // Level1 elevation = 0, Level2 = 430cm = 4.3m
        let elev_prop = storeys[1]
            .properties
            .iter()
            .find(|p| p.key == "elevation")
            .unwrap();
        if let crate::semantic::model::ResolvedValue::Number(e) = elev_prop.value {
            assert!(
                (e - 4.3).abs() < 0.1,
                "Level2 elevation should be ~4.3m, got {}",
                e
            );
        }

        // Check spaces
        let total_spaces: usize = storeys.iter().map(|s| s.spaces.len()).sum();
        assert!(total_spaces > 0, "should have spaces, got {}", total_spaces);
    }

    #[test]
    fn test_decompile_model_3_ifc() {
        let path = "../../tests/fixtures/model_3.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let content = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&content).unwrap();

        assert_eq!(model.sites.len(), 1);
        let storeys = &model.sites[0].buildings[0].storeys;
        assert_eq!(storeys.len(), 6);

        // model_3 uses metres, level 2 elevation = 3.8
        let elev_prop = storeys[1]
            .properties
            .iter()
            .find(|p| p.key == "elevation")
            .unwrap();
        if let crate::semantic::model::ResolvedValue::Number(e) = elev_prop.value {
            assert!(
                (e - 3.8).abs() < 0.1,
                "level 2 elevation should be ~3.8m, got {}",
                e
            );
        }

        let total_elements: usize = storeys.iter().map(|s| count_elements(s)).sum();
        assert!(
            total_elements > 50,
            "should have elements, got {}",
            total_elements
        );
    }

    #[test]
    fn test_decompile_model_1_to_bml_roundtrip() {
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let content = std::fs::read_to_string(path).unwrap();
        let bml = decompile_to_bml(&content).unwrap();

        // BML output should contain expected structure
        assert!(bml.contains("# Site: Default"));
        assert!(bml.contains("## Building: Building Name"));
        assert!(bml.contains("### Storey: GROUND FLOOR"));
        assert!(bml.contains("### Storey: FIRST FLOOR"));
        assert!(bml.contains("#### Space:"));
        assert!(bml.contains("Wall:"));
        assert!(bml.contains("Window:"));
        assert!(bml.contains("Door:"));
        assert!(bml.contains("Beam:"));
        assert!(bml.contains("Column:"));
    }

    #[test]
    fn test_decompile_element_types_model_1() {
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let content = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&content).unwrap();

        // Collect all element types found
        let mut types_found = std::collections::HashSet::new();
        for storey in &model.sites[0].buildings[0].storeys {
            for elem in &storey.elements {
                types_found.insert(elem.element_type.as_str().to_string());
                for child in &elem.children {
                    types_found.insert(child.element_type.as_str().to_string());
                }
            }
        }

        // model_1 should have all these element types
        assert!(types_found.contains("Wall"), "should contain Wall");
        assert!(types_found.contains("Slab"), "should contain Slab");
        assert!(types_found.contains("Beam"), "should contain Beam");
        assert!(types_found.contains("Column"), "should contain Column");
        assert!(types_found.contains("Window"), "should contain Window");
        assert!(types_found.contains("Door"), "should contain Door");
        assert!(
            types_found.contains("Furniture"),
            "should contain Furniture"
        );
        assert!(types_found.contains("Stair"), "should contain Stair");
        assert!(types_found.contains("Railing"), "should contain Railing");
    }

    #[test]
    fn test_decompile_spatial_containment() {
        // Verify elements are distributed across storeys (not all on first storey)
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let content = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&content).unwrap();

        let storeys = &model.sites[0].buildings[0].storeys;
        let storeys_with_elements: usize =
            storeys.iter().filter(|s| !s.elements.is_empty()).count();

        assert!(
            storeys_with_elements > 1,
            "elements should be distributed across multiple storeys, not just the first; found elements on {} storeys",
            storeys_with_elements
        );
    }

    #[test]
    fn test_decompile_properties_extracted() {
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let content = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&content).unwrap();

        // At least some elements should have properties from Pset_*
        let elements_with_props: usize = model.sites[0].buildings[0]
            .storeys
            .iter()
            .flat_map(|s| &s.elements)
            .filter(|e| !e.properties.is_empty())
            .count();

        assert!(
            elements_with_props > 0,
            "some elements should have properties from Pset_*"
        );
    }

    #[test]
    fn test_decompile_window_door_dimensions() {
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let ifc = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&ifc).unwrap();
        let all_children: Vec<_> = model
            .sites
            .iter()
            .flat_map(|s| &s.buildings)
            .flat_map(|b| &b.storeys)
            .flat_map(|st| st.elements.iter().flat_map(|e| &e.children))
            .collect();

        let window = all_children
            .iter()
            .find(|e| e.element_type == crate::semantic::model::ElementType::Window)
            .unwrap();
        assert!(
            window.properties.iter().any(|p| p.key == "width"),
            "Window should have width"
        );
        assert!(
            window.properties.iter().any(|p| p.key == "height"),
            "Window should have height"
        );

        let door = all_children
            .iter()
            .find(|e| e.element_type == crate::semantic::model::ElementType::Door)
            .unwrap();
        assert!(
            door.properties.iter().any(|p| p.key == "width"),
            "Door should have width"
        );
        assert!(
            door.properties.iter().any(|p| p.key == "height"),
            "Door should have height"
        );
    }

    #[test]
    fn test_decompile_wall_geometry() {
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let ifc = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&ifc).unwrap();
        let all_elements: Vec<_> = model
            .sites
            .iter()
            .flat_map(|s| &s.buildings)
            .flat_map(|b| &b.storeys)
            .flat_map(|st| &st.elements)
            .collect();
        let wall = all_elements
            .iter()
            .find(|e| e.element_type == crate::semantic::model::ElementType::Wall)
            .unwrap();
        let has_from = wall.properties.iter().any(|p| p.key == "from");
        let has_height = wall.properties.iter().any(|p| p.key == "height");
        let has_thickness = wall.properties.iter().any(|p| p.key == "thickness");
        assert!(has_from, "Wall should have from (axis) property");
        assert!(has_height, "Wall should have height property");
        assert!(has_thickness, "Wall should have thickness property");
    }

    #[test]
    fn test_decompile_wall_geometry_model_3() {
        // model_3 uses metres and IFCPOLYLINE + IFCRECTANGLEPROFILEDEF
        let path = "../../tests/fixtures/model_3.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let ifc = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&ifc).unwrap();
        let all_elements: Vec<_> = model
            .sites
            .iter()
            .flat_map(|s| &s.buildings)
            .flat_map(|b| &b.storeys)
            .flat_map(|st| &st.elements)
            .collect();
        let wall = all_elements
            .iter()
            .find(|e| e.element_type == crate::semantic::model::ElementType::Wall)
            .unwrap();
        let has_from = wall.properties.iter().any(|p| p.key == "from");
        let has_height = wall.properties.iter().any(|p| p.key == "height");
        let has_thickness = wall.properties.iter().any(|p| p.key == "thickness");
        assert!(has_from, "Wall should have from (axis) property");
        assert!(has_height, "Wall should have height property");
        assert!(has_thickness, "Wall should have thickness property");

        // Verify reasonable values (model_3 uses metres)
        if let Some(from_prop) = wall.properties.iter().find(|p| p.key == "from") {
            if let crate::semantic::model::ResolvedValue::Axis { from, to, .. } = &from_prop.value {
                // Wall axis should have some length
                let dx = to.0 - from.0;
                let dy = to.1 - from.1;
                let length = (dx * dx + dy * dy).sqrt();
                assert!(
                    length > 0.1,
                    "Wall axis length should be > 0.1m, got {}",
                    length
                );
            }
        }
    }

    #[test]
    fn test_decompile_column_geometry() {
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let ifc = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&ifc).unwrap();
        let all_elements: Vec<_> = model
            .sites
            .iter()
            .flat_map(|s| &s.buildings)
            .flat_map(|b| &b.storeys)
            .flat_map(|st| &st.elements)
            .collect();
        let col = all_elements
            .iter()
            .find(|e| e.element_type == crate::semantic::model::ElementType::Column)
            .expect("Should have at least one column");
        assert!(
            col.properties.iter().any(|p| p.key == "position"),
            "Column should have position"
        );
        assert!(
            col.properties.iter().any(|p| p.key == "profile"),
            "Column should have profile"
        );
        assert!(
            col.properties.iter().any(|p| p.key == "height"),
            "Column should have height"
        );

        // Verify profile is a Profile variant with reasonable values
        if let Some(prof) = col.properties.iter().find(|p| p.key == "profile") {
            if let crate::semantic::model::ResolvedValue::Profile {
                profile_type,
                params,
            } = &prof.value
            {
                assert_eq!(
                    profile_type, "rectangle",
                    "Column profile should be rectangle"
                );
                assert_eq!(params.len(), 2, "Rectangle profile should have 2 params");
                assert!(
                    params[0] > 0.01 && params[0] < 10.0,
                    "Column width should be reasonable, got {}",
                    params[0]
                );
            } else {
                panic!("Column profile should be a Profile variant");
            }
        }
    }

    #[test]
    fn test_decompile_beam_geometry() {
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let ifc = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&ifc).unwrap();
        let all_elements: Vec<_> = model
            .sites
            .iter()
            .flat_map(|s| &s.buildings)
            .flat_map(|b| &b.storeys)
            .flat_map(|st| &st.elements)
            .collect();
        let beam = all_elements
            .iter()
            .find(|e| e.element_type == crate::semantic::model::ElementType::Beam)
            .expect("Should have at least one beam");
        assert!(
            beam.properties.iter().any(|p| p.key == "from"),
            "Beam should have from (axis)"
        );
        assert!(
            beam.properties.iter().any(|p| p.key == "profile"),
            "Beam should have profile"
        );

        // Verify profile is a Profile variant
        if let Some(prof) = beam.properties.iter().find(|p| p.key == "profile") {
            if let crate::semantic::model::ResolvedValue::Profile {
                profile_type,
                params,
            } = &prof.value
            {
                assert_eq!(
                    profile_type, "rectangle",
                    "Beam profile should be rectangle"
                );
                assert_eq!(params.len(), 2, "Rectangle profile should have 2 params");
            } else {
                panic!("Beam profile should be a Profile variant");
            }
        }
    }

    #[test]
    fn test_decompile_window_offset_sill() {
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let ifc = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&ifc).unwrap();
        let all_children: Vec<_> = model
            .sites
            .iter()
            .flat_map(|s| &s.buildings)
            .flat_map(|b| &b.storeys)
            .flat_map(|st| st.elements.iter().flat_map(|e| &e.children))
            .collect();

        let window = all_children
            .iter()
            .find(|e| e.element_type == crate::semantic::model::ElementType::Window)
            .unwrap();
        assert!(
            window.properties.iter().any(|p| p.key == "offset"),
            "Window should have offset"
        );
        assert!(
            window.properties.iter().any(|p| p.key == "sill_height"),
            "Window should have sill_height"
        );
    }

    #[test]
    fn test_decompile_door_offset() {
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let ifc = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&ifc).unwrap();
        let all_children: Vec<_> = model
            .sites
            .iter()
            .flat_map(|s| &s.buildings)
            .flat_map(|b| &b.storeys)
            .flat_map(|st| st.elements.iter().flat_map(|e| &e.children))
            .collect();

        let door = all_children
            .iter()
            .find(|e| e.element_type == crate::semantic::model::ElementType::Door)
            .unwrap();
        assert!(
            door.properties.iter().any(|p| p.key == "offset"),
            "Door should have offset"
        );
        // Doors should NOT have sill_height
        assert!(
            !door.properties.iter().any(|p| p.key == "sill_height"),
            "Door should not have sill_height"
        );
    }

    #[test]
    fn test_decompile_slab_geometry() {
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let ifc = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&ifc).unwrap();
        let all_elements: Vec<_> = model
            .sites
            .iter()
            .flat_map(|s| &s.buildings)
            .flat_map(|b| &b.storeys)
            .flat_map(|st| &st.elements)
            .collect();

        let slab = all_elements
            .iter()
            .find(|e| e.element_type == crate::semantic::model::ElementType::Slab);
        if let Some(slab) = slab {
            assert!(
                slab.properties.iter().any(|p| p.key == "boundary"),
                "Slab should have boundary"
            );
            assert!(
                slab.properties.iter().any(|p| p.key == "thickness"),
                "Slab should have thickness"
            );
        }
    }

    #[test]
    fn test_decompile_no_duplicate_element_ids() {
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let ifc = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&ifc).unwrap();
        let mut all_ids = Vec::new();
        for site in &model.sites {
            for bldg in &site.buildings {
                for storey in &bldg.storeys {
                    for elem in &storey.elements {
                        all_ids.push(elem.id.clone());
                        for child in &elem.children {
                            all_ids.push(child.id.clone());
                        }
                    }
                }
            }
        }
        let len_before = all_ids.len();
        all_ids.sort();
        all_ids.dedup();
        assert_eq!(all_ids.len(), len_before, "Found duplicate element IDs");
    }

    #[test]
    fn test_decompile_proxy_has_position() {
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let ifc = std::fs::read_to_string(path).unwrap();
        let model = decompile_ifc(&ifc).unwrap();
        let all_elements: Vec<_> = model
            .sites
            .iter()
            .flat_map(|s| &s.buildings)
            .flat_map(|b| &b.storeys)
            .flat_map(|st| &st.elements)
            .collect();

        if let Some(proxy) = all_elements
            .iter()
            .find(|e| e.element_type == crate::semantic::model::ElementType::Proxy)
        {
            assert!(
                proxy.properties.iter().any(|p| p.key == "position"),
                "Proxy should have position"
            );
        }
    }

    /// Count all elements in a storey (including children).
    fn count_elements(storey: &crate::semantic::model::ResolvedStorey) -> usize {
        let mut count = 0;
        for elem in &storey.elements {
            count += 1;
            count += elem.children.len();
        }
        for space in &storey.spaces {
            count += space.elements.len();
        }
        count
    }

    /// IFC features the roundtrip relies on: GlobalIds, names with quotes,
    /// bodiless aggregates, explicit openings and space containment.
    const ROUNDTRIP_BML: &str = r#"Material: Brick__Common
  ifc_name: "Brick, Common"

# Site: S
## Building: B
### Storey: GF
  elevation: 0.0 m
  height: 3.0 m

  Stair: ST-1
    guid: "0abcdefghijklmnopqrstu"
    ifc_name: "Stair 60\" wide"
    representation: none

  Proxy: PX-2
    guid: "1abcdefghijklmnopqrstu"
    ifc_class: "IFCSTAIRFLIGHT"
    part_of: ST-1
    boundary: [(0, 0), (1, 0), (1, 2), (0, 2)]
    height: 1.5 m

  Wall: W-3
    guid: "2abcdefghijklmnopqrstu"
    material: Brick__Common
    from: (0, 0) to: (5, 0)
    thickness: 0.2 m
    height: 3.0 m
    Opening: OP-4
      guid: "3abcdefghijklmnopqrstu"
      boundary: [(1, -0.1), (2, -0.1), (2, 0.1), (1, 0.1)]
      z_offset: 1.0 m
      height: 1.2 m
    Window: WIN-5
      guid: "4abcdefghijklmnopqrstu"
      opening: OP-4
      boundary: [(1, -0.05), (2, -0.05), (2, 0.05), (1, 0.05)]
      z_offset: 1.0 m
      height: 1.2 m

#### Space: R1
  boundary: [(0, 0), (5, 0), (5, 4), (0, 4)]
  height: 3.0 m

  Proxy: PX-6
    guid: "5abcdefghijklmnopqrstu"
    boundary: [(1, 1), (2, 1), (2, 2), (1, 2)]
    height: 0.8 m
"#;

    fn compile(source: &str) -> String {
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, diags) = semantic::analyze(&ast);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == semantic::diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
        codegen::generate_ifc(&model).unwrap()
    }

    fn line_with<'a>(ifc: &'a str, needle: &str) -> &'a str {
        ifc.lines()
            .find(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("no line with {needle}"))
    }

    #[test]
    fn test_decode_step_str() {
        use super::ifc_reader::decode_step_str;
        assert_eq!(decode_step_str(r"B\X2\00FC\X0\ro"), "Büro");
        assert_eq!(decode_step_str(r"Gr\X\F6\X\DFe"), "Größe");
        assert_eq!(decode_step_str("it''s"), "it's");
        assert_eq!(decode_step_str(r"a\\b"), r"a\b");
        assert_eq!(decode_step_str(r"bad\X2\00F"), r"bad\X2\00F");
        assert_eq!(decode_step_str(r"p\S\e grunn"), "på grunn");
        assert_eq!(decode_step_str(r"\S\'' 34 BauO"), "§ 34 BauO");
        // `§` (`\S\'`) and an escaped quote at the end survive unquoting.
        use super::ifc_reader::unquote;
        assert_eq!(
            decode_step_str(unquote(r"'Brandschutz \S\'''")),
            "Brandschutz §"
        );
        assert_eq!(decode_step_str(unquote("'say ''hi'''")), "say 'hi'");
    }

    #[test]
    fn test_statements_span_lines() {
        use super::ifc_reader::statements;
        let s: Vec<&str> = statements("#1=IFCA('a;b');\n#2=IFCB((#3,\n#4));\n").collect();
        assert_eq!(s, ["#1=IFCA('a;b');", "\n#2=IFCB((#3,\n#4));", "\n"]);
        let e: Vec<String> = super::ifc_reader::entity_statements(
            "/* Architect's; model */\n#1=IFCA('x');\n#2=IFCB(\n#3);",
        )
        .map(|s| s.into_owned())
        .collect();
        assert_eq!(e, ["#1=IFCA('x');", "#2=IFCB(#3);"]);
    }

    #[test]
    fn test_window_filling_slab_opening_is_valid() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Slab: S1
    boundary: [(0,0), (5,0), (5,5), (0,5)]
    thickness: 0.2 m
    Opening: OP-01
      boundary: [(1,1), (2,1), (2,2), (1,2)]
      height: 0.2 m
    Window: WIN-01
      opening: OP-01
      boundary: [(1,1), (2,1), (2,2), (1,2)]
      height: 0.05 m
";
        let ast = cst_to_ast::parse(source).expect("should parse");
        let (_, diagnostics) = semantic::analyze(&ast);
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.code == semantic::diagnostics::DiagnosticCode::E202),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn test_codegen_aggregates_and_space_containment() {
        let ifc = compile(ROUNDTRIP_BML);
        let stair = line_with(&ifc, "IFCSTAIR('0abcdefghijklmnopqrstu'");
        assert!(stair.contains("'Stair 60\" wide'"), "{stair}");
        assert!(
            stair.contains(",$,"),
            "bodiless stair has no shape: {stair}"
        );
        let flight = line_with(&ifc, "IFCSTAIRFLIGHT('1abcdefghijklmnopqrstu'");
        let id = |l: &str| l.split('=').next().unwrap().to_string();
        let mat = id(line_with(&ifc, "IFCMATERIAL('Brick, Common',$,$)"));
        let wall = id(line_with(&ifc, "'2abcdefghijklmnopqrstu'"));
        assert!(
            line_with(&ifc, "IFCRELASSOCIATESMATERIAL(").ends_with(&format!("({wall}),{mat});"))
        );
        assert!(
            ifc.lines().any(|l| l.contains("IFCRELAGGREGATES(")
                && l.contains(&format!("{},({})", id(stair), id(flight)))),
            "no stair aggregate"
        );
        // The flight is a part, not contained; the space's proxy is in the space.
        let space = id(line_with(&ifc, "IFCSPACE("));
        let proxy = id(line_with(&ifc, "'5abcdefghijklmnopqrstu'"));
        let in_space = ifc
            .lines()
            .find(|l| {
                l.contains("IFCRELCONTAINEDINSPATIALSTRUCTURE")
                    && l.ends_with(&format!(",{space});"))
            })
            .expect("space containment");
        assert!(in_space.contains(&proxy), "{in_space}");
        for l in ifc
            .lines()
            .filter(|l| l.contains("IFCRELCONTAINEDINSPATIALSTRUCTURE"))
        {
            assert!(
                !l.contains(&format!("{},", id(flight)))
                    && !l.contains(&format!("{})", id(flight))),
                "{l}"
            );
        }
    }

    #[test]
    fn test_ifc_bml_ifc_roundtrip() {
        let ifc = compile(ROUNDTRIP_BML);
        let bml = decompile_to_bml(&ifc).unwrap();
        for expected in [
            "guid: \"0abcdefghijklmnopqrstu\"",
            "ifc_name: \"Stair 60\\\" wide\"",
            "representation: none",
            "ifc_class: \"IFCSTAIRFLIGHT\"",
            "#### Space: R1",
            "Material: Brick__Common",
            "ifc_name: \"Brick, Common\"",
            "material: Brick__Common",
        ] {
            assert!(bml.contains(expected), "missing `{expected}` in:\n{bml}");
        }
        assert!(
            bml.lines()
                .any(|l| l.trim_start().starts_with("part_of: ST-")),
            "{bml}"
        );
        assert!(
            bml.lines()
                .any(|l| l.trim_start().starts_with("opening: OP-")),
            "{bml}"
        );
        // Storey elements come before the space heading, so they stay in the storey.
        let space_at = bml.find("#### Space").unwrap();
        assert!(bml.find("guid: \"2abcdefghijklmnopqrstu\"").unwrap() < space_at);
        assert!(bml.find("guid: \"5abcdefghijklmnopqrstu\"").unwrap() > space_at);

        // The decompiled BML parses back with every GlobalId. Geometry the
        // reader cannot recover comes from `mesh_fit` (needs a mesher, CLI only).
        let ast = cst_to_ast::parse(&bml).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let bml_again = bml_writer::generate_bml(&model);
        for g in ["0abc", "1abc", "2abc", "3abc", "4abc", "5abc"] {
            assert!(
                bml_again.contains(&format!("guid: \"{g}defghijklmnopqrstu\"")),
                "lost {g}"
            );
        }
    }

    /// Hand-written export with spaces around `=`, feet as length unit, a wall
    /// with only an axis representation and an empty `AcousticRating`.
    const FEET_IFC: &str = "ISO-10303-21;
DATA;
#1 = IFCPROJECT('0projectghijklmnopqrst', $, 'P', $, $, $, $, $, #2);
#2 = IFCUNITASSIGNMENT((#5));
#3 = IFCSIUNIT(*, .LENGTHUNIT., $, .METRE.);
#4 = IFCMEASUREWITHUNIT(IFCRATIOMEASURE(0.3048), #3);
#5 = IFCCONVERSIONBASEDUNIT(#6, .LENGTHUNIT., 'FOOT', #4);
#6 = IFCDIMENSIONALEXPONENTS(1, 0, 0, 0, 0, 0, 0);
#10 = IFCSITE('0siteefghijklmnopqrstu', $, 'S', $, $, $, $, $, .ELEMENT., $, $, $, $, $);
#11 = IFCBUILDING('0buildingijklmnopqrstu', $, 'B', $, $, $, $, $, .ELEMENT., $, $, $);
#12 = IFCBUILDINGSTOREY('0storeyghijklmnopqrstu', $, 'L1', $, $, $, $, $, .ELEMENT., 10.);
#13 = IFCRELAGGREGATES('0agg1fghijklmnopqrstu', $, $, $, #1, (#10));
#14 = IFCRELAGGREGATES('0agg2fghijklmnopqrstu', $, $, $, #10, (#11));
#15 = IFCRELAGGREGATES('0agg3fghijklmnopqrstu', $, $, $, #11, (#12));
#20 = IFCWALL('0wallefghijklmnopqrstu', $, 'W', $, $, $, #21, $);
#21 = IFCPRODUCTDEFINITIONSHAPE($, $, (#22));
#22 = IFCSHAPEREPRESENTATION($, 'Axis', 'Curve2D', ());
#23 = IFCRELCONTAINEDINSPATIALSTRUCTURE('0contfghijklmnopqrstu', $, $, $, (#20), #12);
#30 = IFCPROPERTYSINGLEVALUE('AcousticRating', $, IFCLABEL(''), $);
#31 = IFCPROPERTYSINGLEVALUE('IsExternal', $, IFCBOOLEAN(.T.), $);
#32 = IFCPROPERTYSET('0psetfghijklmnopqrstu', $, 'Pset_WallCommon', $, (#30, #31));
#33 = IFCRELDEFINESBYPROPERTIES('0defsfghijklmnopqrstu', $, $, $, (#20), #32);
ENDSEC;
END-ISO-10303-21;
";

    #[test]
    fn test_decompile_layered_materials_roundtrip() {
        let source = std::fs::read_to_string("../../tests/fixtures/layered_materials.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = crate::codegen::generate_ifc(&model).unwrap();

        let bml = decompile_to_bml(&ifc).unwrap();
        assert!(
            bml.contains(
                "    - mineral_wool: 0.14 m\n    - concrete: 0.15 m\n    - plaster: 0.01 m"
            ),
            "{bml}"
        );
        assert!(bml.contains("thermal_transmittance: 0.24"), "{bml}");
        assert!(bml.contains("acoustic_rating: 52"), "{bml}");

        // Wall geometry needs the CLI's mesh fitting, so only the materials are checked here.
        let ast = cst_to_ast::parse(&bml).unwrap();
        let (model, diags) = semantic::analyze(&ast);
        assert!(
            !diags
                .iter()
                .any(|d| matches!(d.code.as_str(), "E301" | "E306")),
            "{diags:?}"
        );
        let wall = model
            .materials
            .iter()
            .find(|m| m.layers().is_some_and(|l| l.len() == 3));
        assert!(wall.is_some(), "{bml}");
    }

    #[test]
    fn test_decompile_predefined_type_roundtrip() {
        let source = std::fs::read_to_string("../../tests/fixtures/predefined_types.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let ifc = crate::codegen::generate_ifc(&model).unwrap();
        let bml = decompile_to_bml(&ifc).unwrap();
        for t in ["shear", "gate", "dome_roof"] {
            assert!(bml.contains(&format!("predefined_type: {t}\n")), "{bml}");
        }
        // The default flat roof type is not repeated.
        let ifc = ifc.replace(".DOME_ROOF.", ".FLAT_ROOF.");
        let bml = decompile_to_bml(&ifc).unwrap();
        assert!(!bml.contains("flat_roof"), "{bml}");
    }

    #[test]
    fn test_decompile_covering() {
        let source = std::fs::read_to_string("../../tests/fixtures/all_elements.bml").unwrap();
        let (model, _) = semantic::analyze(&cst_to_ast::parse(&source).unwrap());
        let ifc = crate::codegen::generate_ifc(&model).unwrap();
        let bml = decompile_to_bml(&ifc).unwrap();
        let covering = &bml[bml.find("  Covering: CV-").expect(&bml)..];
        assert!(covering.contains("ifc_name: \"CV-01\""), "{bml}");
        assert!(covering.contains("predefined_type: ceiling\n"), "{bml}");
        assert!(!bml.contains("ifc_class: IfcCovering"), "{bml}");
    }

    #[test]
    fn test_decompile_footing() {
        let source = std::fs::read_to_string("../../tests/fixtures/all_elements.bml").unwrap();
        let (model, _) = semantic::analyze(&cst_to_ast::parse(&source).unwrap());
        let ifc = crate::codegen::generate_ifc(&model).unwrap();
        let bml = decompile_to_bml(&ifc).unwrap();
        let footing = &bml[bml.find("  Footing: FT-").expect(&bml)..];
        assert!(footing.contains("ifc_name: \"FT-01\""), "{bml}");
        assert!(
            footing.contains("predefined_type: strip_footing\n"),
            "{bml}"
        );
        assert!(!bml.contains("ifc_class: IfcFooting"), "{bml}");
    }

    #[test]
    fn test_decompile_feet_axis_only_wall_and_mistyped_pset() {
        let bml = decompile_to_bml(FEET_IFC).unwrap();
        assert!(bml.contains("elevation: 3.048"), "{bml}");
        assert!(bml.contains("representation: none"), "{bml}");
        assert!(bml.contains("is_external: true"), "{bml}");
        assert!(!bml.contains("acoustic_rating"), "{bml}");
        let ast = cst_to_ast::parse(&bml).unwrap();
        let (_, diags) = semantic::analyze(&ast);
        assert!(
            !diags
                .iter()
                .any(|d| d.severity == semantic::diagnostics::Severity::Error),
            "{diags:?}"
        );
    }

    /// Two buildings, one with a storey, one containing its wall directly; a
    /// wall contained twice (invalid) keeps its first container.
    const TWO_BUILDINGS_IFC: &str = "ISO-10303-21;
DATA;
#1 = IFCPROJECT('0projectghijklmnopqrst', $, 'P', $, $, $, $, $, $);
#10 = IFCSITE('0siteefghijklmnopqrstu', $, 'S', $, $, $, $, $, .ELEMENT., $, $, $, $, $);
#11 = IFCBUILDING('0buildingAjklmnopqrstu', $, 'A', $, $, $, $, $, .ELEMENT., $, $, $);
#12 = IFCBUILDING('0buildingBjklmnopqrstu', $, 'B', $, $, $, $, $, .ELEMENT., $, $, $);
#13 = IFCBUILDINGSTOREY('0storeyghijklmnopqrstu', $, 'L1', $, $, $, $, $, .ELEMENT., 0.);
#14 = IFCRELAGGREGATES('0agg1fghijklmnopqrstu', $, $, $, #1, (#10));
#15 = IFCRELAGGREGATES('0agg2fghijklmnopqrstu', $, $, $, #10, (#11, #12));
#16 = IFCRELAGGREGATES('0agg3fghijklmnopqrstu', $, $, $, #11, (#13));
#20 = IFCWALL('0wallAfghijklmnopqrstu', $, 'WA', $, $, $, $, $);
#21 = IFCWALL('0wallBfghijklmnopqrstu', $, 'WB', $, $, $, $, $);
#22 = IFCRELCONTAINEDINSPATIALSTRUCTURE('0cont1ghijklmnopqrstu', $, $, $, (#20), #13);
#23 = IFCRELCONTAINEDINSPATIALSTRUCTURE('0cont2ghijklmnopqrstu', $, $, $, (#21), #12);
#24 = IFCRELCONTAINEDINSPATIALSTRUCTURE('0cont3ghijklmnopqrstu', $, $, $, (#21), #13);
ENDSEC;
END-ISO-10303-21;
";

    #[test]
    fn test_decompile_two_buildings() {
        let model = decompile_ifc(TWO_BUILDINGS_IFC).unwrap();
        let b = &model.sites[0].buildings;
        assert_eq!(b.len(), 2);
        assert_eq!((b[0].name.as_str(), b[1].name.as_str()), ("A", "B"));
        assert_eq!(b[0].storeys[0].name, "L1");
        assert_eq!(b[0].storeys[0].elements.len(), 1);
        // Contained in the building itself: a building-level element.
        assert!(b[1].storeys.is_empty());
        assert_eq!(b[1].elements.len(), 1);
    }

    /// Spaces and elements of the site and building themselves, outside any storey.
    const NO_STOREY_BML: &str = r#"# Site: S
  Slab: SL-1
    boundary: [(0, 0), (9, 0), (9, 9), (0, 9)]
    thickness: 0.2 m

#### Space: Yard
  boundary: [(0, 0), (4, 0), (4, 4), (0, 4)]
  height: 2 m

## Building: B
  Wall: W-1
    from: (0, 0) to: (5, 0)
    thickness: 0.2 m
    height: 3 m

#### Space: Hall
  boundary: [(0, 0), (5, 0), (5, 4), (0, 4)]
  height: 3 m

### Storey: GF
  elevation: 0 m
#### Space: Room
  boundary: [(0, 0), (2, 0), (2, 2), (0, 2)]
  height: 3 m
"#;

    #[test]
    fn test_site_and_building_level_contents() {
        let (model, diags) =
            crate::semantic::analyze(&crate::parser::cst_to_ast::parse(NO_STOREY_BML).unwrap());
        assert!(
            diags
                .iter()
                .all(|d| d.severity != crate::semantic::diagnostics::Severity::Error),
            "{diags:?}"
        );
        let site = &model.sites[0];
        let b = &site.buildings[0];
        assert_eq!(
            (site.elements.len(), site.spaces[0].name.as_str()),
            (1, "Yard")
        );
        assert_eq!((b.elements.len(), b.spaces[0].name.as_str()), (1, "Hall"));
        assert_eq!(b.storeys[0].spaces[0].name, "Room");

        // Building-level elements are contained in the IfcBuilding, site-level
        // ones in the IfcSite; their spaces are aggregated there.
        let ifc = compile(NO_STOREY_BML);
        let id = |l: &str| l.split('=').next().unwrap().to_string();
        let (site_id, building) = (
            id(line_with(&ifc, "IFCSITE(")),
            id(line_with(&ifc, "IFCBUILDING(")),
        );
        let (wall, slab) = (
            id(line_with(&ifc, "IFCWALL(")),
            id(line_with(&ifc, "IFCSLAB(")),
        );
        assert!(ifc.contains(&format!("({wall}),{building});")), "{ifc}");
        assert!(ifc.contains(&format!("({slab}),{site_id});")), "{ifc}");
        let yard = id(line_with(&ifc, "'Yard'"));
        assert!(
            ifc.contains(&format!("{site_id},({building},{yard}))")),
            "{ifc}"
        );

        // And they decompile back to the same places.
        let back = decompile_ifc(&ifc).unwrap();
        let (site, b) = (&back.sites[0], &back.sites[0].buildings[0]);
        assert_eq!((site.elements.len(), site.spaces.len()), (1, 1));
        assert_eq!(
            (b.elements.len(), b.spaces.len(), b.storeys.len()),
            (1, 1, 1)
        );
        let bml = decompile_to_bml(&ifc).unwrap();
        let (round, _) = crate::semantic::analyze(&crate::parser::cst_to_ast::parse(&bml).unwrap());
        assert_eq!(round.sites[0].spaces.len(), 1, "{bml}");
        assert_eq!(round.sites[0].buildings[0].spaces.len(), 1, "{bml}");
    }

    /// A door of the upper storey in a two-storey wall of the ground floor.
    const CROSS_STOREY_BML: &str = r#"# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W-1
    from: (0, 0) to: (5, 0)
    thickness: 0.2 m
    height: 6 m
    Opening: OP-1
      boundary: [(1, -0.1), (2, -0.1), (2, 0.1), (1, 0.1)]
      z_offset: 3 m
      height: 2 m

### Storey: UF
  elevation: 3 m
  Door: D-1
    opening: OP-1
    boundary: [(1, -0.05), (2, -0.05), (2, 0.05), (1, 0.05)]
    height: 2 m

  Roof: R-1
    type: flat
    boundary: [(0, 0), (5, 0), (5, 4), (0, 4)]
    thickness: 0.3 m
"#;

    #[test]
    fn test_cross_storey_filler_roundtrip() {
        let ifc = compile(CROSS_STOREY_BML);
        let id = |l: &str| l.split('=').next().unwrap().to_string();
        let opening = id(line_with(&ifc, "IFCOPENINGELEMENT("));
        let door = id(line_with(&ifc, "IFCDOOR("));
        let fills = line_with(&ifc, "IFCRELFILLSELEMENT(");
        assert!(fills.ends_with(&format!("{opening},{door});")), "{fills}");

        // The door stays in its own storey and keeps its opening.
        let bml = decompile_to_bml(&ifc).unwrap();
        let upper = bml.find("### Storey: UF").unwrap();
        let door_at = bml.find("Door:").unwrap();
        assert!(door_at > upper, "{bml}");
        assert!(bml[door_at..].contains("opening: OP-"), "{bml}");
    }

    #[test]
    fn test_sleeve_filler_stays_out_of_its_host() {
        let ifc = compile(
            "# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W
    from: (5, 0) to: (5, 8)
    height: 3 m
    thickness: 0.24 m
  Pipe: P
    diameter: 0.1 m
    from: (1, 4, 2) to: (9, 4, 2)
    sleeve: true
",
        );
        assert_eq!(ifc.matches("=IFCRELFILLSELEMENT(").count(), 1);
        // Only doors and windows nest under their host wall.
        let bml = decompile_to_bml(&ifc).unwrap();
        let (_, diags) = crate::semantic::analyze(&crate::parser::cst_to_ast::parse(&bml).unwrap());
        assert!(diags.is_empty(), "{diags:?}\n{bml}");
        assert!(compile(&bml).contains("=IFCRELFILLSELEMENT("), "{bml}");
    }

    #[test]
    fn test_uncontained_filler_takes_host_storey() {
        let ifc = compile(
            "# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
### Storey: OG
  elevation: 3 m
  Wall: W
    from: (5, 0) to: (5, 8)
    height: 3 m
    thickness: 0.24 m
  Pipe: P
    diameter: 0.1 m
    from: (1, 4, 2) to: (9, 4, 2)
    sleeve: true
",
        );
        // Drop the sleeve from its storey's containment.
        let sleeve = line_with(&ifc, "'P-W-SL'");
        let sleeve = &sleeve[..sleeve.find('=').unwrap()];
        let contains = line_with(&ifc, "IFCRELCONTAINEDINSPATIALSTRUCTURE(");
        let stripped = contains
            .replace(&format!("{sleeve},"), "")
            .replace(&format!(",{sleeve})"), ")");
        assert_ne!(contains, stripped);
        let bml = decompile_to_bml(&ifc.replace(contains, &stripped)).unwrap();
        let og = bml.find("### Storey: OG").unwrap();
        assert!(bml.find("P-W-SL").unwrap() > og, "{bml}");
    }

    #[test]
    fn test_grid_with_boundary_has_body() {
        let ifc = compile(
            "# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Grid: GR-1
    boundary: [(0, 0), (1, 0), (1, 1), (0, 1)]
    height: 0.05
",
        );
        let grid = line_with(&ifc, "IFCGRID(");
        let args: Vec<&str> = grid.split(',').collect();
        assert!(args[6].starts_with('#'), "{grid}");
    }

    #[test]
    fn test_grid_with_boundary_keeps_axis_position() {
        let ifc = compile(
            "# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Grid: GR-1
    position: (20, 10)
    boundary: [(20, 10), (21, 10), (21, 11), (20, 11)]
    height: 0.05
",
        );
        assert!(ifc.contains("IFCCARTESIANPOINT((20.,10.))"), "{ifc}");
    }

    #[test]
    fn test_bodiless_door_in_wall_has_no_body() {
        let ifc = compile(
            "# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W-1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.2 m
    Door: D-1
      representation: none
",
        );
        let door = line_with(&ifc, "IFCDOOR(");
        assert_eq!(door.split(',').nth(6), Some("$"), "{door}");
        assert!(!ifc.contains("IFCRELFILLSELEMENT"), "{ifc}");
    }

    #[test]
    fn test_hosted_filler_of_later_opening_is_filled_at_the_end() {
        let ifc = compile(
            "# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W-1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.2 m
    Door: D-1
      opening: OP-1
      offset: 1 m
      width: 1 m
      height: 2 m

### Storey: UF
  elevation: 3 m
  Opening: OP-1
    boundary: [(1, -0.1), (2, -0.1), (2, 0.1), (1, 0.1)]
    height: 2 m
",
        );
        assert_eq!(ifc.matches("IFCOPENINGELEMENT(").count(), 1, "{ifc}");
        let id = |l: &str| l.split('=').next().unwrap().to_string();
        let opening = id(line_with(&ifc, "IFCOPENINGELEMENT("));
        let door = id(line_with(&ifc, "IFCDOOR("));
        let fills = line_with(&ifc, "IFCRELFILLSELEMENT(");
        assert!(fills.ends_with(&format!("{opening},{door});")), "{fills}");
    }

    #[test]
    fn test_opening_ref_to_non_opening_writes_no_fill() {
        let ifc = compile(
            "# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W-1
    from: (0, 0) to: (5, 0)
    height: 3 m
    thickness: 0.2 m
  Door: D-1
    opening: W-1
    boundary: [(1, 0), (2, 0), (2, 0.1), (1, 0.1)]
    height: 2 m
",
        );
        assert!(!ifc.contains("IFCRELFILLSELEMENT"), "{ifc}");
    }

    #[test]
    fn test_ifc2x3_only_class_becomes_ifc4() {
        let ifc = TWO_BUILDINGS_IFC
            .replace(
                "IFCWALL('0wallAfghijklmnopqrstu', $, 'WA', $, $, $, $, $)",
                "IFCELECTRICDISTRIBUTIONPOINT('0wallAfghijklmnopqrstu', $, 'WA', $, $, #32, #30, $, .ALARMPANEL., $)",
            )
            .replace(
                "ENDSEC;\nEND",
                "#30 = IFCPRODUCTDEFINITIONSHAPE($, $, (#31));\n\
                 #31 = IFCSHAPEREPRESENTATION($, 'Body', 'Brep', ());\n#32 = IFCLOCALPLACEMENT($, $);\nENDSEC;\nEND",
            );
        let bml = decompile_to_bml(&ifc).unwrap();
        assert!(bml.contains("IFCELECTRICDISTRIBUTIONBOARD"), "{bml}");
    }

    #[test]
    fn test_bodiless_host_keeps_fillers() {
        let ifc = compile(
            "# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Roof: R-1
    representation: none
    Opening: OP-1
      boundary: [(1, 1), (2, 1), (2, 2), (1, 2)]
      height: 0.3 m
    Window: WIN-1
      opening: OP-1
      boundary: [(1, 1), (2, 1), (2, 2), (1, 2)]
      height: 0.05 m
",
        );
        for class in [
            "IFCOPENINGELEMENT(",
            "IFCWINDOW(",
            "IFCRELVOIDSELEMENT(",
            "IFCRELFILLSELEMENT(",
        ] {
            line_with(&ifc, class);
        }
    }

    const NURBS_SLAB: &str = "# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Slab: SL-1
    boundary: { outer: [(0, 0), smooth (10, -2), (20, 0), (20, 15), smooth (10, 18), (0, 15)], holes: [[smooth (8, 6), smooth (11, 5), smooth (12, 8), smooth (9, 9)]] }
    thickness: 0.25 m
";

    fn analyze_clean(source: &str) -> semantic::model::SemanticModel {
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, diags) = semantic::analyze(&ast);
        use semantic::diagnostics::Severity;
        let problems: Vec<_> = diags
            .iter()
            .filter(|d| matches!(d.severity, Severity::Error | Severity::Warning))
            .collect();
        assert!(problems.is_empty(), "{problems:?}\n{source}");
        model
    }

    fn slab_bbox(model: &semantic::model::SemanticModel) -> [f32; 6] {
        let scene = crate::scene::generate_scene(model);
        fn walk(n: &crate::scene::SceneNode, b: &mut [f32; 6]) {
            for v in &n.mesh.vertices {
                for i in 0..3 {
                    b[i] = b[i].min(v.position[i]);
                    b[i + 3] = b[i + 3].max(v.position[i]);
                }
            }
            n.children.iter().for_each(|c| walk(c, b));
        }
        let mut b = [f32::MAX, f32::MAX, f32::MAX, f32::MIN, f32::MIN, f32::MIN];
        scene
            .nodes
            .iter()
            .filter(|n| n.element_type == "Slab")
            .for_each(|n| walk(n, &mut b));
        b
    }

    #[test]
    fn decompile_nurbs_slab_roundtrips() {
        let ifc = compile(NURBS_SLAB);
        let bml = decompile_to_bml(&ifc).unwrap();
        let back = analyze_clean(&bml);
        let slab = back
            .elements()
            .find(|e| e.element_type == semantic::model::ElementType::Slab)
            .unwrap();
        assert!(slab.curve("boundary").is_some(), "{bml}");
        codegen::generate_ifc(&back).unwrap();
        let (want, got) = (slab_bbox(&analyze_clean(NURBS_SLAB)), slab_bbox(&back));
        for i in 0..6 {
            assert!((want[i] - got[i]).abs() < 0.005, "{want:?} vs {got:?}");
        }
    }

    #[test]
    fn bml_writer_writes_nurbs_item() {
        let bml = decompile_to_bml(&compile(NURBS_SLAB)).unwrap();
        assert!(bml.contains("nurbs { degree: 3"), "{bml}");
        assert!(!bml.contains("weights"), "{bml}");
    }

    #[test]
    fn bml_writer_writes_weights_and_knots() {
        let source = "# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Slab: SL-1
    boundary: [(0, 0), nurbs { degree: 2, points: [(0, 0), (4, -2), (8, -1), (10, 0)], weights: [1, 2, 0.5, 1], knots: [0, 0, 0, 0.3, 1, 1, 1] }, (10, 6), (0, 6)]
    thickness: 0.25 m
";
        let bml = decompile_to_bml(&compile(source)).unwrap();
        assert!(bml.contains("weights: [1, 2, 0.5, 1]"), "{bml}");
        assert!(bml.contains("knots: [0, 0, 0, 0.3, 1, 1, 1]"), "{bml}");
        // The closed outline does not repeat its start point.
        assert!(bml.contains("(10.0, 6.0), (0.0, 6.0)]"), "{bml}");
        analyze_clean(&bml);
    }

    #[test]
    fn decompile_spline_wall_axis_to_path() {
        let ifc = compile(
            "# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W-1
    path: [(1, 1), smooth (5, 2.5), smooth (9, 2), (13, 4)]
    height: 3 m
    thickness: 0.3 m
",
        );
        let model = decompile_ifc(&ifc).unwrap();
        let wall = model
            .elements()
            .find(|e| e.element_type == semantic::model::ElementType::Wall)
            .unwrap();
        let Some(semantic::model::ElementCurve::Path2(path)) = wall.curve("path") else {
            panic!("{:?}", wall.properties)
        };
        let ends = (
            path.segments[0].start(),
            path.segments.last().unwrap().end(),
        );
        let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-4;
        assert!(
            near(ends.0, [1.0, 1.0]) && near(ends.1, [13.0, 4.0]),
            "{ends:?}"
        );
        let num = |k: &str| match &wall.properties.iter().find(|p| p.key == k).unwrap().value {
            semantic::model::ResolvedValue::Number(n) => *n,
            v => panic!("{v:?}"),
        };
        assert!(
            (num("thickness") - 0.3).abs() < 0.002,
            "{}",
            num("thickness")
        );
        assert!((num("height") - 3.0).abs() < 1e-9);
    }

    #[test]
    fn decompile_hollow_swept_disk_keeps_no_curve() {
        let ifc = compile(
            "# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Beam: B-1
    profile: circle(0.1 m)
    z_offset: 2 m
    path: [(0, 0), smooth (3, 1), (6, 0)]
",
        );
        let has_curve = |ifc: &str| {
            let model = decompile_ifc(ifc).unwrap();
            model
                .elements()
                .find(|e| e.element_type == semantic::model::ElementType::Beam)
                .unwrap()
                .curve("path")
                .is_some()
        };
        assert!(has_curve(&ifc));
        // A hollow tube is not a BML beam; the plain reader takes it.
        let hollow: String = ifc
            .lines()
            .map(|l| {
                let l = if l.contains("IFCSWEPTDISKSOLID(") {
                    l.replace(",$,$,$)", ",0.04,$,$)")
                } else {
                    l.to_string()
                };
                l + "\n"
            })
            .collect();
        assert!(hollow.contains(",0.04,$,$)"), "{hollow}");
        assert!(!has_curve(&hollow));
    }
}
