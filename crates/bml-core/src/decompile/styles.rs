//! IFC surface styles → BML material appearance, so the BML preview of a
//! decompiled model matches its IFC preview.

use std::collections::HashMap;

use super::ifc_reader::{make_unique, material_def, prop};
use crate::scene::{SceneMaterial, appearance};
use crate::semantic::model::{ResolvedElement, ResolvedMaterial, ResolvedValue, SemanticModel};

/// Give every material its IFC look and every styled element the material it
/// is drawn with.
///
/// `element_styles` maps a GlobalId to the surface style covering most of the
/// element's area. That style's material becomes the element's `material:`
/// (created when the element was not associated with it), because the IFC
/// preview shows the style. Elements with a layered material keep it, so their
/// BML preview shows the first layer even where the IFC item style differs.
/// `material_style` resolves a material name to its own style. Each styled
/// material gets `color` / `opacity` / `specular` / `shininess`. Returns how
/// many materials were styled.
pub fn apply_styles(
    model: &mut SemanticModel,
    element_styles: &HashMap<String, SceneMaterial>,
    material_style: impl Fn(&str) -> Option<SceneMaterial>,
) -> usize {
    fn walk(
        e: &mut ResolvedElement,
        styles: &HashMap<String, SceneMaterial>,
        defs: &mut Vec<ResolvedMaterial>,
    ) {
        let guid = e
            .properties
            .iter()
            .find(|p| p.key == "guid")
            .map(|p| &p.value);
        // A layer set keeps its layers; its look comes from the first layer.
        let layered = || {
            e.properties.iter().any(|p| {
                p.key == "material"
                    && matches!(&p.value, ResolvedValue::Identifier(id)
                        if defs.iter().any(|d| d.id == *id && d.layers().is_some()))
            })
        };
        if let Some(ResolvedValue::String(g)) = guid
            && let Some(style) = styles.get(g).filter(|s| !s.name.is_empty())
            && !layered()
        {
            // A layer set of the style's name is not the styled material.
            let id = match defs
                .iter()
                .find(|d| d.layers().is_none() && d.ifc_name() == style.name)
            {
                Some(d) => d.id.clone(),
                None => {
                    let mut def = material_def(style.name.clone());
                    // Different names can sanitize to the same id ("Ziegel ä", "Ziegel ö").
                    make_unique(&mut def, |id| defs.iter().any(|d| d.id == id));
                    let id = def.id.clone();
                    defs.push(def);
                    id
                }
            };
            e.properties.retain(|p| p.key != "material");
            e.properties
                .push(prop("material", ResolvedValue::Identifier(id)));
        }
        e.children.iter_mut().for_each(|c| walk(c, styles, defs));
    }
    let mut defs = std::mem::take(&mut model.materials);
    for e in model.elements_mut() {
        walk(e, element_styles, &mut defs);
    }
    model.materials = defs;

    let mut styled = 0;
    for def in &mut model.materials {
        let name = def.ifc_name().to_string();
        let Some(look) = material_style(&name) else {
            continue;
        };
        let [r, g, b, a] = look.color;
        def.properties.push(prop(
            "color",
            ResolvedValue::String(appearance::to_hex([r, g, b])),
        ));
        // Always written: an opaque "Glass fibre" style must not take the glass palette's opacity.
        def.properties
            .push(prop("opacity", ResolvedValue::Number(round2(a))));
        def.properties.push(prop(
            "specular",
            ResolvedValue::Number(round2(look.specular)),
        ));
        def.properties.push(prop(
            "shininess",
            ResolvedValue::Number(round2(look.shininess)),
        ));
        styled += 1;
    }
    styled
}

fn round2(v: f32) -> f64 {
    (v as f64 * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cst_to_ast;
    use crate::semantic;

    fn look(name: &str, color: [f32; 4]) -> SceneMaterial {
        SceneMaterial {
            name: name.into(),
            color,
            specular: 0.5,
            shininess: 64.0,
        }
    }

    #[test]
    fn styled_elements_get_one_material_per_style_name() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Proxy: A
    guid: \"a\"
    position: (0, 0)
  Proxy: B
    guid: \"b\"
    position: (1, 0)
";
        let (mut model, _) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        let styles = [
            ("a".to_string(), look("Ziegel ä", [1.0, 0.0, 0.0, 1.0])),
            ("b".to_string(), look("Ziegel ö", [0.0, 0.0, 1.0, 1.0])),
        ];
        let by_name: HashMap<_, _> = styles
            .iter()
            .map(|(_, m)| (m.name.clone(), m.clone()))
            .collect();
        let element_styles: HashMap<_, _> = styles.into_iter().collect();

        let styled = apply_styles(&mut model, &element_styles, |n| by_name.get(n).cloned());

        assert_eq!(styled, 2);
        let ids: Vec<_> = model.materials.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(
            ids[0], ids[1],
            "names that sanitize alike keep distinct ids"
        );
        let blue = model
            .materials
            .iter()
            .find(|m| m.ifc_name() == "Ziegel ö")
            .unwrap();
        let get = |k: &str| {
            blue.properties
                .iter()
                .find(|p| p.key == k)
                .map(|p| p.value.clone())
        };
        assert!(matches!(get("color"), Some(ResolvedValue::String(c)) if c == "#0000ff"));
        // Opaque styles still write their opacity, so no palette alpha leaks in.
        assert!(matches!(get("opacity"), Some(ResolvedValue::Number(o)) if o == 1.0));
    }
}
