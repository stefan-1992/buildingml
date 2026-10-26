//! Resolve IFC presentation styles and material associations.
//!
//! Without this, every element is drawn in one flat colour per IFC type, which
//! throws away the surface styles authoring tools write into the file. Two
//! sources are read here:
//!
//! * `IfcStyledItem` — a style attached to a geometry representation item.
//!   This is the per-item channel, so a window's frame and glazing resolve to
//!   different materials.
//! * `IfcRelAssociatesMaterial` → `IfcMaterial` →
//!   `IfcMaterialDefinitionRepresentation` — a style attached to the material
//!   rather than the geometry. Some exporters use only this channel.
//!
//! Both funnel into `IfcSurfaceStyle` → `IfcSurfaceStyleRendering` (or the
//! `…Shading` supertype) → `IfcColourRgb`, which also carries transparency and
//! the specular parameters the viewer shades with.

use std::collections::HashMap;

use bml_core::scene::SceneMaterial;
use ifc_lite_core::{AttributeValue, EntityDecoder, EntityScanner};

/// The style/material entity types we need to tell apart by entity id.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    SurfaceStyle,
    /// `IfcPresentationStyleAssignment` — the IFC2x3 wrapper around a style set.
    StyleAssignment,
    /// `IfcSurfaceStyleShading` or its `IfcSurfaceStyleRendering` subtype.
    Shading,
    ColourRgb,
    Material,
    /// Container whose material members are reached via attribute 0.
    MaterialVia0,
    /// Container whose material members are reached via attribute 2.
    MaterialVia2,
}

fn classify(type_name: &str) -> Option<Kind> {
    Some(match type_name {
        "IFCSURFACESTYLE" => Kind::SurfaceStyle,
        "IFCPRESENTATIONSTYLEASSIGNMENT" => Kind::StyleAssignment,
        "IFCSURFACESTYLESHADING" | "IFCSURFACESTYLERENDERING" => Kind::Shading,
        "IFCCOLOURRGB" => Kind::ColourRgb,
        "IFCMATERIAL" => Kind::Material,
        // ForLayerSet / MaterialLayers / Material / Materials / ForProfileSet
        "IFCMATERIALLAYERSETUSAGE"
        | "IFCMATERIALLAYERSET"
        | "IFCMATERIALLAYER"
        | "IFCMATERIALLAYERWITHOFFSETS"
        | "IFCMATERIALLIST"
        | "IFCMATERIALPROFILESETUSAGE"
        | "IFCMATERIALPROFILESETUSAGETAPERING" => Kind::MaterialVia0,
        // MaterialConstituents / Material / MaterialProfiles
        "IFCMATERIALCONSTITUENTSET"
        | "IFCMATERIALCONSTITUENT"
        | "IFCMATERIALPROFILESET"
        | "IFCMATERIALPROFILE"
        | "IFCMATERIALPROFILEWITHOFFSETS" => Kind::MaterialVia2,
        _ => return None,
    })
}

/// Guard against malformed files pointing a material container at itself.
const MAX_MATERIAL_DEPTH: usize = 8;

/// Lower bound on opacity.
///
/// Revit writes `Transparency = 1.0` for glass, meaning "as transparent as
/// glass gets" rather than "invisible". Taken literally it erases the glazing:
/// on `tests/fixtures/model_2.ifc` that is 409 whole elements, most of a
/// curtain wall facade. Mainstream IFC viewers clamp instead, so we do too.
const MIN_ALPHA: f32 = 0.12;

/// Separator for name lists. Material names routinely contain commas
/// ("Brick, Common"), so joining with one would be unreadable.
pub const NAME_SEP: &str = " · ";

/// Style and material lookup for one IFC file.
pub struct StyleIndex {
    /// Deduplicated materials; scene nodes reference these by index.
    pub materials: Vec<SceneMaterial>,
    /// Geometry representation item id → material index.
    by_item: HashMap<u32, u32>,
    /// Product id → material index, resolved through its associated material.
    by_product: HashMap<u32, u32>,
    /// Product id → associated material names, for the inspector.
    names: HashMap<u32, String>,
    /// `IfcMaterial` name → material index, from the material channel.
    by_material_name: HashMap<String, u32>,
}

