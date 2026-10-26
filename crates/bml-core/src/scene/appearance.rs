//! Material appearance shared by the BML and the IFC preview.
//!
//! A material's look comes from, in order: its explicit `color` / `opacity` /
//! `specular` / `shininess` properties, a keyword match of its name against
//! [`PALETTE`], or the default material of the element type. The IFC preview
//! falls back to the same palette for elements without a surface style, so a
//! BML model and the IFC compiled from it look alike.
//!
//! Colours are sRGB. Most are the defaults Autodesk Revit writes into IFC
//! surface styles (read from `tests/fixtures/model_*.ifc`); where Revit has no
//! single default they are physicallybased.info base colours converted from
//! linear to sRGB (marked "pb").

use super::graph::SceneMaterial;
use crate::semantic::model::{ElementType, ResolvedMaterial, ResolvedProperty, ResolvedValue};

/// One palette entry: display name, sRGB colour, opacity, specular, shininess.
struct Look(&'static str, u32, f32, f32, f32);

const GLASS: Look = Look("Glass", 0xcce0ea, 0.3, 0.5, 128.0);
const ALUMINIUM: Look = Look("Aluminium", 0xf4f5f6, 1.0, 0.5, 128.0);
const STAINLESS: Look = Look("Stainless Steel", 0xecefef, 1.0, 0.5, 128.0);
const COPPER: Look = Look("Copper", 0xf7cfbf, 1.0, 0.5, 64.0); // pb
const STEEL: Look = Look("Steel", 0xc1bebb, 1.0, 0.5, 64.0); // pb iron
const CMU: Look = Look("Concrete Masonry Units", 0x99968e, 1.0, 0.1, 16.0);
const BRICK: Look = Look("Brick", 0x8c5746, 1.0, 0.0, 4.0); // pb
const CONCRETE: Look = Look("Concrete", 0xc0c0c0, 1.0, 0.1, 16.0);
const GYPSUM: Look = Look("Gypsum Wall Board", 0xdddddb, 1.0, 0.0, 4.0);
const PLASTER: Look = Look("Plaster", 0xf9f9f9, 1.0, 0.0, 4.0);
const OAK: Look = Look("Oak", 0xd4b371, 1.0, 0.2, 32.0);
const BIRCH: Look = Look("Birch", 0xe0b27e, 1.0, 0.2, 32.0);
const PINE: Look = Look("Pine", 0xcfaf80, 1.0, 0.2, 32.0);
const CHERRY: Look = Look("Cherry", 0x9e4d28, 1.0, 0.2, 32.0);
const PLYWOOD: Look = Look("Plywood", 0xd4c2ab, 1.0, 0.0, 8.0);
const TIMBER: Look = Look("Structural Timber", 0xe6bd85, 1.0, 0.0, 8.0);
const WOOD: Look = Look("Wood", 0xad8460, 1.0, 0.2, 32.0);
const DOOR: Look = Look("Door Panel", 0xd29f5f, 1.0, 0.2, 32.0);
const TILE: Look = Look("Ceramic Tile", 0xe0e0dc, 1.0, 0.5, 64.0); // pb porcelain
const STONE: Look = Look("Stone", 0xebe6e1, 1.0, 0.3, 64.0); // pb marble
const CARPET: Look = Look("Carpet", 0xa09384, 1.0, 0.0, 1.0);
const ROOFING: Look = Look("Asphalt Shingle", 0x5f5e55, 1.0, 0.0, 8.0);
const INSULATION: Look = Look("Insulation", 0x7f7f7f, 1.0, 0.0, 4.0);
const FABRIC: Look = Look("Fabric", 0xf7f7ef, 1.0, 0.0, 4.0);
const LEATHER: Look = Look("Leather", 0x282828, 1.0, 0.3, 32.0);
const LAMINATE: Look = Look("Laminate", 0xf4f4f4, 1.0, 0.3, 32.0);

/// Keyword → look. The first entry with a keyword in the name wins, so the more
/// specific entries come first ("plywood" before "wood", "masonry unit" before
/// "concrete").
const PALETTE: &[(&[&str], Look)] = &[
    (
        &[
            "glass wool",
            "glass fibre",
            "glass fiber",
            "stone wool",
            "mineral wool",
        ],
        INSULATION,
    ),
    (&["glass", "glazing"], GLASS),
    (&["aluminium", "aluminum"], ALUMINIUM),
    (&["stainless", "chrome"], STAINLESS),
    (&["copper", "bronze"], COPPER),
    (&["steel", "metal", "iron"], STEEL),
    (&["masonry unit", "cmu", "concrete block"], CMU),
    (&["brick", "masonry"], BRICK),
    (&["concrete", "cement", "screed"], CONCRETE),
    (&["gypsum", "drywall", "plasterboard"], GYPSUM),
    (&["plaster", "stucco"], PLASTER),
    (&["plywood", "sheathing", "osb"], PLYWOOD),
    (&["joist", "rafter", "lumber", "glulam"], TIMBER),
    (&["oak"], OAK),
    (&["birch"], BIRCH),
    (&["pine", "spruce"], PINE),
    (&["cherry"], CHERRY),
    (&["wood", "timber"], WOOD),
    (&["tile", "ceramic", "porcelain"], TILE),
    (&["marble", "stone", "granite"], STONE),
    (&["carpet"], CARPET),
    (&["shingle", "asphalt", "bitumen", "roofing"], ROOFING),
    (&["insulation", "membrane", "vapor", "damp"], INSULATION),
    (&["fabric", "textile", "linen", "upholstery"], FABRIC),
    (&["leather"], LEATHER),
    (&["laminate"], LAMINATE),
];

impl Look {
    fn material(&self) -> SceneMaterial {
        let Look(name, rgb, alpha, specular, shininess) = *self;
        let channel = |shift: u32| ((rgb >> shift) & 0xff) as f32 / 255.0;
        SceneMaterial {
            name: name.to_string(),
            color: [channel(16), channel(8), channel(0), alpha],
            specular,
            shininess,
        }
    }
}

/// Palette look for a free-form material name (`"Wood - Birch"`, `wood_light`).
/// Keywords match at the start of a word, so "textile" is no tile and
/// "fiberglass" no glass.
pub fn guess(name: &str) -> Option<SceneMaterial> {
    let name = name.to_lowercase().replace(['_', '-'], " ");
    let at_word_start = |k: &str| {
        name.match_indices(k)
            .any(|(i, _)| !name[..i].ends_with(|c: char| c.is_alphanumeric()))
    };
    PALETTE
        .iter()
        .find(|(keys, _)| keys.iter().any(|k| at_word_start(k)))
        .map(|(_, look)| look.material())
}

/// Default look of an element type without a (recognised) material. `None` for
/// types drawn as annotations (grids, openings, proxies, elevator shafts).
pub fn for_type(et: ElementType) -> Option<SceneMaterial> {
    let look = match et {
        ElementType::Wall => PLASTER,
        ElementType::Slab | ElementType::Column | ElementType::Beam => CONCRETE,
        ElementType::Stair | ElementType::Ramp => CONCRETE,
        ElementType::Member | ElementType::Plate | ElementType::Railing => STEEL,
        ElementType::Window => GLASS,
        ElementType::Door => DOOR,
        ElementType::Roof => ROOFING,
        ElementType::Furniture => WOOD,
        _ => return None,
    };
    Some(look.material())
}

/// Frame of a window (aluminium, Revit's "Metal - Aluminum") or door (Revit's
/// "Door - Frame", the oak tone).
pub fn frame(et: ElementType) -> SceneMaterial {
    match et {
        ElementType::Door => OAK,
        _ => ALUMINIUM,
    }
    .material()
}

/// Look of a BML `Material:` definition: explicit appearance properties over a
/// palette guess from its `ifc_name`, id and category. A material with only
/// `opacity` / `specular` / `shininess` gets them on a neutral grey. `None` when
/// nothing applies, leaving the element type's default.
pub fn of_material(def: &ResolvedMaterial) -> Option<SceneMaterial> {
    let p = &def.properties;
    let text = |key| match p.iter().find(|q| q.key == key).map(|q| &q.value) {
        Some(ResolvedValue::String(s) | ResolvedValue::Identifier(s)) => Some(s.as_str()),
        _ => None,
    };
    let name = text("ifc_name").unwrap_or(&def.id);
    let guessed = guess(&format!(
        "{} {} {}",
        name,
        def.id,
        text("category").unwrap_or("")
    ));
    let color = text("color").and_then(parse_hex);
    let explicit = ["opacity", "specular", "shininess"]
        .iter()
        .any(|k| number(p, k).is_some());
    if color.is_none() && guessed.is_none() && !explicit {
        return None;
    }
    let base = guessed.unwrap_or_else(|| CONCRETE.material());
    let [r, g, b, a] = base.color;
    let rgb = color.unwrap_or([r, g, b]);
    Some(SceneMaterial {
        name: name.to_string(),
        // Clamped to the ranges IFC and the IFC preview reader accept.
        color: [
            rgb[0],
            rgb[1],
            rgb[2],
            number(p, "opacity").map_or(a, |o| o.clamp(0.0, 1.0)),
        ],
        specular: number(p, "specular").map_or(base.specular, |s| s.clamp(0.0, 1.0)),
        shininess: number(p, "shininess").map_or(base.shininess, |s| s.clamp(1.0, 256.0)),
    })
}

/// Look of material `id` among `materials`, else a palette guess from the id.
/// A layered material looks like its first layer (the exterior face of a wall,
/// the top of a slab), which is also the first styled material IFC viewers find.
pub fn named(materials: &[ResolvedMaterial], id: &str) -> Option<SceneMaterial> {
    let find = |id: &str| materials.iter().find(|m| m.id == id);
    let def = find(id);
    let id = match def.and_then(|d| d.layers()).and_then(|l| l.first()) {
        Some(first) => first.name.as_str(),
        None => id,
    };
    find(id).map_or_else(|| guess(id), of_material)
}

fn number(props: &[ResolvedProperty], key: &str) -> Option<f32> {
    match props.iter().find(|p| p.key == key).map(|p| &p.value) {
        Some(ResolvedValue::Number(n)) => Some(*n as f32),
        _ => None,
    }
}

/// `#rrggbb` → linear 0..1 channels (still sRGB-encoded, as the renderers expect).
pub fn parse_hex(s: &str) -> Option<[f32; 3]> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some([16, 8, 0].map(|shift| ((v >> shift) & 0xff) as f32 / 255.0))
}

