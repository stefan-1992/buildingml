use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};

use super::ifc_curves::exact_geometry;
use crate::ast::span::{Position, Span};
use crate::codegen::ifc_writer::{default_ifc_class, predefined_type_slot};
use crate::semantic::containment::valid_child_types;
use crate::semantic::model::*;
use crate::semantic::registry::get_descriptor;

/// A pair of 3D direction vectors (axis, ref_direction).
type DirectionPair = ((f64, f64, f64), (f64, f64, f64));

/// Errors during IFC reading.
#[derive(Debug, thiserror::Error)]
pub enum IfcReadError {
    #[error("failed to parse IFC file: {0}")]
    ParseError(String),

    #[error("no spatial structure found in IFC file")]
    NoSpatialStructure,
}

/// A parsed IFC entity with its ID and data.
#[derive(Debug, Clone)]
pub(super) struct IfcEntity {
    pub(super) id: u64,
    pub(super) entity_type: String,
    pub(super) raw_text: String,
}

/// Read an IFC STEP string and convert to a SemanticModel.
///
/// This parser works directly on the STEP text without relying on ifc_rs
/// for validation, making it resilient to real-world IFC files that may
/// contain entity types not supported by ifc_rs.
pub fn read_ifc(ifc_content: &str) -> Result<SemanticModel, IfcReadError> {
    let entities = parse_all_entities(ifc_content);
    if entities.is_empty() {
        return Err(IfcReadError::ParseError(
            "no STEP entities found in file".to_string(),
        ));
    }
    let entity_map: HashMap<u64, &IfcEntity> = entities.iter().map(|e| (e.id, e)).collect();
    let unit_scale = detect_length_unit_scale(&entity_map);
    // IFC2x3 classes keep other enumerations where IFC4 has PredefinedType.
    let ifc4 = !ifc_content.contains("'IFC2X3");
    // Exact geometry is only read for B-spline curves (also the rational ones).
    let splines = ifc_content.contains("BSPLINECURVEWITHKNOTS");

    let mut site: Option<(String, Vec<ResolvedProperty>)> = None;
    let mut site_ids: HashSet<u64> = HashSet::new();
    let mut buildings: Vec<(u64, ResolvedBuilding)> = Vec::new();
    let mut storeys: Vec<(u64, ResolvedStorey)> = Vec::new();
    let mut spaces: Vec<(u64, ResolvedSpace)> = Vec::new();
    let mut elements: Vec<(u64, ElementType)> = Vec::new();
    let mut materials = Vec::new();
    // Relationships: element -> spatial container, part -> aggregate whole,
    // host -> openings, opening -> fillers.
    let mut container: HashMap<u64, u64> = HashMap::new();
    let mut whole: HashMap<u64, u64> = HashMap::new();
    let mut voids: HashMap<u64, Vec<u64>> = HashMap::new();
    let mut fills: HashMap<u64, Vec<u64>> = HashMap::new();
    let mut connects: HashMap<u64, Vec<u64>> = HashMap::new(); // element -> elements it connects
    // element or type -> its IfcRelAssociatesMaterial target; element -> type
    // IfcMaterialLayerSet entity -> its layered `Material:`
    let mut layer_sets: BTreeMap<u64, ResolvedMaterial> = BTreeMap::new();
    let mut material_of: HashMap<u64, u64> = HashMap::new();
    let mut type_of: HashMap<u64, u64> = HashMap::new();

    for entity in &entities {
        let text = &entity.raw_text;
        match entity.entity_type.as_str() {
            "IFCSITE" => {
                site_ids.insert(entity.id);
                site.get_or_insert_with(|| (spatial_name(text, "Site"), guid_props(text)));
            }
            "IFCBUILDING" => {
                buildings.push((
                    entity.id,
                    ResolvedBuilding {
                        name: spatial_name(text, "Building"),
                        properties: guid_props(text),
                        storeys: Vec::new(),
                        spaces: Vec::new(),
                        elements: Vec::new(),
                        span: dummy_span(),
                    },
                ));
            }
            "IFCBUILDINGSTOREY" => {
                let elevation = extract_elevation(text).unwrap_or(0.0) * unit_scale;
                let mut properties = vec![prop("elevation", ResolvedValue::Number(elevation))];
                properties.extend(guid_props(text));
                let name = spatial_name(text, "Storey");
                if name.trim() != name {
                    // A heading trims its name.
                    properties.push(prop("ifc_name", ResolvedValue::String(name.clone())));
                }
                storeys.push((
                    entity.id,
                    ResolvedStorey {
                        name,
                        properties,
                        spaces: Vec::new(),
                        elements: Vec::new(),
                        span: dummy_span(),
                    },
                ));
            }
            "IFCSPACE" => {
                let number = extract_name(text).unwrap_or_default();
                let long_name = extract_long_name(text).unwrap_or_default();
                let name = match (number.is_empty(), long_name.is_empty()) {
                    (false, false) => format!("{} - {}", number, long_name),
                    (true, false) => long_name.clone(),
                    (false, true) => number.clone(),
                    (true, true) => "Space".to_string(),
                };
                let mut properties = guid_props(text);
                if !number.is_empty() {
                    properties.push(prop("ifc_name", ResolvedValue::String(number)));
                }
                if !long_name.is_empty() {
                    properties.push(prop("long_name", ResolvedValue::String(long_name)));
                }
                let (geometry, curves) = splines
                    .then(|| exact_geometry(text, None, &entity_map, unit_scale))
                    .flatten()
                    .unwrap_or_default();
                properties.extend(geometry);
                spaces.push((
                    entity.id,
                    ResolvedSpace {
                        name,
                        properties,
                        elements: Vec::new(),
                        span: dummy_span(),
                        curves,
                    },
                ));
            }
            "IFCMATERIAL" => {
                if let Some(name) = extract_field(text, 0) {
                    materials.push(material_def(name));
                }
            }
            "IFCRELCONTAINEDINSPATIALSTRUCTURE" => {
                let a = args_of(text);
                if a.len() >= 6 {
                    let structure = parse_ref(&a[5]);
                    // An element contained twice (invalid, seen in hitos) keeps
                    // its first container, as IfcOpenShell does
                    for e in parse_ref_list(&a[4]) {
                        container.entry(e).or_insert(structure);
                    }
                }
            }
            "IFCRELAGGREGATES" => {
                let a = args_of(text);
                if a.len() >= 6 {
                    let parent = parse_ref(&a[4]);
                    for e in parse_ref_list(&a[5]) {
                        whole.insert(e, parent);
                    }
                }
            }
            "IFCRELASSOCIATESMATERIAL" | "IFCRELDEFINESBYTYPE" => {
                let a = args_of(text);
                if a.len() >= 6 {
                    let map = if entity.entity_type == "IFCRELDEFINESBYTYPE" {
                        &mut type_of
                    } else {
                        &mut material_of
                    };
                    let target = parse_ref(&a[5]);
                    for e in parse_ref_list(&a[4]) {
                        map.insert(e, target);
                    }
                }
            }
            "IFCRELVOIDSELEMENT" | "IFCRELFILLSELEMENT" => {
                let a = args_of(text);
                if a.len() >= 6 {
                    let map = if entity.entity_type == "IFCRELVOIDSELEMENT" {
                        &mut voids
                    } else {
                        &mut fills
                    };
                    map.entry(parse_ref(&a[4]))
                        .or_default()
                        .push(parse_ref(&a[5]));
                }
            }
            // Exact names: IfcRelConnectsPathElements (wall joins) is a subtype.
            "IFCRELCONNECTSELEMENTS" | "IFCRELCONNECTSWITHREALIZINGELEMENTS" => {
                let a = args_of(text);
                if a.len() >= 7 {
                    let (relating, related) = (parse_ref(&a[5]), parse_ref(&a[6]));
                    let realizing = a.get(7).map(|r| parse_ref_list(r)).unwrap_or_default();
                    if realizing.is_empty() {
                        connects.entry(relating).or_default().push(related);
                    }
                    for r in realizing {
                        let list = connects.entry(r).or_default();
                        for e in [relating, related] {
                            if !list.contains(&e) {
                                list.push(e);
                            }
                        }
                    }
                }
            }
            class => {
                if let Some(et) = element_type_for(class, text) {
                    elements.push((entity.id, et));
                }
            }
        }
    }

    let element_types: HashMap<u64, ElementType> = elements.iter().copied().collect();
    let bml_id = |id: u64| {
        format!(
            "{}-{}",
            get_descriptor(element_types[&id]).auto_id_prefix,
            id
        )
    };

    let property_map = parse_property_sets(ifc_content, unit_scale);
    // `top`: not nested in a host (openings and their fillers are).
    let mut build =
        |id: u64, extra: Vec<ResolvedProperty>, children: Vec<ResolvedElement>, top: bool| {
            let et = element_types[&id];
            let entity = entity_map[&id];
            let text = &entity.raw_text;
            let mut props = guid_props(text);
            let mut curves = BTreeMap::new();
            if let Some(name) = extract_name(text) {
                props.push(prop("ifc_name", ResolvedValue::String(name)));
            }
            let class = ifc4_class(&entity.entity_type);
            if class != default_ifc_class(et) {
                props.push(prop("ifc_class", ResolvedValue::String(class.to_string())));
            }
            if let Some(t) = predefined_type(class, text).filter(|_| ifc4) {
                props.push(prop("predefined_type", ResolvedValue::Identifier(t)));
            }
            if !has_body(text, &entity_map) {
                props.push(prop(
                    "representation",
                    ResolvedValue::Identifier("none".to_string()),
                ));
            } else if let Some((geometry, exact)) = splines
                .then(|| exact_geometry(text, Some((et, top)), &entity_map, unit_scale))
                .flatten()
            {
                props.extend(geometry);
                curves = exact;
            } else {
                props.extend(extract_entity_dimensions(id, et, &entity_map, unit_scale));
            }
            let material = material_of
                .get(&id)
                .or_else(|| type_of.get(&id).and_then(|t| material_of.get(t)));
            if let Some(m) = material.and_then(|&m| {
                material_ref(m, &entity_map, unit_scale, &materials, &mut layer_sets)
            }) {
                props.push(prop("material", ResolvedValue::Identifier(m)));
            }
            let connected: Vec<String> = connects
                .get(&id)
                .into_iter()
                .flatten()
                .filter(|e| element_types.contains_key(e))
                .map(|e| bml_id(*e))
                .collect();
            props.extend(extra);
            if !connected.is_empty() {
                props.push(prop("connects", ResolvedValue::EnumList(connected)));
            }
            props.extend(
                property_map
                    .get(&id)
                    .into_iter()
                    .flatten()
                    .filter(|p| fits_schema(et, p))
                    .cloned(),
            );
            ResolvedElement {
                element_type: et,
                id: bml_id(id),
                properties: props,
                children,
                span: dummy_span(),
                curves,
            }
        };

    if buildings.is_empty() {
        buildings.push((
            0,
            ResolvedBuilding {
                name: "Building".to_string(),
                properties: Vec::new(),
                storeys: Vec::new(),
                spaces: Vec::new(),
                elements: Vec::new(),
                span: dummy_span(),
            },
        ));
    }

    // Spatial container of an element or space: its storey, building or site,
    // plus the space it is in. Elements that are in none go to the first storey,
    // else the first building.
    let storey_index: HashMap<u64, usize> = storeys
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (*id, i))
        .collect();
    let building_index: HashMap<u64, usize> = buildings
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (*id, i))
        .collect();
    let space_index: HashMap<u64, usize> = spaces
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (*id, i))
        .collect();
    let place_of = |c: &u64| {
        storey_index
            .get(c)
            .map(|&i| Place::Storey(i))
            .or_else(|| building_index.get(c).map(|&i| Place::Building(i)))
            .or_else(|| site_ids.contains(c).then_some(Place::Site))
    };
    let space_place = |space: u64| whole.get(&space).and_then(place_of);
    let locate = |mut id: u64| -> (Option<Place>, Option<usize>) {
        for _ in 0..16 {
            if let Some(c) = container.get(&id) {
                if let Some(p) = place_of(c) {
                    return (Some(p), None);
                }
                if let Some(&sp) = space_index.get(c) {
                    return (space_place(*c), Some(sp));
                }
                return (None, None);
            }
            match whole.get(&id) {
                Some(&w) => id = w,
                None => break,
            }
        }
        (None, None)
    };
    let fallback = if storeys.is_empty() {
        Place::Building(0)
    } else {
        Place::Storey(0)
    };

    // Openings and the fillers BML allows as children (a wall's doors and
    // windows) are written as children of the host. Other fillers (a sleeve,
    // a skylight), and a door of an upper floor in a multi-storey wall, stay in
    // their own container (or, without one, the host's) and name their opening.
    let mut hosted: HashSet<u64> = HashSet::new();
    let mut filled: HashMap<u64, (u64, Option<Place>)> = HashMap::new(); // filler -> opening, host place
    for (host, openings) in &voids {
        if !element_types.contains_key(host) {
            continue;
        }
        let host_place = locate(*host).0;
        for o in openings
            .iter()
            .filter(|o| element_types.get(o) == Some(&ElementType::Opening))
        {
            hosted.insert(*o);
            for f in fills.get(o).into_iter().flatten() {
                let nests = element_types
                    .get(f)
                    .is_some_and(|t| valid_child_types(element_types[host]).contains(t));
                let own = locate(*f).0;
                if nests && (own.is_none() || own == host_place) {
                    hosted.insert(*f);
                } else {
                    filled.insert(*f, (*o, host_place));
                }
            }
        }
    }

    let mut space_elements: Vec<Vec<ResolvedElement>> = vec![Vec::new(); spaces.len()];
    let mut placed: HashMap<Place, (Vec<ResolvedSpace>, Vec<ResolvedElement>)> = HashMap::new();
    for &(id, _) in &elements {
        if hosted.contains(&id) {
            continue;
        }
        let mut children = Vec::new();
        for o in voids.get(&id).into_iter().flatten() {
            if element_types.get(o) != Some(&ElementType::Opening) {
                continue;
            }
            children.push(build(*o, Vec::new(), Vec::new(), false));
            for f in fills.get(o).into_iter().flatten() {
                if element_types.contains_key(f) && hosted.contains(f) {
                    let opening = prop("opening", ResolvedValue::Identifier(bml_id(*o)));
                    children.push(build(*f, vec![opening], Vec::new(), false));
                }
            }
        }
        let part_of = whole
            .get(&id)
            .filter(|w| element_types.contains_key(w))
            .map(|w| prop("part_of", ResolvedValue::Identifier(bml_id(*w))));
        let opening = filled
            .get(&id)
            .map(|(o, _)| prop("opening", ResolvedValue::Identifier(bml_id(*o))));
        let element = build(
            id,
            part_of.into_iter().chain(opening).collect(),
            children,
            true,
        );
        match locate(id) {
            (_, Some(sp)) => space_elements[sp].push(element),
            (p, None) => placed
                .entry(
                    p.or_else(|| filled.get(&id).and_then(|f| f.1))
                        .unwrap_or(fallback),
                )
                .or_default()
                .1
                .push(element),
        }
    }

    for (i, (id, mut space)) in spaces.into_iter().enumerate() {
        space.elements = std::mem::take(&mut space_elements[i]);
        let p = space_place(id).unwrap_or(fallback);
        placed.entry(p).or_default().0.push(space);
    }
    let mut take = |p: Place| placed.remove(&p).unwrap_or_default();

    for (i, (_, storey)) in storeys.iter_mut().enumerate() {
        (storey.spaces, storey.elements) = take(Place::Storey(i));
    }
    for (i, (_, building)) in buildings.iter_mut().enumerate() {
        (building.spaces, building.elements) = take(Place::Building(i));
    }
    // Each storey goes to the building it is aggregated in, else the first.
    for (id, storey) in storeys {
        let b = whole
            .get(&id)
            .and_then(|w| building_index.get(w))
            .copied()
            .unwrap_or(0);
        buildings[b].1.storeys.push(storey);
    }
    let (site_name, site_props) = site.unwrap_or(("Site".to_string(), Vec::new()));
    let (site_spaces, site_elements) = take(Place::Site);
    let sites = vec![ResolvedSite {
        name: site_name,
        implicit: false,
        properties: site_props,
        buildings: buildings.into_iter().map(|(_, b)| b).collect(),
        spaces: site_spaces,
        elements: site_elements,
        span: dummy_span(),
    }];

    let mut seen_material_ids = HashSet::new();
    materials.retain(|m| seen_material_ids.insert(m.id.clone()));
    materials.extend(layer_sets.into_values());

    Ok(SemanticModel {
        sites,
        materials,
        asset_libraries: Vec::new(),
    })
}