impl StyleIndex {
    /// Material for a geometry representation item, as reported by ifc-lite's
    /// sub-mesh channel.
    pub fn for_item(&self, item_id: u32) -> Option<u32> {
        self.by_item.get(&item_id).copied()
    }

    /// Material associated with the product itself (no per-item style).
    pub fn for_product(&self, product_id: u32) -> Option<u32> {
        self.by_product.get(&product_id).copied()
    }

    /// `IfcMaterial` names associated with the product, joined with `NAME_SEP`
    /// (material names contain commas, so a comma cannot be the separator). For
    /// a layer buildup this lists every layer, which is what a user inspecting
    /// a wall wants to see even though we draw it with a single colour.
    pub fn material_names(&self, product_id: u32) -> Option<&str> {
        self.names.get(&product_id).map(|s| s.as_str())
    }

    pub fn get(&self, index: u32) -> Option<&SceneMaterial> {
        self.materials.get(index as usize)
    }

    /// Look of the `IfcMaterial` called `name`: its own style, else a surface
    /// style of the same name (Revit names styles after their material but
    /// attaches them to geometry items only).
    pub fn for_material_name(&self, name: &str) -> Option<&SceneMaterial> {
        match self.by_material_name.get(name) {
            Some(&i) => self.get(i),
            None => self.materials.iter().find(|m| m.name == name),
        }
    }

    /// Scan `content` and resolve every style reachable from a geometry item or
    /// a material association.
    pub fn build(content: &[u8], decoder: &mut EntityDecoder) -> Self {
        let mut kinds: HashMap<u32, Kind> = HashMap::new();
        let mut styled_items: Vec<u32> = Vec::new();
        let mut material_defs: Vec<u32> = Vec::new();
        let mut assoc_material: Vec<u32> = Vec::new();

        let mut scanner = EntityScanner::new(content);
        while let Some((id, type_name, _, _)) = scanner.next_entity() {
            match type_name {
                "IFCSTYLEDITEM" => styled_items.push(id),
                "IFCMATERIALDEFINITIONREPRESENTATION" => material_defs.push(id),
                "IFCRELASSOCIATESMATERIAL" => assoc_material.push(id),
                _ => {
                    if let Some(kind) = classify(type_name) {
                        kinds.insert(id, kind);
                    }
                }
            }
        }

        let mut index = StyleIndex {
            materials: Vec::new(),
            by_item: HashMap::new(),
            by_product: HashMap::new(),
            names: HashMap::new(),
            by_material_name: HashMap::new(),
        };
        // Surface style entity id → index into `materials`. Files reuse a style
        // across thousands of items, so this is where the dedup happens.
        let mut interned: HashMap<u32, u32> = HashMap::new();

        // --- Channel 1: styles attached to geometry items -------------------
        // A styled item with a null Item belongs to an IfcStyledRepresentation
        // (channel 2); remember its style so the material lookup can use it.
        let mut style_of_styled_item: HashMap<u32, u32> = HashMap::new();
        for sid in &styled_items {
            let Ok(styled) = decoder.decode_by_id(*sid) else {
                continue;
            };
            let item = styled.get(0).and_then(|a| a.as_entity_ref());
            let Some(styles) = styled.get(1).and_then(|a| a.as_list()) else {
                continue;
            };
            let Some(style_id) = surface_style_in(styles, &kinds, decoder) else {
                continue;
            };
            let Some(mat) = index.intern(style_id, &kinds, decoder, &mut interned) else {
                continue;
            };
            match item {
                Some(item_id) => {
                    index.by_item.insert(item_id, mat);
                }
                None => {
                    style_of_styled_item.insert(*sid, mat);
                }
            }
        }

        // --- Channel 2: styles attached to materials ------------------------
        let mut style_of_material: HashMap<u32, u32> = HashMap::new();
        for did in &material_defs {
            let Ok(def) = decoder.decode_by_id(*did) else {
                continue;
            };
            let Some(material_id) = def.get(3).and_then(|a| a.as_entity_ref()) else {
                continue;
            };
            let Some(reps) = def.get(2).and_then(|a| a.as_list()) else {
                continue;
            };
            'reps: for rep in reps {
                let Some(rep) = rep
                    .as_entity_ref()
                    .and_then(|r| decoder.decode_by_id(r).ok())
                else {
                    continue;
                };
                let Some(items) = rep.get(3).and_then(|a| a.as_list()) else {
                    continue;
                };
                for item in items {
                    let Some(item_id) = item.as_entity_ref() else {
                        continue;
                    };
                    // The styled item may carry its style inline (Item = $) or,
                    // rarely, point at geometry we already indexed.
                    if let Some(&mat) = style_of_styled_item
                        .get(&item_id)
                        .or_else(|| index.by_item.get(&item_id))
                    {
                        style_of_material.insert(material_id, mat);
                        if let Some(name) = decoder
                            .decode_by_id(material_id)
                            .ok()
                            .and_then(|m| m.get(0).and_then(|a| a.as_string()).map(str::to_string))
                        {
                            index.by_material_name.insert(name, mat);
                        }
                        break 'reps;
                    }
                }
            }
        }