/// `[r, g, b]` 0..1 → `#rrggbb`.
pub fn to_hex(rgb: [f32; 3]) -> String {
    let [r, g, b] = rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Index of `material` in `table`, appending it unless an equal entry exists.
pub fn intern(table: &mut Vec<SceneMaterial>, material: SceneMaterial) -> u32 {
    match table.iter().position(|m| *m == material) {
        Some(i) => i as u32,
        None => {
            table.push(material);
            (table.len() - 1) as u32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cst_to_ast;
    use crate::semantic;

    fn materials(source: &str) -> Vec<ResolvedMaterial> {
        let (model, _) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        model.materials
    }

    #[test]
    fn specific_keywords_win_over_generic_ones() {
        let name = |s| guess(s).map(|m| m.name);
        assert_eq!(name("Plywood, Sheathing").as_deref(), Some("Plywood"));
        assert_eq!(
            name("Concrete Masonry Units").as_deref(),
            Some("Concrete Masonry Units")
        );
        assert_eq!(
            name("Concrete, Cast-in-Place gray").as_deref(),
            Some("Concrete")
        );
        assert_eq!(
            name("Structure, Wood Joist/Rafter Layer").as_deref(),
            Some("Structural Timber")
        );
        assert_eq!(name("Glass_ALUPROF_Clear").as_deref(), Some("Glass"));
        assert_eq!(name("wood_light").as_deref(), Some("Wood"));
        assert_eq!(name("Clad - White"), None);
        assert_eq!(name("Textile").as_deref(), Some("Fabric"));
        assert_eq!(name("Fiberglass"), None);
        assert_eq!(name("glass_wool").as_deref(), Some("Insulation"));
    }

    #[test]
    fn explicit_appearance_overrides_the_palette() {
        let defs = materials(
            "\
Material: conc
  ifc_name: \"Concrete, Precast\"
  color: \"#ff8000\"
  opacity: 0.5

Material: Clad_-_White

Material: double
  category: glass

Material: frosted
  opacity: 0.4
",
        );
        let m = of_material(&defs[0]).unwrap();
        assert_eq!(m.name, "Concrete, Precast");
        assert_eq!(m.color, [1.0, 128.0 / 255.0, 0.0, 0.5]);
        assert_eq!(m.shininess, CONCRETE.material().shininess);
        // Unknown names without a colour fall back to the element type.
        assert!(of_material(&defs[1]).is_none());
        assert_eq!(of_material(&defs[2]).unwrap().color, GLASS.material().color);
        assert_eq!(of_material(&defs[3]).unwrap().color[3], 0.4);
    }

    #[test]
    fn hex_roundtrip() {
        assert_eq!(to_hex(parse_hex("#c0c0c0").unwrap()), "#c0c0c0");
        assert_eq!(parse_hex("c0c0c0"), None);
        assert_eq!(parse_hex("#c0c0"), None);
        assert_eq!(parse_hex("#+fffff"), None);
    }
}