/// BML element type for an IFC class. Products without a BML type of their own
/// become `Proxy` (the class is kept as `ifc_class`); `None` for everything else.
fn element_type_for(class: &str, text: &str) -> Option<ElementType> {
    let base = class
        .strip_suffix("STANDARDCASE")
        .or_else(|| class.strip_suffix("ELEMENTEDCASE"))
        .unwrap_or(class);
    Some(match base {
        "IFCWALL" => ElementType::Wall,
        "IFCSLAB" => ElementType::Slab,
        "IFCCOLUMN" => ElementType::Column,
        "IFCBEAM" => ElementType::Beam,
        "IFCWINDOW" => ElementType::Window,
        "IFCDOOR" => ElementType::Door,
        "IFCROOF" => ElementType::Roof,
        "IFCSTAIR" => ElementType::Stair,
        "IFCRAILING" => ElementType::Railing,
        "IFCRAMP" => ElementType::Ramp,
        "IFCFURNITURE" | "IFCFURNISHINGELEMENT" => ElementType::Furniture,
        "IFCTRANSPORTELEMENT" => ElementType::Elevator,
        "IFCBUILDINGELEMENTPROXY" => ElementType::Proxy,
        "IFCMEMBER" => ElementType::Member,
        "IFCPLATE" => ElementType::Plate,
        "IFCCOVERING" => ElementType::Covering,
        "IFCFOOTING" => ElementType::Footing,
        // IFCOPENINGSTANDARDCASE strips to IFCOPENING
        "IFCOPENING" | "IFCOPENINGELEMENT" => ElementType::Opening,
        "IFCGRID" => ElementType::Grid,
        "IFCCURTAINWALL" => ElementType::Proxy,
        "IFCSITE"
        | "IFCBUILDING"
        | "IFCBUILDINGSTOREY"
        | "IFCSPACE"
        | "IFCANNOTATION"
        | "IFCVIRTUALELEMENT"
        | "IFCDISTRIBUTIONPORT" => return None,
        _ if base.starts_with("IFCREL") => return None,
        // Any other product with a body: GlobalId, ..., placement (5), shape (6).
        _ => {
            // Cheap reject first: most entities are geometry without a GlobalId.
            let open = text.find('(')?;
            if text.as_bytes().get(open + 1) != Some(&b'\'')
                || text.as_bytes().get(open + 24) != Some(&b'\'')
            {
                return None;
            }
            let a = args_of(text);
            let is_ref = |i: usize| a.get(i).is_some_and(|s| s.trim().starts_with('#'));
            if a.first().is_some_and(|g| g.trim().len() == 24) && is_ref(5) && is_ref(6) {
                ElementType::Proxy
            } else {
                return None;
            }
        }
    })
}

/// Whether a property set value has the type BML expects for that key, so
/// e.g. an empty `AcousticRating` label does not fail to compile. Keys BML
/// does not know are kept.
fn fits_schema(et: ElementType, p: &ResolvedProperty) -> bool {
    use crate::semantic::type_check::ExpectedType as T;
    let Some(d) = get_descriptor(et).get_property(&p.key) else {
        return true;
    };
    match (d.expected_type, &p.value) {
        (
            T::Length | T::Area | T::Thermal | T::Acoustic | T::Density | T::BareNumber,
            ResolvedValue::Number(_),
        )
        | (T::Boolean, ResolvedValue::Boolean(_))
        | (T::String | T::EnumOrString(_), ResolvedValue::String(_))
        | (T::MaterialRef | T::StoreyRef | T::Identifier, ResolvedValue::Identifier(_)) => true,
        (T::Enum(v) | T::EnumOrString(v), ResolvedValue::Identifier(i)) => v.contains(&i.as_str()),
        _ => false,
    }
}

/// Whether a product has a body: a shape representation other than an axis,
/// footprint, bounding box or similar helper that viewers do not draw.
fn has_body(text: &str, entity_map: &HashMap<u64, &IfcEntity>) -> bool {
    const HELPERS: [&str; 8] = [
        "'Axis'",
        "'FootPrint'",
        "'Box'",
        "'Annotation'",
        "'Profile'",
        "'Clearance'",
        "'CoG'",
        "'Survey'",
    ];
    let args = args_of(text);
    let Some(shape) = args.get(6).map(|r| r.trim()) else {
        return true;
    };
    if shape == "$" {
        return false;
    }
    let Some(pds) = entity_map.get(&parse_ref(shape)) else {
        return true;
    };
    let reps = args_of(&pds.raw_text)
        .last()
        .map(|l| parse_ref_list(l))
        .unwrap_or_default();
    reps.iter().any(|r| {
        entity_map.get(r).is_none_or(|e| {
            !args_of(&e.raw_text)
                .get(1)
                .is_some_and(|id| HELPERS.contains(&id.trim()))
        })
    })
}

/// PredefinedType of an element in snake_case. NOTDEFINED, USERDEFINED (its
/// name is in ObjectType) and the writer's default roof type are left out.
fn predefined_type(class: &str, text: &str) -> Option<String> {
    // IfcGrid has no Tag; PredefinedType follows its three axis lists.
    let i = match class {
        "IFCGRID" => 10,
        _ => 8 + predefined_type_slot(class).1?,
    };
    let args = args_of(text);
    let name = args.get(i)?.trim().strip_prefix('.')?.strip_suffix('.')?;
    if matches!(name, "NOTDEFINED" | "USERDEFINED") || (class == "IFCROOF" && name == "FLAT_ROOF") {
        return None;
    }
    Some(name.to_ascii_lowercase())
}

fn args_of(text: &str) -> Vec<String> {
    parse_entity_args(text).unwrap_or_default()
}

pub(crate) fn prop(key: &str, value: ResolvedValue) -> ResolvedProperty {
    ResolvedProperty {
        key: key.to_string(),
        value,
        span: dummy_span(),
    }
}

/// `guid:` from an IfcRoot entity's GlobalId.
fn guid_props(text: &str) -> Vec<ResolvedProperty> {
    extract_field(text, 0)
        .filter(|g| g.len() == 22)
        .map(|g| prop("guid", ResolvedValue::String(g)))
        .into_iter()
        .collect()
}