        // --- Material associations -> product colour + names ----------------
        for rid in &assoc_material {
            let Ok(rel) = decoder.decode_by_id(*rid) else {
                continue;
            };
            let Some(relating) = rel.get(5).and_then(|a| a.as_entity_ref()) else {
                continue;
            };
            let mut material_ids = Vec::new();
            collect_materials(relating, &kinds, decoder, &mut material_ids, 0);
            if material_ids.is_empty() {
                continue;
            }

            let mut names: Vec<String> = Vec::new();
            for mid in &material_ids {
                let name = decoder
                    .decode_by_id(*mid)
                    .ok()
                    .and_then(|m| m.get(0).and_then(|a| a.as_string()).map(str::to_string))
                    .unwrap_or_default();
                if !name.is_empty() && !names.contains(&name) {
                    names.push(name);
                }
            }
            let joined = names.join(NAME_SEP);
            // The dominant layer is not knowable without slicing, so the first
            // material with a style stands in for the whole element.
            let style = material_ids
                .iter()
                .find_map(|mid| style_of_material.get(mid).copied());

            let Some(objects) = rel.get(4).and_then(|a| a.as_list()) else {
                continue;
            };
            for obj in objects {
                let Some(oid) = obj.as_entity_ref() else {
                    continue;
                };
                if !joined.is_empty() {
                    index.names.insert(oid, joined.clone());
                }
                if let Some(mat) = style {
                    index.by_product.insert(oid, mat);
                }
            }
        }

        index
    }

    /// Resolve an `IfcSurfaceStyle` to a material index, reusing the entry if
    /// the style was already resolved.
    fn intern(
        &mut self,
        style_id: u32,
        kinds: &HashMap<u32, Kind>,
        decoder: &mut EntityDecoder,
        interned: &mut HashMap<u32, u32>,
    ) -> Option<u32> {
        if let Some(&existing) = interned.get(&style_id) {
            return Some(existing);
        }
        let material = resolve_surface_style(style_id, kinds, decoder)?;
        let idx = self.materials.len() as u32;
        self.materials.push(material);
        interned.insert(style_id, idx);
        Some(idx)
    }
}