fn spatial_name(text: &str, default: &str) -> String {
    extract_name(text)
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// BML identifier from an IFC name: anything outside `[A-Za-z0-9_-]` becomes `_`.
/// Name of the IfcMaterial behind a material select: the material itself, or
/// the first member of a layer, constituent or profile set (or list).
fn material_name(id: u64, entity_map: &HashMap<u64, &IfcEntity>) -> Option<String> {
    let mut id = id;
    for _ in 0..8 {
        let entity = entity_map.get(&id)?;
        let a = args_of(&entity.raw_text);
        let one = |i: usize| a.get(i).map(|s| parse_ref(s)).filter(|&r| r != 0);
        let first = |i: usize| a.get(i).and_then(|s| parse_ref_list(s).first().copied());
        id = match entity.entity_type.as_str() {
            "IFCMATERIAL" => return extract_field(&entity.raw_text, 0),
            "IFCMATERIALLAYERSETUSAGE" | "IFCMATERIALPROFILESETUSAGE" | "IFCMATERIALLAYER" => {
                one(0)?
            }
            "IFCMATERIALLAYERSET" | "IFCMATERIALLIST" => first(0)?,
            "IFCMATERIALCONSTITUENTSET" | "IFCMATERIALPROFILESET" => first(2)?,
            "IFCMATERIALCONSTITUENT" | "IFCMATERIALPROFILE" => one(2)?,
            _ => return None,
        };
    }
    None
}

/// The `material:` id for a material select. A layer set, or its usage, whose
/// layers all name an IfcMaterial becomes a layered `Material:` in `layer_sets`
/// (keyed by set entity); anything else names its first IfcMaterial.
fn material_ref(
    id: u64,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
    materials: &[ResolvedMaterial],
    layer_sets: &mut BTreeMap<u64, ResolvedMaterial>,
) -> Option<String> {
    let entity = entity_map.get(&id)?;
    let set = match entity.entity_type.as_str() {
        "IFCMATERIALLAYERSETUSAGE" => Some(parse_ref(args_of(&entity.raw_text).first()?)),
        "IFCMATERIALLAYERSET" => Some(id),
        _ => None,
    };
    if let Some(set) = set {
        if let Some(m) = layer_sets.get(&set) {
            return Some(m.id.clone());
        }
        if let Some(mut m) = layered_material(set, entity_map, unit_scale) {
            // A set named like one of its materials ("Concrete") must not shadow it.
            make_unique(&mut m, |id| {
                materials
                    .iter()
                    .chain(layer_sets.values())
                    .any(|d| d.id == id)
            });
            let id = m.id.clone();
            layer_sets.insert(set, m);
            return Some(id);
        }
    }
    material_name(id, entity_map).map(|m| sanitize_id(&m))
}

/// Layered `Material:` for an IfcMaterialLayerSet, named after its LayerSetName.
/// `None` when a layer has no IfcMaterial or thickness.
fn layered_material(
    set: u64,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Option<ResolvedMaterial> {
    let text = &entity_map.get(&set)?.raw_text;
    let mut layers = Vec::new();
    for layer in parse_ref_list(args_of(text).first()?) {
        let a = args_of(&entity_map.get(&layer)?.raw_text);
        let material = entity_map.get(&parse_ref(a.first()?))?;
        if material.entity_type != "IFCMATERIAL" {
            return None;
        }
        layers.push(MaterialLayer {
            name: sanitize_id(&extract_field(&material.raw_text, 0)?),
            thickness: a.get(1)?.trim().parse::<f64>().ok()? * unit_scale,
        });
    }
    if layers.is_empty() {
        return None;
    }
    let name = extract_field(text, 1).unwrap_or_else(|| format!("LayerSet_{set}"));
    let mut def = material_def(name);
    def.material_type = MaterialType::Layered { layers };
    Some(def)
}

/// Rename `def` to the first id not `taken` (`id`, `id_2`, `id_3`, ...). Its
/// IFC name survives as `ifc_name` when the id no longer matches it.
pub(crate) fn make_unique(def: &mut ResolvedMaterial, taken: impl Fn(&str) -> bool) {
    let name = def.ifc_name().to_string();
    let base = def.id.clone();
    let mut n = 1;
    while taken(&def.id) {
        n += 1;
        def.id = format!("{base}_{n}");
    }
    if def.id != name && !def.properties.iter().any(|p| p.key == "ifc_name") {
        def.properties
            .push(prop("ifc_name", ResolvedValue::String(name)));
    }
}

/// `Material:` for an IFC material name. The IFC name survives as `ifc_name`
/// when the id had to change it.
pub(crate) fn material_def(name: String) -> ResolvedMaterial {
    let id = sanitize_id(&name);
    let properties = if id == name {
        Vec::new()
    } else {
        vec![prop("ifc_name", ResolvedValue::String(name))]
    };
    ResolvedMaterial {
        id,
        material_type: MaterialType::Homogeneous {
            category: None,
            grade: None,
            density: None,
        },
        properties,
        span: dummy_span(),
    }
}

fn sanitize_id(name: &str) -> String {
    let id: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if id.starts_with(|c: char| c.is_ascii_alphabetic()) {
        id
    } else {
        format!("M_{}", id)
    }
}

// ─── STEP Parsing Helpers ────────────────────────────────────────────

/// The IFC4 class for an IFC2x3 class that IFC4 deleted, as the writer
/// writes IFC4.
fn ifc4_class(class: &str) -> &str {
    match class {
        "IFCELECTRICDISTRIBUTIONPOINT" => "IFCELECTRICDISTRIBUTIONBOARD",
        c => c,
    }
}

/// Spatial container of a decompiled element or space.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Place {
    Site,
    Building(usize),
    Storey(usize),
}

/// Parse all STEP entities from an IFC file into a list.
pub(super) fn parse_all_entities(ifc_content: &str) -> Vec<IfcEntity> {
    let mut entities = Vec::new();

    for statement in entity_statements(ifc_content) {
        let trimmed: &str = &statement;
        let Some(eq_pos) = trimmed.find('=') else {
            continue;
        };

        // Parse entity ID: #123=...
        let id_str = trimmed[1..eq_pos].trim();
        let Ok(id) = id_str.parse::<u64>() else {
            continue;
        };

        let entity_text = trimmed[eq_pos + 1..].trim();
        let Some(paren_pos) = entity_text.find('(') else {
            continue;
        };
        let entity_type = entity_text[..paren_pos].trim().to_string();
        if entity_type.is_empty() {
            continue;
        }

        entities.push(IfcEntity {
            id,
            entity_type,
            raw_text: entity_text.to_string(),
        });
    }

    entities
}

/// STEP statements, each up to and including its `;`, so an entity may span
/// several lines. A `;` or `'` inside a string or `/* */` comment does not count.
pub(crate) fn statements(content: &str) -> impl Iterator<Item = &str> {
    let (mut in_string, mut in_comment, mut prev) = (false, false, '\0');
    content.split_inclusive(move |c| {
        let was_comment = in_comment;
        if in_comment {
            in_comment = !(prev == '*' && c == '/');
        } else if c == '\'' {
            in_string = !in_string;
        } else if c == '*' && prev == '/' && !in_string {
            in_comment = true;
        }
        // A closing `*/` must not pair its `/` with a following `*`
        prev = if was_comment && !in_comment { '\0' } else { c };
        c == ';' && !in_string && !was_comment
    })
}

/// The `#id=ENTITY(...);` statements of a STEP file, trimmed, without leading
/// comments and line breaks (line breaks outside strings are insignificant;
/// STEP strings have none).
pub(crate) fn entity_statements(content: &str) -> impl Iterator<Item = Cow<'_, str>> {
    statements(content).filter_map(|s| {
        let mut t = s.trim();
        while let Some(rest) = t.strip_prefix("/*") {
            t = rest.split_once("*/").map_or("", |(_, r)| r.trim_start());
        }
        if !t.starts_with('#') {
            return None;
        }
        Some(if t.contains(['\n', '\r']) {
            Cow::Owned(t.replace(['\n', '\r'], ""))
        } else {
            Cow::Borrowed(t)
        })
    })
}

/// Scale factor from the project length unit to metres: the LENGTHUNIT of the
/// `IfcUnitAssignment`, an `IfcSIUnit` (with prefix) or an
/// `IfcConversionBasedUnit` such as FOOT. Defaults to metres.
fn detect_length_unit_scale(entity_map: &HashMap<u64, &IfcEntity>) -> f64 {
    fn si(text: &str) -> f64 {
        let a = args_of(text);
        match a.get(2).map(|p| p.trim()) {
            Some(".MILLI.") => 0.001,
            Some(".CENTI.") => 0.01,
            Some(".DECI.") => 0.1,
            Some(".KILO.") => 1000.0,
            _ => 1.0,
        }
    }
    let scale = |id: u64| -> Option<f64> {
        let e = entity_map.get(&id)?;
        if !e.raw_text.contains(".LENGTHUNIT.") {
            return None;
        }
        match e.entity_type.as_str() {
            "IFCSIUNIT" => Some(si(&e.raw_text)),
            // IFCCONVERSIONBASEDUNIT(dims, .LENGTHUNIT., 'FOOT', #measure)
            // IFCMEASUREWITHUNIT(IFCRATIOMEASURE(0.3048), #siunit)
            "IFCCONVERSIONBASEDUNIT" => {
                let m = entity_map.get(&parse_ref(args_of(&e.raw_text).get(3)?))?;
                let a = args_of(&m.raw_text);
                let factor: f64 = extract_ifc_value_inner(a.first()?)
                    .unwrap_or_else(|| a[0].clone())
                    .trim()
                    .trim_end_matches('.')
                    .parse()
                    .ok()?;
                let base = entity_map
                    .get(&parse_ref(a.get(1)?))
                    .map_or(1.0, |u| si(&u.raw_text));
                Some(factor * base)
            }
            _ => None,
        }
    };
    // File order, so a second assignment (e.g. a library's) never wins by hash order.
    let mut assignments: Vec<(&u64, &&IfcEntity)> = entity_map
        .iter()
        .filter(|(_, e)| e.entity_type == "IFCUNITASSIGNMENT")
        .collect();
    assignments.sort_by_key(|(id, _)| **id);
    assignments
        .into_iter()
        .flat_map(|(_, e)| parse_ref_list(args_of(&e.raw_text).first().map_or("", |s| s)))
        .find_map(scale)
        .unwrap_or(1.0)
}

// ─── Wall Geometry Extraction Helpers ─────────────────────────────────

/// Parse the arguments inside the outermost parentheses of an IFC entity's raw text.
/// Returns the top-level arguments split by comma, respecting nesting.
pub(super) fn parse_entity_args(raw_text: &str) -> Option<Vec<String>> {
    let paren_start = raw_text.find('(')?;
    let paren_end = raw_text.rfind(')')?;
    if paren_start >= paren_end {
        return None;
    }
    let args_str = &raw_text[paren_start + 1..paren_end];
    Some(
        split_ifc_args(args_str)
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
    )
}

/// Parse a coordinate list like `((1.0,2.0,3.0),(4.0,5.0,6.0))` into flat floats.
pub(super) fn parse_coordinate_list(s: &str) -> Vec<f64> {
    let mut result = Vec::new();
    // Extract all numeric tokens from the nested parens
    let cleaned = s.replace(['(', ')'], " ");
    for token in cleaned.split(',') {
        let t = token.trim();
        if t.is_empty() || t == "$" {
            continue;
        }
        if let Ok(v) = t.trim_end_matches('.').parse::<f64>() {
            result.push(v);
        }
    }
    result
}

/// Resolve an IFCCARTESIANPOINT entity to (x, y, z).
/// 2D points return z=0.
pub(super) fn resolve_cartesian_point(
    entity_id: u64,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Option<(f64, f64, f64)> {
    let entity = entity_map.get(&entity_id)?;
    if entity.entity_type != "IFCCARTESIANPOINT" {
        return None;
    }
    let args = parse_entity_args(&entity.raw_text)?;
    if args.is_empty() {
        return None;
    }
    let coords = parse_coordinate_list(&args[0]);
    match coords.len() {
        2 => Some((coords[0] * unit_scale, coords[1] * unit_scale, 0.0)),
        3 => Some((
            coords[0] * unit_scale,
            coords[1] * unit_scale,
            coords[2] * unit_scale,
        )),
        _ => None,
    }
}

/// Resolve IFCAXIS2PLACEMENT3D location point.
pub(super) fn resolve_axis2placement3d_location(
    entity_id: u64,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Option<(f64, f64, f64)> {
    let entity = entity_map.get(&entity_id)?;
    if entity.entity_type != "IFCAXIS2PLACEMENT3D" {
        return None;
    }
    let args = parse_entity_args(&entity.raw_text)?;
    if args.is_empty() {
        return None;
    }
    let point_id = parse_ref(args[0].trim());
    resolve_cartesian_point(point_id, entity_map, unit_scale)
}

/// Resolve an IFCAXIS2PLACEMENT3D to get the axis direction (Z) and ref direction (X).
/// Returns (axis_direction, ref_direction) as unit vectors, or None.
pub(super) fn resolve_axis2placement3d_directions(
    entity_id: u64,
    entity_map: &HashMap<u64, &IfcEntity>,
) -> Option<DirectionPair> {
    let entity = entity_map.get(&entity_id)?;
    if entity.entity_type != "IFCAXIS2PLACEMENT3D" {
        return None;
    }
    let args = parse_entity_args(&entity.raw_text)?;
    // args[1] = axis (Z direction), args[2] = ref direction (X direction)
    let axis = if args.len() > 1 && args[1].trim() != "$" {
        resolve_direction(parse_ref(args[1].trim()), entity_map)?
    } else {
        (0.0, 0.0, 1.0) // default Z-up
    };
    let ref_dir = if args.len() > 2 && args[2].trim() != "$" {
        resolve_direction(parse_ref(args[2].trim()), entity_map)?
    } else {
        (1.0, 0.0, 0.0) // default X
    };
    Some((axis, ref_dir))
}

/// Resolve an IFCDIRECTION entity to (x, y, z).
pub(super) fn resolve_direction(
    entity_id: u64,
    entity_map: &HashMap<u64, &IfcEntity>,
) -> Option<(f64, f64, f64)> {
    let entity = entity_map.get(&entity_id)?;
    if entity.entity_type != "IFCDIRECTION" {
        return None;
    }
    let args = parse_entity_args(&entity.raw_text)?;
    if args.is_empty() {
        return None;
    }
    let coords = parse_coordinate_list(&args[0]);
    match coords.len() {
        2 => Some((coords[0], coords[1], 0.0)),
        3 => Some((coords[0], coords[1], coords[2])),
        _ => None,
    }
}

/// Resolve IFCPOLYLINE to 2D points by following IFCCARTESIANPOINT references.
fn resolve_polyline_points(
    entity_id: u64,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Option<Vec<(f64, f64)>> {
    let entity = entity_map.get(&entity_id)?;
    if entity.entity_type != "IFCPOLYLINE" {
        return None;
    }
    let args = parse_entity_args(&entity.raw_text)?;
    if args.is_empty() {
        return None;
    }
    // The single argument is a list of point refs: (#pt1,#pt2,...)
    let ref_ids = parse_ref_list(args[0].trim());
    let mut points = Vec::new();
    for ref_id in ref_ids {
        if let Some((x, y, _z)) = resolve_cartesian_point(ref_id, entity_map, unit_scale) {
            points.push((x, y));
        }
    }
    if points.is_empty() {
        None
    } else {
        Some(points)
    }
}

/// Resolve IFCCARTESIANPOINTLIST2D or IFCCARTESIANPOINTLIST3D to 2D points.
fn resolve_point_list(
    entity_id: u64,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Option<Vec<(f64, f64)>> {
    let entity = entity_map.get(&entity_id)?;
    let is_2d = entity.entity_type == "IFCCARTESIANPOINTLIST2D";
    let is_3d = entity.entity_type == "IFCCARTESIANPOINTLIST3D";
    if !is_2d && !is_3d {
        return None;
    }
    let args = parse_entity_args(&entity.raw_text)?;
    if args.is_empty() {
        return None;
    }
    let coords = parse_coordinate_list(&args[0]);
    let stride = if is_3d { 3 } else { 2 };
    let mut points = Vec::new();
    for chunk in coords.chunks(stride) {
        if chunk.len() >= 2 {
            points.push((chunk[0] * unit_scale, chunk[1] * unit_scale));
        }
    }
    if points.is_empty() {
        None
    } else {
        Some(points)
    }
}

/// Find axis endpoints from a product's IFCPRODUCTDEFINITIONSHAPE.
/// Looks for the 'Axis' IFCSHAPEREPRESENTATION and extracts 2 endpoints.
/// Returns (x1, y1, x2, y2) in local coordinates (scaled to metres).
fn find_axis_endpoints(
    repr_id: u64,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Option<(f64, f64, f64, f64)> {
    let repr_entity = entity_map.get(&repr_id)?;
    if repr_entity.entity_type != "IFCPRODUCTDEFINITIONSHAPE" {
        return None;
    }
    let repr_args = parse_entity_args(&repr_entity.raw_text)?;
    if repr_args.len() < 3 {
        return None;
    }
    // Last arg is the list of shape representations
    let shape_repr_ids = parse_ref_list(repr_args.last()?.trim());

    for sr_id in shape_repr_ids {
        let sr_entity = entity_map.get(&sr_id)?;
        if sr_entity.entity_type != "IFCSHAPEREPRESENTATION" {
            continue;
        }
        // Check if this is the 'Axis' representation
        if !sr_entity.raw_text.contains("'Axis'") {
            continue;
        }
        let sr_args = parse_entity_args(&sr_entity.raw_text)?;
        if sr_args.len() < 4 {
            continue;
        }
        // sr_args[3] = list of geometry items
        let item_ids = parse_ref_list(sr_args[3].trim());
        for item_id in item_ids {
            let item = entity_map.get(&item_id)?;
            match item.entity_type.as_str() {
                "IFCPOLYLINE" => {
                    let points = resolve_polyline_points(item_id, entity_map, unit_scale)?;
                    if points.len() >= 2 {
                        let first = points[0];
                        let last = points[points.len() - 1];
                        return Some((first.0, first.1, last.0, last.1));
                    }
                }
                "IFCINDEXEDPOLYCURVE" => {
                    let curve_args = parse_entity_args(&item.raw_text)?;
                    if curve_args.is_empty() {
                        continue;
                    }
                    let ptlist_id = parse_ref(curve_args[0].trim());
                    let points = resolve_point_list(ptlist_id, entity_map, unit_scale)?;
                    if points.len() >= 2 {
                        let first = points[0];
                        let last = points[points.len() - 1];
                        return Some((first.0, first.1, last.0, last.1));
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Resolve width from an IFC profile definition.
/// Supports IFCRECTANGLEPROFILEDEF (x_dim), and IFCARBITRARYCLOSEDPROFILEDEF /
/// IFCARBITRARYPROFILEDEFWITHVOIDS (bounding box of outer curve).
///
/// Resolve points from a curve entity (IFCPOLYLINE or IFCINDEXEDPOLYCURVE).
fn resolve_curve_points(
    entity_id: u64,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Option<Vec<(f64, f64)>> {
    let entity = entity_map.get(&entity_id)?;
    match entity.entity_type.as_str() {
        "IFCPOLYLINE" => resolve_polyline_points(entity_id, entity_map, unit_scale),
        "IFCINDEXEDPOLYCURVE" => {
            let args = parse_entity_args(&entity.raw_text)?;
            if args.is_empty() {
                return None;
            }
            let ptlist_id = parse_ref(args[0].trim());
            resolve_point_list(ptlist_id, entity_map, unit_scale)
        }
        _ => None,
    }
}

/// Resolve profile bounding box extents (x_extent, y_extent) in metres.
fn resolve_profile_extents(
    entity_id: u64,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Option<(f64, f64)> {
    let entity = entity_map.get(&entity_id)?;
    match entity.entity_type.as_str() {
        "IFCRECTANGLEPROFILEDEF" => {
            let args = parse_entity_args(&entity.raw_text)?;
            if args.len() >= 5 {
                let x_dim: f64 = args[3].trim().trim_end_matches('.').parse().ok()?;
                let y_dim: f64 = args[4].trim().trim_end_matches('.').parse().ok()?;
                return Some((x_dim * unit_scale, y_dim * unit_scale));
            }
            None
        }
        "IFCARBITRARYCLOSEDPROFILEDEF" | "IFCARBITRARYPROFILEDEFWITHVOIDS" => {
            let args = parse_entity_args(&entity.raw_text)?;
            if args.len() < 3 {
                return None;
            }
            let curve_id = parse_ref(args[2].trim());
            let points = resolve_curve_points(curve_id, entity_map, unit_scale)?;
            if points.is_empty() {
                return None;
            }
            let min_x = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
            let max_x = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
            let min_y = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
            let max_y = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
            Some((max_x - min_x, max_y - min_y))
        }
        _ => None,
    }
}

/// Resolve an IFCEXTRUDEDAREASOLID to get (depth, profile_x_extent, profile_y_extent).
fn resolve_extruded_solid(
    entity_id: u64,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Option<(f64, f64, f64)> {
    let entity = entity_map.get(&entity_id)?;
    if entity.entity_type != "IFCEXTRUDEDAREASOLID" {
        return None;
    }
    // IFCEXTRUDEDAREASOLID(#profile, #position, #direction, depth)
    let args = parse_entity_args(&entity.raw_text)?;
    if args.len() < 4 {
        return None;
    }
    let profile_id = parse_ref(args[0].trim());
    let depth: f64 = args[3].trim().trim_end_matches('.').parse().ok()?;
    let (x_extent, y_extent) = resolve_profile_extents(profile_id, entity_map, unit_scale)?;
    Some((depth * unit_scale, x_extent, y_extent))
}

/// Find wall extrusion data from a product's IFCPRODUCTDEFINITIONSHAPE.
/// Looks for the 'Body' IFCSHAPEREPRESENTATION with IFCEXTRUDEDAREASOLID items.
///
/// For layered walls (multiple extruded solids), each solid represents one material
/// layer. The extrusion depth is the layer thickness and the profile contains the
/// wall's cross-section (X = wall length, Y = wall height). We sum the extrusion
/// depths as total wall thickness and use the profile Y extent as wall height.
///
/// For single-solid walls, the extrusion depth is the wall height and the profile
/// Y extent is the wall thickness (standard case).
///
/// Returns (height, thickness) in metres.
fn find_wall_extrusion(
    repr_id: u64,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
    axis_length: Option<f64>,
) -> Option<(f64, f64)> {
    let repr_entity = entity_map.get(&repr_id)?;
    if repr_entity.entity_type != "IFCPRODUCTDEFINITIONSHAPE" {
        return None;
    }
    let repr_args = parse_entity_args(&repr_entity.raw_text)?;
    if repr_args.len() < 3 {
        return None;
    }
    let shape_repr_ids = parse_ref_list(repr_args.last()?.trim());

    for sr_id in &shape_repr_ids {
        let Some(sr_entity) = entity_map.get(sr_id) else {
            continue;
        };
        if sr_entity.entity_type != "IFCSHAPEREPRESENTATION" {
            continue;
        }
        if !sr_entity.raw_text.contains("'Body'") {
            continue;
        }
        let sr_args = parse_entity_args(&sr_entity.raw_text);
        let Some(sr_args) = sr_args else { continue };
        if sr_args.len() < 4 {
            continue;
        }
        let item_ids = parse_ref_list(sr_args[3].trim());

        // Collect all extruded solids (layered walls have multiple)
        // Each solid: (depth, profile_x_extent, profile_y_extent)
        let mut solids: Vec<(f64, f64, f64)> = Vec::new();
        for item_id in &item_ids {
            if let Some(result) = resolve_extruded_solid(*item_id, entity_map, unit_scale) {
                solids.push(result);
            }
            // Try IFCBOOLEANCLIPPINGRESULT → first operand
            if solids.is_empty()
                && let Some(entity) = entity_map.get(item_id)
                && entity.entity_type == "IFCBOOLEANCLIPPINGRESULT"
                && let Some(args) = parse_entity_args(&entity.raw_text)
                && args.len() >= 2
            {
                let first_operand = parse_ref(args[1].trim());
                if let Some(result) = resolve_extruded_solid(first_operand, entity_map, unit_scale)
                {
                    solids.push(result);
                }
            }
        }

        if solids.is_empty() {
            continue;
        }

        if solids.len() == 1 {
            // Single solid: depth = height, profile_y = thickness (standard)
            let (depth, _x, y) = solids[0];
            return Some((depth, y));
        }

        // Multiple solids (layered wall). Two Revit patterns exist:
        //
        // Pattern A: extrusion depth varies per layer (= layer thickness),
        //   profile contains wall elevation (length × height).
        //   → height = non-length profile extent, thickness = sum of depths.
        //
        // Pattern B: all layers share the same extrusion depth (= wall height),
        //   profile X = layer thickness, profile Y = wall length.
        //   → height = shared depth, thickness = sum of min profile extents.
        //
        // Heuristic: if all depths are approximately equal (within 5%), it's pattern B.
        // Minor differences occur when some layers protrude slightly (e.g., studs past plasterboard).
        let max_depth = solids.iter().map(|(d, _, _)| *d).fold(0.0_f64, f64::max);
        let min_depth = solids
            .iter()
            .map(|(d, _, _)| *d)
            .fold(f64::INFINITY, f64::min);
        let all_same_depth = max_depth > 0.0 && (max_depth - min_depth) / max_depth < 0.05;

        if all_same_depth {
            // Pattern B: depth = wall height, smaller profile extent = layer thickness
            let wall_height = solids[0].0;
            let total_thickness: f64 = solids.iter().map(|(_, x, y)| x.min(*y)).sum();
            return Some((wall_height, total_thickness));
        }

        // Pattern A: depth = layer thickness, profile has wall elevation.
        // Use axis_length to distinguish wall-length extent from wall-height extent.
        // For each solid, pick the profile extent that is NOT close to the axis length.
        let total_thickness: f64 = solids.iter().map(|(depth, _, _)| depth).sum();

        // Find wall height from the largest solid's profile (skip small embedded solids)
        let main_solid = solids.iter().max_by(|a, b| {
            (a.1.max(a.2))
                .partial_cmp(&(b.1.max(b.2)))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if let Some(&(_depth, x_ext, y_ext)) = main_solid {
            let wall_height = if let Some(axis_len) = axis_length {
                // The extent closer to axis_length is the wall length; the other is height
                let x_diff = (x_ext - axis_len).abs();
                let y_diff = (y_ext - axis_len).abs();
                if x_diff < y_diff { y_ext } else { x_ext }
            } else {
                // Fallback: smaller extent is height (wall length > wall height typically)
                x_ext.min(y_ext)
            };
            return Some((wall_height, total_thickness));
        }

        return Some((0.0, total_thickness));
    }

    // Fallback: try tessellated body representation
    // Tessellated walls have IFCPOLYGONALFACESET items with IFCCARTESIANPOINTLIST3D.
    // Z extent = wall height, and the smaller of X/Y extents (perpendicular to axis) = thickness.
    for sr_id in &shape_repr_ids {
        let Some(sr_entity) = entity_map.get(sr_id) else {
            continue;
        };
        if sr_entity.entity_type != "IFCSHAPEREPRESENTATION" {
            continue;
        }
        if !sr_entity.raw_text.contains("'Body'") || !sr_entity.raw_text.contains("'Tessellation'")
        {
            continue;
        }
        let sr_args = parse_entity_args(&sr_entity.raw_text)?;
        if sr_args.len() < 4 {
            continue;
        }
        let item_ids = parse_ref_list(sr_args[3].trim());

        // Collect Z, X, Y bounds from all point lists
        let mut z_min = f64::INFINITY;
        let mut z_max = f64::NEG_INFINITY;
        let mut x_min = f64::INFINITY;
        let mut x_max = f64::NEG_INFINITY;
        let mut y_min = f64::INFINITY;
        let mut y_max = f64::NEG_INFINITY;
        let mut found_points = false;

        for item_id in &item_ids {
            let Some(faceset) = entity_map.get(item_id) else {
                continue;
            };
            if faceset.entity_type != "IFCPOLYGONALFACESET" {
                continue;
            }
            let faceset_args = parse_entity_args(&faceset.raw_text);
            let Some(fa) = faceset_args else { continue };
            if fa.is_empty() {
                continue;
            }
            let ptlist_id = parse_ref(fa[0].trim());
            let Some(ptlist) = entity_map.get(&ptlist_id) else {
                continue;
            };
            if ptlist.entity_type != "IFCCARTESIANPOINTLIST3D" {
                continue;
            }

            // Parse 3D point triples from the raw text
            // Format: IFCCARTESIANPOINTLIST3D(((x,y,z),(x,y,z),...))
            let raw = &ptlist.raw_text;
            if let Some(start) = raw.find("(((") {
                let data = &raw[start + 2..]; // skip outer "(("
                for triple in data.split("),(") {
                    let clean = triple.trim_matches(|c: char| c == '(' || c == ')');
                    let coords: Vec<&str> = clean.split(',').collect();
                    if coords.len() >= 3
                        && let (Ok(x), Ok(y), Ok(z)) = (
                            coords[0].trim().parse::<f64>(),
                            coords[1].trim().parse::<f64>(),
                            coords[2].trim().trim_end_matches('.').parse::<f64>(),
                        )
                    {
                        let x = x * unit_scale;
                        let y = y * unit_scale;
                        let z = z * unit_scale;
                        x_min = x_min.min(x);
                        x_max = x_max.max(x);
                        y_min = y_min.min(y);
                        y_max = y_max.max(y);
                        z_min = z_min.min(z);
                        z_max = z_max.max(z);
                        found_points = true;
                    }
                }
            }
        }

        if found_points {
            let height = z_max - z_min;
            let x_ext = x_max - x_min;
            let y_ext = y_max - y_min;
            // The axis-aligned extent matching the wall axis is the length;
            // the perpendicular extent is the total wall thickness (sum of layers).
            let thickness = if let Some(axis_len) = axis_length {
                let x_diff = (x_ext - axis_len).abs();
                let y_diff = (y_ext - axis_len).abs();
                if x_diff < y_diff { y_ext } else { x_ext }
            } else {
                x_ext.min(y_ext)
            };
            if height > 0.001 {
                return Some((height, thickness));
            }
        }
    }

    None
}

/// Transform local axis endpoints by the wall's own placement.
/// The wall's IFCLOCALPLACEMENT contains a relative IFCAXIS2PLACEMENT3D
/// with location + optional rotation (axis + ref_direction).
fn transform_axis_by_placement(
    placement_id: u64,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> (f64, f64, f64, f64) {
    let Some(entity) = entity_map.get(&placement_id) else {
        return (x1, y1, x2, y2);
    };
    if entity.entity_type != "IFCLOCALPLACEMENT" {
        return (x1, y1, x2, y2);
    }
    let Some(args) = parse_entity_args(&entity.raw_text) else {
        return (x1, y1, x2, y2);
    };
    if args.len() < 2 {
        return (x1, y1, x2, y2);
    }

    let axis2_id = parse_ref(args[1].trim());

    // Get translation
    let (tx, ty, _tz) = resolve_axis2placement3d_location(axis2_id, entity_map, unit_scale)
        .unwrap_or((0.0, 0.0, 0.0));

    // Get rotation (ref_direction = local X axis)
    let (_axis_dir, ref_dir) = resolve_axis2placement3d_directions(axis2_id, entity_map)
        .unwrap_or(((0.0, 0.0, 1.0), (1.0, 0.0, 0.0)));

    // 2D rotation: ref_dir gives the local X axis direction in the parent frame
    let rx = ref_dir.0;
    let ry = ref_dir.1;

    // Transform: p_parent = translation + rotation * p_local
    let nx1 = tx + rx * x1 - ry * y1;
    let ny1 = ty + ry * x1 + rx * y1;
    let nx2 = tx + rx * x2 - ry * y2;
    let ny2 = ty + ry * x2 + rx * y2;

    (nx1, ny1, nx2, ny2)
}

/// Extract wall geometry: axis (from/to), height, and thickness.
fn extract_wall_geometry(
    entity: &IfcEntity,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Vec<ResolvedProperty> {
    let mut props = Vec::new();
    let span = dummy_span();

    let Some(args) = parse_entity_args(&entity.raw_text) else {
        return props;
    };
    // IFCWALL(guid, owner, name, desc, type, #placement, #repr, tag, predefined_type)
    if args.len() < 7 {
        return props;
    }

    let placement_id = parse_ref(args[5].trim());
    let repr_id = parse_ref(args[6].trim());

    // Extract axis endpoints (local coordinates)
    if let Some((x1, y1, x2, y2)) = find_axis_endpoints(repr_id, entity_map, unit_scale) {
        // Transform by wall placement to get storey-relative coordinates
        let (tx1, ty1, tx2, ty2) =
            transform_axis_by_placement(placement_id, x1, y1, x2, y2, entity_map, unit_scale);

        // Round to 3 decimal places for clean output
        let round3 = |v: f64| (v * 1000.0).round() / 1000.0;

        props.push(ResolvedProperty {
            key: "from".to_string(),
            value: ResolvedValue::Axis {
                from: (round3(tx1), round3(ty1)),
                to: (round3(tx2), round3(ty2)),
                via: None,
                z: None,
            },
            span,
        });
    }

    // Compute axis length for disambiguation of layered wall profile extents
    let axis_length = find_axis_endpoints(repr_id, entity_map, unit_scale)
        .map(|(x1, y1, x2, y2)| ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt());

    // Extract height and thickness from body extrusion
    if let Some((height, thickness)) =
        find_wall_extrusion(repr_id, entity_map, unit_scale, axis_length)
    {
        let round3 = |v: f64| (v * 1000.0).round() / 1000.0;

        props.push(ResolvedProperty {
            key: "height".to_string(),
            value: ResolvedValue::Number(round3(height)),
            span,
        });
        props.push(ResolvedProperty {
            key: "thickness".to_string(),
            value: ResolvedValue::Number(round3(thickness)),
            span,
        });
    }

    props
}

/// Resolve an IFCEXTRUDEDAREASOLID to get (height, profile_type, profile_params).
/// - IFCRECTANGLEPROFILEDEF → ("rectangle", [x_dim, y_dim])
/// - IFCCIRCLEPROFILEDEF → ("circular", [diameter])
/// - IFCARBITRARYCLOSEDPROFILEDEF → bounding-box fallback ("rectangle", [width, depth])
fn resolve_profile_and_height(
    entity_id: u64,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Option<(f64, String, Vec<f64>)> {
    let entity = entity_map.get(&entity_id)?;
    if entity.entity_type != "IFCEXTRUDEDAREASOLID" {
        return None;
    }
    // IFCEXTRUDEDAREASOLID(#profile, #position, #direction, depth)
    let args = parse_entity_args(&entity.raw_text)?;
    if args.len() < 4 {
        return None;
    }
    let profile_id = parse_ref(args[0].trim());
    let depth: f64 = args[3].trim().trim_end_matches('.').parse().ok()?;
    let height = depth * unit_scale;

    let profile_entity = entity_map.get(&profile_id)?;
    match profile_entity.entity_type.as_str() {
        "IFCRECTANGLEPROFILEDEF" => {
            // IFCRECTANGLEPROFILEDEF(.AREA., name, #position, x_dim, y_dim)
            let pargs = parse_entity_args(&profile_entity.raw_text)?;
            if pargs.len() >= 5 {
                let x_dim: f64 = pargs[3].trim().trim_end_matches('.').parse().ok()?;
                let y_dim: f64 = pargs[4].trim().trim_end_matches('.').parse().ok()?;
                Some((
                    height,
                    "rectangle".to_string(),
                    vec![x_dim * unit_scale, y_dim * unit_scale],
                ))
            } else {
                None
            }
        }
        "IFCCIRCLEPROFILEDEF" => {
            // IFCCIRCLEPROFILEDEF(.AREA., name, #position, radius)
            let pargs = parse_entity_args(&profile_entity.raw_text)?;
            if pargs.len() >= 4 {
                let radius: f64 = pargs[3].trim().trim_end_matches('.').parse().ok()?;
                Some((
                    height,
                    "circular".to_string(),
                    vec![radius * 2.0 * unit_scale],
                ))
            } else {
                None
            }
        }
        "IFCARBITRARYCLOSEDPROFILEDEF" | "IFCARBITRARYPROFILEDEFWITHVOIDS" => {
            // Fallback: compute bounding box of outer curve
            let pargs = parse_entity_args(&profile_entity.raw_text)?;
            if pargs.len() < 3 {
                return None;
            }
            let curve_id = parse_ref(pargs[2].trim());
            let points = resolve_curve_points(curve_id, entity_map, unit_scale)?;
            if points.is_empty() {
                return None;
            }
            let min_x = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
            let max_x = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
            let min_y = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
            let max_y = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
            let width = max_x - min_x;
            let depth_y = max_y - min_y;
            Some((height, "rectangle".to_string(), vec![width, depth_y]))
        }
        _ => None,
    }
}

/// Find the IFCEXTRUDEDAREASOLID inside a product's Body shape representation,
/// handling both direct solids and IFCMAPPEDITEM indirection.
/// Returns the entity ID of the extruded solid.
fn find_body_extruded_solid_id(repr_id: u64, entity_map: &HashMap<u64, &IfcEntity>) -> Option<u64> {
    let repr_entity = entity_map.get(&repr_id)?;
    if repr_entity.entity_type != "IFCPRODUCTDEFINITIONSHAPE" {
        return None;
    }
    let repr_args = parse_entity_args(&repr_entity.raw_text)?;
    if repr_args.len() < 3 {
        return None;
    }
    let shape_repr_ids = parse_ref_list(repr_args.last()?.trim());

    for sr_id in shape_repr_ids {
        let sr_entity = entity_map.get(&sr_id)?;
        if sr_entity.entity_type != "IFCSHAPEREPRESENTATION" {
            continue;
        }
        if !sr_entity.raw_text.contains("'Body'") {
            continue;
        }
        let sr_args = parse_entity_args(&sr_entity.raw_text)?;
        if sr_args.len() < 4 {
            continue;
        }
        let item_ids = parse_ref_list(sr_args[3].trim());

        for item_id in &item_ids {
            // Direct IFCEXTRUDEDAREASOLID
            if let Some(entity) = entity_map.get(item_id)
                && entity.entity_type == "IFCEXTRUDEDAREASOLID"
            {
                return Some(*item_id);
            }
            // IFCMAPPEDITEM → IFCREPRESENTATIONMAP → source repr → extruded solid
            if let Some(entity) = entity_map.get(item_id)
                && entity.entity_type == "IFCMAPPEDITEM"
                && let Some(mi_args) = parse_entity_args(&entity.raw_text)
                && !mi_args.is_empty()
            {
                let rep_map_id = parse_ref(mi_args[0].trim());
                if let Some(rep_map) = entity_map.get(&rep_map_id)
                    && rep_map.entity_type == "IFCREPRESENTATIONMAP"
                    && let Some(rm_args) = parse_entity_args(&rep_map.raw_text)
                    && rm_args.len() >= 2
                {
                    let source_repr_id = parse_ref(rm_args[1].trim());
                    if let Some(source_repr) = entity_map.get(&source_repr_id)
                        && source_repr.entity_type == "IFCSHAPEREPRESENTATION"
                        && let Some(src_args) = parse_entity_args(&source_repr.raw_text)
                        && src_args.len() >= 4
                    {
                        let src_item_ids = parse_ref_list(src_args[3].trim());
                        for src_item_id in &src_item_ids {
                            if let Some(src_item) = entity_map.get(src_item_id)
                                && src_item.entity_type == "IFCEXTRUDEDAREASOLID"
                            {
                                return Some(*src_item_id);
                            }
                        }
                    }
                }
            }
            // IFCBOOLEANCLIPPINGRESULT → first operand
            if let Some(entity) = entity_map.get(item_id)
                && entity.entity_type == "IFCBOOLEANCLIPPINGRESULT"
                && let Some(args) = parse_entity_args(&entity.raw_text)
                && args.len() >= 2
            {
                let first_operand = parse_ref(args[1].trim());
                if let Some(fo) = entity_map.get(&first_operand)
                    && fo.entity_type == "IFCEXTRUDEDAREASOLID"
                {
                    return Some(first_operand);
                }
            }
        }
    }
    None
}

/// Find axis endpoints from a product's IFCPRODUCTDEFINITIONSHAPE,
/// handling IFCMAPPEDITEM indirection for 'Axis' representations.
/// Returns (x1, y1, x2, y2) in local coordinates (scaled to metres).
fn find_axis_endpoints_with_mapped(
    repr_id: u64,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Option<(f64, f64, f64, f64)> {
    // First try the standard direct resolution
    if let Some(result) = find_axis_endpoints(repr_id, entity_map, unit_scale) {
        return Some(result);
    }

    // If that fails, try resolving through mapped items
    let repr_entity = entity_map.get(&repr_id)?;
    if repr_entity.entity_type != "IFCPRODUCTDEFINITIONSHAPE" {
        return None;
    }
    let repr_args = parse_entity_args(&repr_entity.raw_text)?;
    if repr_args.len() < 3 {
        return None;
    }
    let shape_repr_ids = parse_ref_list(repr_args.last()?.trim());

    for sr_id in shape_repr_ids {
        let sr_entity = entity_map.get(&sr_id)?;
        if sr_entity.entity_type != "IFCSHAPEREPRESENTATION" {
            continue;
        }
        if !sr_entity.raw_text.contains("'Axis'") {
            continue;
        }
        let sr_args = parse_entity_args(&sr_entity.raw_text)?;
        if sr_args.len() < 4 {
            continue;
        }
        let item_ids = parse_ref_list(sr_args[3].trim());
        for item_id in item_ids {
            if let Some(entity) = entity_map.get(&item_id)
                && entity.entity_type == "IFCMAPPEDITEM"
                && let Some(mi_args) = parse_entity_args(&entity.raw_text)
                && !mi_args.is_empty()
            {
                let rep_map_id = parse_ref(mi_args[0].trim());
                if let Some(rep_map) = entity_map.get(&rep_map_id)
                    && rep_map.entity_type == "IFCREPRESENTATIONMAP"
                    && let Some(rm_args) = parse_entity_args(&rep_map.raw_text)
                    && rm_args.len() >= 2
                {
                    let source_repr_id = parse_ref(rm_args[1].trim());
                    // The source is a shape representation with the actual curve
                    if let Some(source_repr) = entity_map.get(&source_repr_id)
                        && source_repr.entity_type == "IFCSHAPEREPRESENTATION"
                        && let Some(src_args) = parse_entity_args(&source_repr.raw_text)
                        && src_args.len() >= 4
                    {
                        let src_items = parse_ref_list(src_args[3].trim());
                        for src_item_id in src_items {
                            if let Some(src_item) = entity_map.get(&src_item_id) {
                                // Try polyline
                                if src_item.entity_type == "IFCPOLYLINE"
                                    && let Some(points) =
                                        resolve_polyline_points(src_item_id, entity_map, unit_scale)
                                    && points.len() >= 2
                                {
                                    return Some((
                                        points[0].0,
                                        points[0].1,
                                        points[points.len() - 1].0,
                                        points[points.len() - 1].1,
                                    ));
                                }
                                // Try indexed polycurve (may be 3D — extract XY)
                                if src_item.entity_type == "IFCINDEXEDPOLYCURVE"
                                    && let Some(curve_args) = parse_entity_args(&src_item.raw_text)
                                    && !curve_args.is_empty()
                                {
                                    let ptlist_id = parse_ref(curve_args[0].trim());
                                    // Try 2D point list first
                                    if let Some(points) =
                                        resolve_point_list(ptlist_id, entity_map, unit_scale)
                                        && points.len() >= 2
                                    {
                                        return Some((
                                            points[0].0,
                                            points[0].1,
                                            points[points.len() - 1].0,
                                            points[points.len() - 1].1,
                                        ));
                                    }
                                    // Try 3D point list — extract X,Y
                                    if let Some(ptlist) = entity_map.get(&ptlist_id)
                                        && ptlist.entity_type == "IFCCARTESIANPOINTLIST3D"
                                        && let Some(pl_args) = parse_entity_args(&ptlist.raw_text)
                                        && !pl_args.is_empty()
                                    {
                                        let coords = parse_coordinate_list(&pl_args[0]);
                                        let mut pts = Vec::new();
                                        for chunk in coords.chunks(3) {
                                            if chunk.len() >= 2 {
                                                pts.push((
                                                    chunk[0] * unit_scale,
                                                    chunk[1] * unit_scale,
                                                ));
                                            }
                                        }
                                        if pts.len() >= 2 {
                                            return Some((
                                                pts[0].0,
                                                pts[0].1,
                                                pts[pts.len() - 1].0,
                                                pts[pts.len() - 1].1,
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Extract column geometry: position, profile, and height.
fn extract_column_geometry(
    entity: &IfcEntity,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Vec<ResolvedProperty> {
    let mut props = Vec::new();
    let span = dummy_span();
    let round3 = |v: f64| (v * 1000.0).round() / 1000.0;

    let Some(args) = parse_entity_args(&entity.raw_text) else {
        return props;
    };
    // IFCCOLUMN(guid, owner, name, desc, type, #placement, #repr, tag, predefined_type)
    if args.len() < 7 {
        return props;
    }

    let placement_id = parse_ref(args[5].trim());
    let repr_id = parse_ref(args[6].trim());

    // 1. Extract position from placement
    if let Some(place_entity) = entity_map.get(&placement_id)
        && place_entity.entity_type == "IFCLOCALPLACEMENT"
        && let Some(lp_args) = parse_entity_args(&place_entity.raw_text)
        && lp_args.len() >= 2
    {
        let axis2_id = parse_ref(lp_args[1].trim());
        if let Some((x, y, _z)) =
            resolve_axis2placement3d_location(axis2_id, entity_map, unit_scale)
        {
            props.push(ResolvedProperty {
                key: "position".to_string(),
                value: ResolvedValue::Point(round3(x), round3(y)),
                span,
            });
        }
    }

    // 2. Extract profile and height from body representation
    if let Some(solid_id) = find_body_extruded_solid_id(repr_id, entity_map)
        && let Some((height, profile_type, params)) =
            resolve_profile_and_height(solid_id, entity_map, unit_scale)
    {
        props.push(ResolvedProperty {
            key: "profile".to_string(),
            value: ResolvedValue::Profile {
                profile_type,
                params: params.iter().map(|v| round3(*v)).collect(),
            },
            span,
        });
        props.push(ResolvedProperty {
            key: "height".to_string(),
            value: ResolvedValue::Number(round3(height)),
            span,
        });
    }

    props
}

/// Extract beam geometry: axis (from/to) and profile.
fn extract_beam_geometry(
    entity: &IfcEntity,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Vec<ResolvedProperty> {
    let mut props = Vec::new();
    let span = dummy_span();
    let round3 = |v: f64| (v * 1000.0).round() / 1000.0;

    let Some(args) = parse_entity_args(&entity.raw_text) else {
        return props;
    };
    // IFCBEAM(guid, owner, name, desc, type, #placement, #repr, tag, predefined_type)
    if args.len() < 7 {
        return props;
    }

    let placement_id = parse_ref(args[5].trim());
    let repr_id = parse_ref(args[6].trim());

    // 1. Extract axis endpoints (with mapped item support)
    if let Some((x1, y1, x2, y2)) = find_axis_endpoints_with_mapped(repr_id, entity_map, unit_scale)
    {
        // Transform by beam placement to get storey-relative coordinates
        let (tx1, ty1, tx2, ty2) =
            transform_axis_by_placement(placement_id, x1, y1, x2, y2, entity_map, unit_scale);

        props.push(ResolvedProperty {
            key: "from".to_string(),
            value: ResolvedValue::Axis {
                from: (round3(tx1), round3(ty1)),
                to: (round3(tx2), round3(ty2)),
                via: None,
                z: None,
            },
            span,
        });
    }

    // 2. Extract profile from body representation
    if let Some(solid_id) = find_body_extruded_solid_id(repr_id, entity_map)
        && let Some((_height, profile_type, params)) =
            resolve_profile_and_height(solid_id, entity_map, unit_scale)
    {
        props.push(ResolvedProperty {
            key: "profile".to_string(),
            value: ResolvedValue::Profile {
                profile_type,
                params: params.iter().map(|v| round3(*v)).collect(),
            },
            span,
        });
    }

    props
}

/// Extract window/door offset along parent wall axis and sill height (for windows).
/// The opening's IfcLocalPlacement is relative to the wall's placement.
/// X component = offset along wall axis, Z component = sill height.
fn extract_opening_offset(
    entity: &IfcEntity,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
    elem_type: ElementType,
) -> Vec<ResolvedProperty> {
    let mut props = Vec::new();
    let span = dummy_span();
    let round3 = |v: f64| (v * 1000.0).round() / 1000.0;

    let Some(args) = parse_entity_args(&entity.raw_text) else {
        return props;
    };
    if args.len() < 6 {
        return props;
    }
    let placement_id = parse_ref(args[5].trim());

    let Some(lp_entity) = entity_map.get(&placement_id) else {
        return props;
    };
    if !lp_entity
        .entity_type
        .eq_ignore_ascii_case("IFCLOCALPLACEMENT")
    {
        return props;
    }
    let Some(lp_args) = parse_entity_args(&lp_entity.raw_text) else {
        return props;
    };
    if lp_args.len() < 2 {
        return props;
    }
    let rel_ref = parse_ref(lp_args[1].trim());

    if let Some((x, _y, z)) = resolve_axis2placement3d_location(rel_ref, entity_map, unit_scale) {
        props.push(ResolvedProperty {
            key: "offset".into(),
            value: ResolvedValue::Number(round3(x.abs())),
            span,
        });
        if elem_type == ElementType::Window {
            props.push(ResolvedProperty {
                key: "sill_height".into(),
                value: ResolvedValue::Number(round3(z.abs())),
                span,
            });
        }
    }
    props
}

/// Extract slab geometry: boundary polygon from profile + thickness from extrusion depth.
fn extract_slab_geometry(
    entity: &IfcEntity,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Vec<ResolvedProperty> {
    let mut props = Vec::new();
    let span = dummy_span();
    let round3 = |v: f64| (v * 1000.0).round() / 1000.0;

    let Some(args) = parse_entity_args(&entity.raw_text) else {
        return props;
    };
    if args.len() < 7 {
        return props;
    }

    let placement_id = parse_ref(args[5].trim());
    let repr_id = parse_ref(args[6].trim());

    // Get placement offset for transforming boundary points
    let (tx, ty) = if let Some(place_entity) = entity_map.get(&placement_id) {
        if place_entity.entity_type == "IFCLOCALPLACEMENT" {
            if let Some(lp_args) = parse_entity_args(&place_entity.raw_text) {
                if lp_args.len() >= 2 {
                    let axis2_id = parse_ref(lp_args[1].trim());
                    resolve_axis2placement3d_location(axis2_id, entity_map, unit_scale)
                        .map(|(x, y, _z)| (x, y))
                        .unwrap_or((0.0, 0.0))
                } else {
                    (0.0, 0.0)
                }
            } else {
                (0.0, 0.0)
            }
        } else {
            (0.0, 0.0)
        }
    } else {
        (0.0, 0.0)
    };

    // Find the extruded solid in the Body representation
    if let Some(solid_id) = find_body_extruded_solid_id(repr_id, entity_map)
        && let Some(solid_entity) = entity_map.get(&solid_id)
        && solid_entity.entity_type == "IFCEXTRUDEDAREASOLID"
        && let Some(solid_args) = parse_entity_args(&solid_entity.raw_text)
        && solid_args.len() >= 4
    {
        let profile_id = parse_ref(solid_args[0].trim());
        let depth: f64 = solid_args[3]
            .trim()
            .trim_end_matches('.')
            .parse()
            .unwrap_or(0.0);

        // Extract thickness from extrusion depth
        if depth > 0.0 {
            props.push(ResolvedProperty {
                key: "thickness".into(),
                value: ResolvedValue::Number(round3(depth * unit_scale)),
                span,
            });
        }

        // Extract boundary polygon from profile
        if let Some(profile_entity) = entity_map.get(&profile_id) {
            match profile_entity.entity_type.as_str() {
                "IFCARBITRARYCLOSEDPROFILEDEF" | "IFCARBITRARYPROFILEDEFWITHVOIDS" => {
                    let pargs = parse_entity_args(&profile_entity.raw_text);
                    if let Some(pargs) = pargs
                        && pargs.len() >= 3
                    {
                        let curve_id = parse_ref(pargs[2].trim());
                        if let Some(points) = resolve_curve_points(curve_id, entity_map, unit_scale)
                            && points.len() >= 3
                        {
                            let boundary: Vec<(f64, f64)> = points
                                .iter()
                                .map(|(x, y)| (round3(x + tx), round3(y + ty)))
                                .collect();
                            props.push(ResolvedProperty {
                                key: "boundary".into(),
                                value: ResolvedValue::Polygon(boundary),
                                span,
                            });
                        }
                    }
                }
                "IFCRECTANGLEPROFILEDEF" => {
                    // Convert rectangle profile to polygon boundary
                    let pargs = parse_entity_args(&profile_entity.raw_text);
                    if let Some(pargs) = pargs
                        && pargs.len() >= 5
                        && let (Ok(x_dim), Ok(y_dim)) = (
                            pargs[3].trim().trim_end_matches('.').parse::<f64>(),
                            pargs[4].trim().trim_end_matches('.').parse::<f64>(),
                        )
                    {
                        let hw = x_dim * unit_scale / 2.0;
                        let hd = y_dim * unit_scale / 2.0;
                        let boundary = vec![
                            (round3(tx - hw), round3(ty - hd)),
                            (round3(tx + hw), round3(ty - hd)),
                            (round3(tx + hw), round3(ty + hd)),
                            (round3(tx - hw), round3(ty + hd)),
                        ];
                        props.push(ResolvedProperty {
                            key: "boundary".into(),
                            value: ResolvedValue::Polygon(boundary),
                            span,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    props
}

/// Extract position from placement for element types that don't have dedicated geometry extraction.
/// Used for Proxy, Opening, Member, Plate.
fn extract_position_from_placement(
    entity: &IfcEntity,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Vec<ResolvedProperty> {
    let mut props = Vec::new();
    let span = dummy_span();
    let round3 = |v: f64| (v * 1000.0).round() / 1000.0;

    let Some(args) = parse_entity_args(&entity.raw_text) else {
        return props;
    };
    if args.len() < 6 {
        return props;
    }
    let placement_id = parse_ref(args[5].trim());

    let Some(lp_entity) = entity_map.get(&placement_id) else {
        return props;
    };
    if !lp_entity
        .entity_type
        .eq_ignore_ascii_case("IFCLOCALPLACEMENT")
    {
        return props;
    }
    let Some(lp_args) = parse_entity_args(&lp_entity.raw_text) else {
        return props;
    };
    if lp_args.len() < 2 {
        return props;
    }
    let rel_ref = parse_ref(lp_args[1].trim());

    if let Some((x, y, _z)) = resolve_axis2placement3d_location(rel_ref, entity_map, unit_scale) {
        props.push(ResolvedProperty {
            key: "position".into(),
            value: ResolvedValue::Point(round3(x), round3(y)),
            span,
        });
    }
    props
}

/// Extract geometry properties directly from IFC entity parameters.
/// IFC schema puts OverallHeight at argument index 8 and OverallWidth at index 9
/// for both IFCWINDOW and IFCDOOR entities.
/// For walls, extracts axis endpoints, height, and thickness from geometry representations.
/// For columns, extracts position, profile, and height.
/// For beams, extracts axis (from/to) and profile.
/// For slabs, extracts boundary polygon and thickness.
/// For windows/doors, also extracts offset and sill height from placement.
/// For members, extracts beam-like axis+profile or falls back to position+height.
/// For proxies/openings/plates, extracts position from placement.
fn extract_entity_dimensions(
    ifc_id: u64,
    elem_type: ElementType,
    entity_map: &HashMap<u64, &IfcEntity>,
    unit_scale: f64,
) -> Vec<ResolvedProperty> {
    let mut props = Vec::new();
    let span = dummy_span();
    let Some(entity) = entity_map.get(&ifc_id) else {
        return props;
    };

    match elem_type {
        ElementType::Wall => {
            props.extend(extract_wall_geometry(entity, entity_map, unit_scale));
        }
        ElementType::Column => {
            props.extend(extract_column_geometry(entity, entity_map, unit_scale));
        }
        ElementType::Beam => {
            props.extend(extract_beam_geometry(entity, entity_map, unit_scale));
        }
        ElementType::Slab => {
            props.extend(extract_slab_geometry(entity, entity_map, unit_scale));
        }
        ElementType::Window | ElementType::Door => {
            if let Some(h) = extract_numeric_field(&entity.raw_text, 8) {
                props.push(ResolvedProperty {
                    key: "height".into(),
                    value: ResolvedValue::Number(h * unit_scale),
                    span,
                });
            }
            if let Some(w) = extract_numeric_field(&entity.raw_text, 9) {
                props.push(ResolvedProperty {
                    key: "width".into(),
                    value: ResolvedValue::Number(w * unit_scale),
                    span,
                });
            }
            // Also extract offset and sill height from placement
            props.extend(extract_opening_offset(
                entity, entity_map, unit_scale, elem_type,
            ));
        }
        ElementType::Member => {
            // Try beam-like extraction (axis + profile) first; fall back to position
            let beam_props = extract_beam_geometry(entity, entity_map, unit_scale);
            if beam_props.iter().any(|p| p.key == "from") {
                props.extend(beam_props);
            } else {
                // No axis representation — extract position and height from body extrusion
                props.extend(extract_position_from_placement(
                    entity, entity_map, unit_scale,
                ));
                // Try to extract profile and height from body representation
                let Some(args) = parse_entity_args(&entity.raw_text) else {
                    return props;
                };
                if args.len() >= 7 {
                    let repr_id = parse_ref(args[6].trim());
                    if let Some(solid_id) = find_body_extruded_solid_id(repr_id, entity_map)
                        && let Some((height, profile_type, params)) =
                            resolve_profile_and_height(solid_id, entity_map, unit_scale)
                    {
                        let round3 = |v: f64| (v * 1000.0).round() / 1000.0;
                        props.push(ResolvedProperty {
                            key: "height".to_string(),
                            value: ResolvedValue::Number(round3(height)),
                            span,
                        });
                        props.push(ResolvedProperty {
                            key: "profile".to_string(),
                            value: ResolvedValue::Profile {
                                profile_type,
                                params: params.iter().map(|v| round3(*v)).collect(),
                            },
                            span,
                        });
                    }
                }
            }
        }
        ElementType::Proxy | ElementType::Opening | ElementType::Plate => {
            props.extend(extract_position_from_placement(
                entity, entity_map, unit_scale,
            ));
        }
        _ => {}
    }
    props
}

/// Parse IfcRelDefinesByProperties + IfcPropertySet + IfcPropertySingleValue
/// to extract properties for elements.
fn parse_property_sets(ifc_content: &str, unit_scale: f64) -> HashMap<u64, Vec<ResolvedProperty>> {
    // Step 1: Parse IfcPropertySingleValue entities
    // IFCPROPERTYSINGLEVALUE('Name',$,IFCTYPE(value),$);
    let mut prop_values: HashMap<u64, (String, String)> = HashMap::new(); // id → (name, value_repr)

    // Step 2: Parse IfcPropertySet entities
    // IFCPROPERTYSET('guid',#owner,'PsetName',$,(#prop1,#prop2,...));
    let mut psets: HashMap<u64, (String, Vec<u64>)> = HashMap::new(); // id → (name, prop_ids)

    // Step 3: Parse IfcRelDefinesByProperties
    // IFCRELDEFINESBYPROPERTIES('guid',#owner,$,$,(#elem1,#elem2,...),#pset);
    let mut rel_defines: Vec<(Vec<u64>, u64)> = Vec::new(); // (element_ids, pset_id)

    for statement in entity_statements(ifc_content) {
        let trimmed: &str = &statement;
        let Some(eq_pos) = trimmed.find('=') else {
            continue;
        };
        let id_str = trimmed[1..eq_pos].trim();
        let Ok(id) = id_str.parse::<u64>() else {
            continue;
        };
        let entity_text = trimmed[eq_pos + 1..].trim();

        if entity_text.starts_with("IFCPROPERTYSINGLEVALUE(")
            && let Some((name, value)) = parse_property_single_value(entity_text)
        {
            prop_values.insert(id, (name, value));
        } else if entity_text.starts_with("IFCPROPERTYSET(")
            && let Some((name, prop_ids)) = parse_property_set(entity_text)
        {
            psets.insert(id, (name, prop_ids));
        } else if entity_text.starts_with("IFCRELDEFINESBYPROPERTIES(")
            && let Some((elem_ids, pset_id)) = parse_rel_defines_by_properties(entity_text)
        {
            rel_defines.push((elem_ids, pset_id));
        }
    }

    // Build element → properties map
    let mut result: HashMap<u64, Vec<ResolvedProperty>> = HashMap::new();

    // Only import recognized property sets (Pset_* BIM standard sets)
    let importable_psets: HashSet<&str> = [
        "Pset_WallCommon",
        "Pset_SlabCommon",
        "Pset_ColumnCommon",
        "Pset_BeamCommon",
        "Pset_WindowCommon",
        "Pset_DoorCommon",
        "Pset_RoofCommon",
        "Pset_StairCommon",
        "Pset_RailingCommon",
        "Pset_RampCommon",
        "Pset_SpaceCommon",
    ]
    .into_iter()
    .collect();

    for (elem_ids, pset_id) in &rel_defines {
        let Some((pset_name, prop_ids)) = psets.get(pset_id) else {
            continue;
        };

        if !importable_psets.contains(pset_name.as_str()) {
            continue;
        }

        for elem_id in elem_ids {
            let elem_props = result.entry(*elem_id).or_default();
            for prop_id in prop_ids {
                if let Some((prop_name, prop_value)) = prop_values.get(prop_id)
                    && let Some(resolved) =
                        convert_property_value(prop_name, prop_value, unit_scale)
                {
                    elem_props.push(resolved);
                }
            }
        }
    }

    result
}

fn parse_property_single_value(entity_text: &str) -> Option<(String, String)> {
    // IFCPROPERTYSINGLEVALUE('Name',$,IFCTYPE(value),$);
    let paren_start = entity_text.find('(')?;
    let paren_end = entity_text.rfind(')')?;
    let args = split_ifc_args(&entity_text[paren_start + 1..paren_end]);

    if args.len() < 3 {
        return None;
    }

    let name = unquote(args[0]).to_string();
    let value = args[2].trim().to_string();

    if name.is_empty() || value == "$" {
        return None;
    }

    Some((name, value))
}

fn parse_property_set(entity_text: &str) -> Option<(String, Vec<u64>)> {
    // IFCPROPERTYSET('guid',#owner,'PsetName',$,(#prop1,#prop2,...));
    let paren_start = entity_text.find('(')?;
    let paren_end = entity_text.rfind(')')?;
    let args = split_ifc_args(&entity_text[paren_start + 1..paren_end]);

    if args.len() < 5 {
        return None;
    }

    let name = unquote(args[2]).to_string();
    let prop_ids = parse_ref_list(args[4].trim());

    Some((name, prop_ids))
}

fn parse_rel_defines_by_properties(entity_text: &str) -> Option<(Vec<u64>, u64)> {
    // IFCRELDEFINESBYPROPERTIES('guid',#owner,$,$,(#elem1,...),#pset);
    let paren_start = entity_text.find('(')?;
    let paren_end = entity_text.rfind(')')?;
    let args = split_ifc_args(&entity_text[paren_start + 1..paren_end]);

    if args.len() < 6 {
        return None;
    }

    let elem_ids = parse_ref_list(args[4].trim());
    let pset_id = parse_ref(args[5].trim());

    Some((elem_ids, pset_id))
}

/// Convert an IFC property value to a BML ResolvedProperty.
fn convert_property_value(
    name: &str,
    value_str: &str,
    unit_scale: f64,
) -> Option<ResolvedProperty> {
    let span = dummy_span();

    // Skip certain properties that are not useful in BML
    let skip_props = [
        "Reference",
        "Status",
        "Compartmentation",
        "SurfaceSpreadOfFlame",
    ];
    if skip_props.contains(&name) {
        return None;
    }

    let value = value_str.trim();

    // Try to parse IFC typed values
    if value.starts_with("IFCBOOLEAN(") || value.starts_with("IFCLOGICAL(") {
        let inner = extract_ifc_value_inner(value)?;
        let b = inner == ".T." || inner.to_uppercase() == "TRUE";
        return Some(ResolvedProperty {
            key: to_snake_case(name),
            value: ResolvedValue::Boolean(b),
            span,
        });
    }

    // Numeric measures with the power of the length unit they scale with
    const MEASURES: [(&str, i32); 7] = [
        ("IFCLENGTHMEASURE(", 1),
        ("IFCPOSITIVELENGTHMEASURE(", 1),
        ("IFCNONNEGATIVELENGTHMEASURE(", 1),
        ("IFCAREAMEASURE(", 2),
        ("IFCVOLUMEMEASURE(", 3),
        // Unit-independent numbers (acoustic rating, U-value)
        ("IFCREAL(", 0),
        ("IFCTHERMALTRANSMITTANCEMEASURE(", 0),
    ];
    if let Some(&(_, power)) = MEASURES.iter().find(|(m, _)| value.starts_with(m)) {
        let inner = extract_ifc_value_inner(value)?;
        let n: f64 = inner.trim_end_matches('.').parse().ok()?;
        return Some(ResolvedProperty {
            key: to_snake_case(name),
            value: ResolvedValue::Number(n * unit_scale.powi(power)),
            span,
        });
    }

    if value.starts_with("IFCLABEL(") || value.starts_with("IFCTEXT(") {
        let inner = extract_ifc_value_inner(value)?;
        let s = decode_step_str(unquote(&inner));
        return Some(ResolvedProperty {
            key: to_snake_case(name),
            value: ResolvedValue::String(s),
            span,
        });
    }

    if value.starts_with("IFCIDENTIFIER(") {
        let inner = extract_ifc_value_inner(value)?;
        let s = unquote(&inner).to_string();
        return Some(ResolvedProperty {
            key: to_snake_case(name),
            value: ResolvedValue::Identifier(s),
            span,
        });
    }

    // Numeric literal
    if let Ok(n) = value.trim_end_matches('.').parse::<f64>() {
        return Some(ResolvedProperty {
            key: to_snake_case(name),
            value: ResolvedValue::Number(n * unit_scale),
            span,
        });
    }

    None
}

/// Extract the inner value from an IFC typed value like `IFCTYPE(value)`.
fn extract_ifc_value_inner(value: &str) -> Option<String> {
    let paren_start = value.find('(')?;
    let paren_end = value.rfind(')')?;
    if paren_start >= paren_end {
        return None;
    }
    Some(value[paren_start + 1..paren_end].to_string())
}

// ─── Low-Level Parsing Utilities ─────────────────────────────────────

/// Extract the name (3rd field, index 2) from an IFC entity string.
pub(crate) fn extract_name(entity_text: &str) -> Option<String> {
    extract_field(entity_text, 2)
}

/// Extract the "long name" (8th field, index 7) from IFCSPACE.
fn extract_long_name(entity_text: &str) -> Option<String> {
    extract_field(entity_text, 7)
}

/// A STEP string without its enclosing quotes, keeping a doubled `''` at
/// either end.
pub(crate) fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .unwrap_or(s)
}

/// Decode the body of a STEP string: `''`, `\\`, `\X\hh` and `\S\c` (Latin-1),
/// and `\X2\hhhh...\X0\` (UTF-16), the inverse of `step_writer::step_str`.
pub(crate) fn decode_step_str(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if rest.starts_with("''") {
            out.push('\'');
            rest = &rest[2..];
        } else if rest.starts_with("\\\\") {
            out.push('\\');
            rest = &rest[2..];
        } else if let Some(body) = rest.strip_prefix("\\X2\\")
            && let Some(end) = body.find("\\X0\\")
        {
            let units: Vec<u16> = (0..end / 4)
                .filter_map(|i| {
                    body.get(i * 4..i * 4 + 4)
                        .and_then(|h| u16::from_str_radix(h, 16).ok())
                })
                .collect();
            out.push_str(&String::from_utf16_lossy(&units));
            rest = &body[end + 4..];
        } else if let Some(hex) = rest.strip_prefix("\\X\\").and_then(|b| b.get(..2))
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b as char);
            rest = &rest[5..];
        } else if let Some(c) = rest.strip_prefix("\\S\\").and_then(|b| b.chars().next())
            && c.is_ascii()
        {
            // Latin-1 upper half: `\S\e` is 0x65 + 0x80 = `å`.
            out.push((c as u8 + 0x80) as char);
            // `§` is `\S\'`, its apostrophe doubled like any other.
            rest = &rest[if rest[3..].starts_with("''") { 5 } else { 4 }..];
        } else {
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// Extract a quoted string field at the given argument index.
fn extract_field(entity_text: &str, target_idx: usize) -> Option<String> {
    let paren_start = entity_text.find('(')?;
    let args = &entity_text[paren_start + 1..];

    let mut field_idx = 0;
    let mut in_quotes = false;
    let mut depth = 0;
    let mut name_start = None;

    let mut chars = args.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        match ch {
            // `''` inside a string is an escaped quote
            '\'' if in_quotes && chars.peek().is_some_and(|(_, c)| *c == '\'') => {
                chars.next();
            }
            '\'' if depth == 0 => {
                if in_quotes {
                    if field_idx == target_idx {
                        return Some(decode_step_str(&args[name_start.unwrap()..i]));
                    }
                    in_quotes = false;
                } else {
                    in_quotes = true;
                    if field_idx == target_idx {
                        name_start = Some(i + 1);
                    }
                }
            }
            ',' if !in_quotes && depth == 0 => {
                field_idx += 1;
            }
            '(' if !in_quotes => depth += 1,
            ')' if !in_quotes && depth > 0 => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Extract elevation from an IFCBUILDINGSTOREY entity.
/// IFC schema: GlobalId(0), OwnerHistory(1), Name(2), Description(3),
/// ObjectType(4), ObjectPlacement(5), Representation(6), LongName(7),
/// CompositionType(8), Elevation(9)
fn extract_elevation(entity_text: &str) -> Option<f64> {
    let paren_start = entity_text.find('(')?;
    let paren_end = entity_text.rfind(')')?;
    let args = &entity_text[paren_start + 1..paren_end];

    let parts: Vec<&str> = split_ifc_args(args);
    // Try index 9 first (standard IFC4 IFCBUILDINGSTOREY with 10 params)
    if parts.len() >= 10 {
        let elev_str = parts[9].trim();
        if elev_str != "$"
            && let Ok(v) = elev_str.trim_end_matches('.').parse::<f64>()
        {
            return Some(v);
        }
    }
    // Fallback to index 8 for older or shorter entity formats
    if parts.len() >= 9 {
        let elev_str = parts[8].trim();
        if elev_str != "$" && !elev_str.starts_with('.') {
            return elev_str.trim_end_matches('.').parse::<f64>().ok();
        }
    }
    None
}

/// Split IFC arguments respecting nested parentheses and quoted strings.
pub(crate) fn split_ifc_args(args: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut in_quotes = false;
    let mut start = 0;

    for (i, ch) in args.char_indices() {
        match ch {
            '\'' => in_quotes = !in_quotes,
            '(' if !in_quotes => depth += 1,
            ')' if !in_quotes => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            ',' if !in_quotes && depth == 0 => {
                parts.push(&args[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&args[start..]);
    parts
}

/// Parse a STEP reference like `#123` into its numeric ID.
pub(super) fn parse_ref(s: &str) -> u64 {
    s.trim().trim_start_matches('#').parse::<u64>().unwrap_or(0)
}

/// Parse a STEP reference list like `(#1,#2,#3)` into a Vec of IDs.
pub(super) fn parse_ref_list(s: &str) -> Vec<u64> {
    let inner = s
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();
    if inner.is_empty() {
        return Vec::new();
    }
    inner
        .split(',')
        .filter_map(|r| {
            let r = r.trim().trim_start_matches('#');
            r.parse::<u64>().ok()
        })
        .collect()
}

/// Convert a CamelCase property name to snake_case for BML.
fn to_snake_case(name: &str) -> String {
    let mut result = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(ch.to_lowercase().next().unwrap_or(ch));
    }
    result
}

/// Extract a numeric field at a given argument index from an IFC entity.
pub(crate) fn extract_numeric_field(entity_text: &str, target_idx: usize) -> Option<f64> {
    let paren_start = entity_text.find('(')?;
    let paren_end = entity_text.rfind(')')?;
    let args = split_ifc_args(&entity_text[paren_start + 1..paren_end]);
    let field = args.get(target_idx)?.trim();
    if field == "$"
        || field.starts_with('.')
        || field.starts_with('\'')
        || field.starts_with('#')
        || field.starts_with('(')
    {
        return None;
    }
    field.trim_end_matches('.').parse::<f64>().ok()
}

fn dummy_span() -> Span {
    Span::new(
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
    )
}