/// First `IfcSurfaceStyle` in a style list, unwrapping the IFC2x3
/// `IfcPresentationStyleAssignment` indirection.
fn surface_style_in(
    styles: &[AttributeValue],
    kinds: &HashMap<u32, Kind>,
    decoder: &mut EntityDecoder,
) -> Option<u32> {
    for style in styles {
        let Some(sid) = style.as_entity_ref() else {
            continue;
        };
        match kinds.get(&sid) {
            Some(Kind::SurfaceStyle) => return Some(sid),
            Some(Kind::StyleAssignment) => {
                let Ok(assignment) = decoder.decode_by_id(sid) else {
                    continue;
                };
                let Some(inner) = assignment.get(0).and_then(|a| a.as_list()) else {
                    continue;
                };
                for candidate in inner {
                    let cid = candidate.as_entity_ref();
                    if cid.is_some_and(|c| kinds.get(&c) == Some(&Kind::SurfaceStyle)) {
                        return cid;
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// `IfcSurfaceStyle(Name, Side, Styles)` → colour + shading parameters.
fn resolve_surface_style(
    style_id: u32,
    kinds: &HashMap<u32, Kind>,
    decoder: &mut EntityDecoder,
) -> Option<SceneMaterial> {
    let style = decoder.decode_by_id(style_id).ok()?;
    let name = style
        .get(0)
        .and_then(|a| a.as_string())
        .unwrap_or("")
        .to_string();
    let members = style.get(2).and_then(|a| a.as_list())?.to_vec();

    for member in &members {
        let Some(mid) = member.as_entity_ref() else {
            continue;
        };
        // Skip IfcSurfaceStyleLighting / …WithTextures / …Refraction, which
        // carry no base colour.
        if kinds.get(&mid) != Some(&Kind::Shading) {
            continue;
        }
        let Ok(shading) = decoder.decode_by_id(mid) else {
            continue;
        };
        let Some(colour_id) = shading.get(0).and_then(|a| a.as_entity_ref()) else {
            continue;
        };
        if kinds.get(&colour_id) != Some(&Kind::ColourRgb) {
            continue;
        }
        let Ok(colour) = decoder.decode_by_id(colour_id) else {
            continue;
        };
        // IfcColourRgb(Name, Red, Green, Blue), each 0..1.
        let r = colour.get(1).and_then(|a| a.as_float()).unwrap_or(0.7) as f32;
        let g = colour.get(2).and_then(|a| a.as_float()).unwrap_or(0.7) as f32;
        let b = colour.get(3).and_then(|a| a.as_float()).unwrap_or(0.7) as f32;
        // Revit writes pure black for materials whose colour was never set
        // ("Concrete, Cast-in-Place gray" in model_1); a recognised name is the
        // better guess.
        let [r, g, b] = match bml_core::scene::appearance::guess(&name) {
            Some(look) if [r, g, b] == [0.0; 3] && !name.to_lowercase().contains("black") => {
                [look.color[0], look.color[1], look.color[2]]
            }
            _ => [r, g, b],
        };

        // Attributes past index 0 exist only on IfcSurfaceStyleRendering; on the
        // IfcSurfaceStyleShading supertype they are simply absent.
        let transparency = shading.get(1).and_then(|a| a.as_float()).unwrap_or(0.0) as f32;
        let specular = shading
            .get(6)
            .and_then(specular_intensity)
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        let shininess = shading
            .get(7)
            .and_then(specular_exponent)
            .unwrap_or(16.0)
            .clamp(1.0, 256.0);

        return Some(SceneMaterial {
            name,
            color: [r, g, b, (1.0 - transparency).clamp(MIN_ALPHA, 1.0)],
            specular,
            shininess,
        });
    }
    None
}

/// The STEP type name of a typed value such as `IFCSPECULAREXPONENT(128.)`,
/// which the decoder stores as a list of `[type name, value…]`.
fn typed_value_name(attr: &AttributeValue) -> Option<&str> {
    attr.as_list()?.first()?.as_string()
}

/// `IfcSurfaceStyleRendering.SpecularColour` is an `IfcColourOrFactor`. Only the
/// factor form carries a usable scalar; a full `IfcColourRgb` would need a
/// second colour channel in the shader, so it is treated as a mid intensity.
fn specular_intensity(attr: &AttributeValue) -> Option<f32> {
    if attr.is_null() {
        return None;
    }
    if attr.as_entity_ref().is_some() {
        return Some(0.5);
    }
    attr.as_float().map(|v| v as f32)
}

/// `IfcSurfaceStyleRendering.SpecularHighlight` is either an exponent or a
/// roughness; convert roughness to the Blinn-Phong exponent the shader wants.
fn specular_exponent(attr: &AttributeValue) -> Option<f32> {
    let value = attr.as_float()? as f32;
    match typed_value_name(attr) {
        Some("IFCSPECULARROUGHNESS") => {
            let r = value.clamp(0.001, 1.0);
            Some(2.0 / (r * r) - 2.0)
        }
        _ => Some(value),
    }
}

/// Flatten a `RelatingMaterial` into the `IfcMaterial` entities it contains,
/// walking layer sets, constituent sets, profile sets and material lists.
fn collect_materials(
    id: u32,
    kinds: &HashMap<u32, Kind>,
    decoder: &mut EntityDecoder,
    out: &mut Vec<u32>,
    depth: usize,
) {
    if depth >= MAX_MATERIAL_DEPTH || out.len() > 64 {
        return;
    }
    let attr_index = match kinds.get(&id) {
        Some(Kind::Material) => {
            if !out.contains(&id) {
                out.push(id);
            }
            return;
        }
        Some(Kind::MaterialVia0) => 0,
        Some(Kind::MaterialVia2) => 2,
        _ => return,
    };
    let Ok(entity) = decoder.decode_by_id(id) else {
        return;
    };
    let Some(attr) = entity.get(attr_index) else {
        return;
    };
    // The member attribute is either a single reference or a list of them.
    if let Some(child) = attr.as_entity_ref() {
        collect_materials(child, kinds, decoder, out, depth + 1);
    } else if let Some(list) = attr.as_list() {
        for child in list.iter().filter_map(|item| item.as_entity_ref()) {
            collect_materials(child, kinds, decoder, out, depth + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ifc_lite_core::build_entity_index;

    /// Wrap DATA-section entities in the minimal STEP envelope the scanner needs.
    fn ifc(entities: &str) -> Vec<u8> {
        format!("ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n{entities}\nENDSEC;\nEND-ISO-10303-21;\n")
            .into_bytes()
    }

    fn build(entities: &str) -> StyleIndex {
        let content = ifc(entities);
        let index = build_entity_index(&content);
        let mut decoder = EntityDecoder::with_index(&content, index);
        // `content` outlives the borrow: build() copies everything it needs.
        let styles = StyleIndex::build(&content, &mut decoder);
        drop(decoder);
        styles
    }

    /// IFC4: IfcStyledItem.Styles holds IfcSurfaceStyle directly.
    #[test]
    fn resolves_styled_item_colour() {
        let styles = build(
            "#1=IFCCOLOURRGB($,0.8,0.4,0.2);\n\
             #2=IFCSURFACESTYLERENDERING(#1,0.,$,$,$,$,IFCNORMALISEDRATIOMEASURE(0.5),IFCSPECULAREXPONENT(64.),.NOTDEFINED.);\n\
             #3=IFCSURFACESTYLE('Brick, Common',.BOTH.,(#2));\n\
             #4=IFCSTYLEDITEM(#99,(#3),$);",
        );
        let idx = styles.for_item(99).expect("geometry item 99 is styled");
        let material = styles.get(idx).unwrap();
        assert_eq!(material.name, "Brick, Common");
        assert_eq!(material.color, [0.8, 0.4, 0.2, 1.0]);
        assert_eq!(material.specular, 0.5);
        assert_eq!(material.shininess, 64.0);
    }

    /// IFC2x3 wraps the style set in an IfcPresentationStyleAssignment.
    #[test]
    fn unwraps_presentation_style_assignment() {
        let styles = build(
            "#1=IFCCOLOURRGB($,0.1,0.2,0.3);\n\
             #2=IFCSURFACESTYLESHADING(#1);\n\
             #3=IFCSURFACESTYLE('Steel',.BOTH.,(#2));\n\
             #4=IFCPRESENTATIONSTYLEASSIGNMENT((#3));\n\
             #5=IFCSTYLEDITEM(#99,(#4),$);",
        );
        let material = styles.get(styles.for_item(99).unwrap()).unwrap();
        assert_eq!(material.name, "Steel");
        assert_eq!(material.color, [0.1, 0.2, 0.3, 1.0]);
        // IfcSurfaceStyleShading has no rendering attributes; shading defaults apply.
        assert_eq!(material.specular, 0.0);
    }

    /// Styles reused across items must resolve to one shared material entry.
    #[test]
    fn dedupes_shared_styles() {
        let styles = build(
            "#1=IFCCOLOURRGB($,0.5,0.5,0.5);\n\
             #2=IFCSURFACESTYLESHADING(#1);\n\
             #3=IFCSURFACESTYLE('Grey',.BOTH.,(#2));\n\
             #4=IFCSTYLEDITEM(#97,(#3),$);\n\
             #5=IFCSTYLEDITEM(#98,(#3),$);\n\
             #6=IFCSTYLEDITEM(#99,(#3),$);",
        );
        assert_eq!(styles.materials.len(), 1);
        assert_eq!(styles.for_item(97), styles.for_item(99));
    }

    /// Channel 2: the style hangs off the material, not the geometry.
    #[test]
    fn resolves_material_definition_representation() {
        let styles = build(
            "#1=IFCCOLOURRGB($,0.2,0.6,0.3);\n\
             #2=IFCSURFACESTYLESHADING(#1);\n\
             #3=IFCSURFACESTYLE('Turf',.BOTH.,(#2));\n\
             #4=IFCSTYLEDITEM($,(#3),$);\n\
             #5=IFCSTYLEDREPRESENTATION(#0,'Style','Material',(#4));\n\
             #6=IFCMATERIAL('Turf');\n\
             #7=IFCMATERIALDEFINITIONREPRESENTATION($,$,(#5),#6);\n\
             #8=IFCRELASSOCIATESMATERIAL('guid',$,$,$,(#50),#6);",
        );
        let material = styles.get(styles.for_product(50).unwrap()).unwrap();
        assert_eq!(material.name, "Turf");
        assert_eq!(material.color, [0.2, 0.6, 0.3, 1.0]);
        assert_eq!(styles.material_names(50), Some("Turf"));
    }

    /// A layer set flattens to every layer's material, in file order.
    #[test]
    fn flattens_material_layer_set_usage() {
        let styles = build(
            "#1=IFCMATERIAL('Brick, Common');\n\
             #2=IFCMATERIAL('Air');\n\
             #3=IFCMATERIAL('Gypsum Wall Board');\n\
             #4=IFCMATERIALLAYER(#1,0.1,$);\n\
             #5=IFCMATERIALLAYER(#2,0.05,$);\n\
             #6=IFCMATERIALLAYER(#3,0.012,$);\n\
             #7=IFCMATERIALLAYERSET((#4,#5,#6),'Ext',$);\n\
             #8=IFCMATERIALLAYERSETUSAGE(#7,.AXIS2.,.POSITIVE.,0.);\n\
             #9=IFCRELASSOCIATESMATERIAL('guid',$,$,$,(#50),#8);",
        );
        assert_eq!(
            styles.material_names(50),
            Some("Brick, Common · Air · Gypsum Wall Board"),
        );
    }

    /// Revit's Transparency=1.0 glass must stay visible rather than vanish.
    #[test]
    fn clamps_fully_transparent_glass() {
        let styles = build(
            "#1=IFCCOLOURRGB($,0.6,0.8,0.9);\n\
             #2=IFCSURFACESTYLERENDERING(#1,1.,$,$,$,$,$,$,.NOTDEFINED.);\n\
             #3=IFCSURFACESTYLE('Glass',.BOTH.,(#2));\n\
             #4=IFCSTYLEDITEM(#99,(#3),$);",
        );
        let material = styles.get(styles.for_item(99).unwrap()).unwrap();
        assert_eq!(material.color[3], MIN_ALPHA);
    }

    /// Black on a recognised material name is an unset Revit colour; black on an
    /// unknown name is kept.
    #[test]
    fn replaces_unset_black_by_name() {
        let styles = build(
            "#1=IFCCOLOURRGB($,0.,0.,0.);\n\
             #2=IFCSURFACESTYLERENDERING(#1,0.,$,$,$,$,$,$,.NOTDEFINED.);\n\
             #3=IFCSURFACESTYLE('Concrete, Cast-in-Place gray',.BOTH.,(#2));\n\
             #4=IFCSTYLEDITEM(#98,(#3),$);\n\
             #5=IFCSURFACESTYLE('Ebony',.BOTH.,(#2));\n\
             #6=IFCSTYLEDITEM(#99,(#5),$);\n\
             #7=IFCSURFACESTYLE('Metal - Steel - Black',.BOTH.,(#2));\n\
             #8=IFCSTYLEDITEM(#97,(#7),$);",
        );
        let black_steel = styles.get(styles.for_item(97).unwrap()).unwrap();
        assert_eq!(black_steel.color, [0.0, 0.0, 0.0, 1.0]);
        let concrete = styles.get(styles.for_item(98).unwrap()).unwrap();
        assert!(concrete.color[0] > 0.5);
        let ebony = styles.get(styles.for_item(99).unwrap()).unwrap();
        assert_eq!(ebony.color, [0.0, 0.0, 0.0, 1.0]);
    }

    /// Partial transparency is carried through untouched.
    #[test]
    fn keeps_partial_transparency() {
        let styles = build(
            "#1=IFCCOLOURRGB($,0.6,0.8,0.9);\n\
             #2=IFCSURFACESTYLERENDERING(#1,0.75,$,$,$,$,$,$,.NOTDEFINED.);\n\
             #3=IFCSURFACESTYLE('Tinted',.BOTH.,(#2));\n\
             #4=IFCSTYLEDITEM(#99,(#3),$);",
        );
        let material = styles.get(styles.for_item(99).unwrap()).unwrap();
        assert!((material.color[3] - 0.25).abs() < 1e-6);
    }

    /// IfcSpecularRoughness is a roughness, not an exponent, so it needs converting.
    #[test]
    fn converts_specular_roughness_to_exponent() {
        let styles = build(
            "#1=IFCCOLOURRGB($,0.5,0.5,0.5);\n\
             #2=IFCSURFACESTYLERENDERING(#1,0.,$,$,$,$,IFCNORMALISEDRATIOMEASURE(0.3),IFCSPECULARROUGHNESS(0.25),.NOTDEFINED.);\n\
             #3=IFCSURFACESTYLE('Polished',.BOTH.,(#2));\n\
             #4=IFCSTYLEDITEM(#99,(#3),$);",
        );
        let material = styles.get(styles.for_item(99).unwrap()).unwrap();
        // 2 / 0.25^2 - 2 = 30
        assert!(
            (material.shininess - 30.0).abs() < 1e-4,
            "{}",
            material.shininess
        );
        assert!((material.specular - 0.3).abs() < 1e-6);
    }

    /// Styles with no shading member carry no colour and must be skipped.
    #[test]
    fn ignores_styles_without_shading() {
        let styles = build(
            "#1=IFCSURFACESTYLELIGHTING($,$,$,$);\n\
             #2=IFCSURFACESTYLE('Lighting only',.BOTH.,(#1));\n\
             #3=IFCSTYLEDITEM(#99,(#2),$);",
        );
        assert!(styles.for_item(99).is_none());
        assert!(styles.materials.is_empty());
    }

    /// A self-referential material container must not recurse forever.
    #[test]
    fn survives_cyclic_material_reference() {
        let styles = build(
            "#1=IFCMATERIALLAYERSET((#2),'Loop',$);\n\
             #2=IFCMATERIALLAYER(#1,0.1,$);\n\
             #3=IFCRELASSOCIATESMATERIAL('guid',$,$,$,(#50),#1);",
        );
        assert_eq!(styles.material_names(50), None);
    }
}
