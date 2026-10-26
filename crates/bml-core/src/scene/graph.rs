use super::appearance;
use crate::codegen::geometry::{self, point_in_polygon};
use crate::codegen::ifc_writer::filler_axis_frame;
use crate::codegen::wall_joins::{self, WallEnds};
use crate::config::default_curve_tolerance;
use crate::geometry::axis::WallAxis;
pub(crate) use crate::semantic::model::number as get_number;
use crate::semantic::model::{
    self, ElementType, ResolvedElement, ResolvedProperty, ResolvedValue, SemanticModel, contents,
};
use std::collections::HashMap;

/// A complete 3D scene generated from a BuildingML semantic model.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SceneGraph {
    pub nodes: Vec<SceneNode>,
    /// Shared material table; nodes reference entries by index via
    /// [`SceneNode::material`]. IFC scenes fill it from surface styles, BML
    /// scenes from `Material:` definitions and [`super::appearance`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<SceneMaterial>,
}

/// A material (from IFC presentation styles or a BML `Material:`), shared across
/// nodes so the colour/shading parameters are transmitted once per scene.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SceneMaterial {
    /// Surface style name, falling back to the `IfcMaterial` name.
    pub name: String,
    /// Base colour; alpha is `1 - IfcSurfaceStyleRendering.Transparency`.
    pub color: [f32; 4],
    /// Specular intensity 0..1 from `IfcSurfaceStyleRendering.SpecularColour`.
    pub specular: f32,
    /// Blinn-Phong exponent from `IfcSurfaceStyleRendering.SpecularHighlight`.
    pub shininess: f32,
}

/// A node in the scene graph representing a building element.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SceneNode {
    pub id: String,
    pub element_type: String,
    pub label: String,
    pub mesh: SceneMesh,
    pub children: Vec<SceneNode>,
    /// Key-value property metadata for interactive display (e.g. height, thickness).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<(String, String)>,
    /// Index into [`SceneGraph::materials`]. `None` falls back to flat shading
    /// of [`SceneMesh::color`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<u32>,
}

/// Triangle mesh data for WebGL rendering.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SceneMesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub color: [f32; 4],
}

/// A vertex with position and normal.
///
/// Both fields serialize through [`round4`]. They are `f32` in memory, but JSON
/// has only one number type, so serde promotes each to `f64` and prints every
/// digit of the promotion: a coordinate held as `-15.2642` goes out as
/// `-15.264200210571289`. Those trailing digits are an artefact of the widening,
/// not data, and on a real model they are most of the payload - vertices are 59
/// MB of a 71 MB scene, and rounding halves that.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Vertex {
    #[serde(serialize_with = "serialize_vec3")]
    pub position: [f32; 3],
    #[serde(serialize_with = "serialize_vec3")]
    pub normal: [f32; 3],
}

/// Decimal places kept when a scene is serialized.
///
/// Scene coordinates are metres, so this is a tenth of a millimetre - finer
/// than any building model means, and finer than the `f32` the value is held in
/// resolves to at building scale anyway. On a unit normal it is an angular
/// error under a hundredth of a degree.
const SCENE_DECIMALS: f64 = 1e4;

/// Round to [`SCENE_DECIMALS`], returning `f64` so serde prints the short form.
fn round4(v: f32) -> f64 {
    let r = ((v as f64) * SCENE_DECIMALS).round() / SCENE_DECIMALS;
    // Keep zero unsigned: `-0.0` serializes as `-0.0` and is pure noise.
    if r == 0.0 { 0.0 } else { r }
}

fn serialize_vec3<S>(v: &[f32; 3], s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeTuple;
    let mut t = s.serialize_tuple(3)?;
    for x in v {
        t.serialize_element(&round4(*x))?;
    }
    t.end()
}

/// Generate a 3D scene graph from a semantic model.
pub fn generate_scene(model: &SemanticModel) -> SceneGraph {
    let mut nodes = Vec::new();
    let mut materials = Vec::new();

    for c in model.containers() {
        let (elevation, height) = (c.elevation(), c.height());
        // Dedicated roof-only storeys (no Wall elements) sit at `elevation`
        // (= top of the walls below). Storeys that contain walls use
        // `elevation + storey_height` so the roof aligns with the wall tops.
        let has_walls = contents(c.spaces, c.elements).any(|e| e.element_type == ElementType::Wall);
        let roof_base = if has_walls { height } else { 0.0 };
        let wall_ends = wall_joins::join_walls(contents(c.spaces, c.elements)).ends;

        // Elements directly under the container, then those inside spaces
        for elem in contents(c.spaces, c.elements) {
            if let Some(mut node) = element_to_node(
                elem,
                elevation,
                height,
                roof_base,
                &model.asset_libraries,
                &wall_ends,
            ) {
                paint(&mut node, elem, model, &mut materials);
                nodes.push(node);
            }
        }
    }

    SceneGraph { nodes, materials }
}

/// Set the material of `node` (built from `elem`) and of its children.
fn paint(
    node: &mut SceneNode,
    elem: &ResolvedElement,
    model: &SemanticModel,
    table: &mut Vec<SceneMaterial>,
) {
    let named = |id: &str| appearance::named(&model.materials, id);
    let set = |node: &mut SceneNode, m: SceneMaterial, table: &mut Vec<SceneMaterial>| {
        node.mesh.color = m.color;
        node.material = Some(appearance::intern(table, m));
    };
    let material = reference(&elem.properties, "material");
    let own = material
        .and_then(named)
        .or_else(|| appearance::for_type(elem.element_type));
    // Layer build-up for the inspector, first layer first.
    let layers = material
        .and_then(|id| model.material(id))
        .and_then(|m| m.layers())
        .unwrap_or_default();
    for (i, layer) in layers.iter().enumerate() {
        node.properties.push((
            format!("Material Layers · {}. {}", i + 1, layer.name),
            format!("{} m", format_prop_number(layer.thickness)),
        ));
    }

    // An asset's parts carry the look; its bounding box stays a faint outline.
    let asset = reference(&elem.properties, "asset").and_then(|id| {
        model
            .asset_libraries
            .iter()
            .flat_map(|l| &l.assets)
            .find(|a| a.id == id)
    });
    if asset.is_none()
        && let Some(m) = own.clone()
    {
        set(node, m, table);
    }
    for child in &mut node.children {
        match child.element_type.as_str() {
            "WindowFrame" | "DoorFrame" => set(child, appearance::frame(elem.element_type), table),
            "FurniturePart" => {
                let part = asset.and_then(|a| {
                    a.parts
                        .iter()
                        .find(|p| child.id == format!("{}_{}", elem.id, p.name))
                });
                if let Some(m) = part
                    .and_then(|p| p.material.as_deref())
                    .and_then(named)
                    .or_else(|| own.clone())
                {
                    set(child, m, table);
                }
            }
            _ => {
                if let Some(c) = elem.children.iter().find(|c| c.id == child.id) {
                    paint(child, c, model, table);
                }
            }
        }
    }
}

/// Identifier (or string) value of a reference property such as `material:`.
fn reference<'a>(props: &'a [ResolvedProperty], key: &str) -> Option<&'a str> {
    match props.iter().find(|p| p.key == key).map(|p| &p.value) {
        Some(ResolvedValue::Identifier(s) | ResolvedValue::String(s)) => Some(s),
        _ => None,
    }
}

fn element_to_node(
    elem: &ResolvedElement,
    elevation: f64,
    storey_height: f64,
    roof_base: f64,
    asset_libraries: &[model::ResolvedAssetLibrary],
    wall_ends: &HashMap<String, WallEnds>,
) -> Option<SceneNode> {
    // `z_offset` lifts the element within its storey; a roof with one ignores the
    // wall-top base.
    let z_offset = get_number(&elem.properties, "z_offset");
    let elevation = elevation + z_offset.unwrap_or(0.0);

    // No body in IFC, none here (one with doors or windows still shows them in it).
    if crate::codegen::ifc_writer::no_body(elem) && elem.children.is_empty() {
        return None;
    }

    // For furniture with asset reference, expand into parts
    if elem.element_type == ElementType::Furniture {
        return furniture_asset_node(elem, elevation, asset_libraries);
    }

    let swept = crate::codegen::ifc_writer::path_sweep(elem);
    let mesh = match elem.element_type {
        // An explicit `boundary` on a type without a native one is a plan prism.
        et if !crate::semantic::registry::get_descriptor(et).has_native_boundary()
            && get_polygon(&elem.properties, "boundary").is_some() =>
        {
            prism_mesh(elem, elevation, get_number(&elem.properties, "height"))
        }
        // A railing, beam or member along `path:`, as in IFC; the frame
        // carries `z_offset`.
        _ if swept.is_some() => swept.map(|shape| {
            let (vertices, indices) =
                super::solids::shape_mesh(&shape, elevation - z_offset.unwrap_or(0.0));
            SceneMesh {
                vertices,
                indices,
                color: element_color(elem.element_type),
            }
        }),
        ElementType::Wall => wall_mesh(elem, elevation, wall_ends.get(&elem.id)),
        ElementType::Slab => slab_mesh(elem, elevation),
        ElementType::Column => column_mesh(elem, elevation),
        ElementType::Beam => beam_mesh(elem, elevation),
        ElementType::Roof => roof_mesh(
            elem,
            elevation,
            if z_offset.is_some() { 0.0 } else { roof_base },
        ),
        ElementType::Stair => stair_mesh(elem, elevation, storey_height),
        ElementType::Window => return None, // rendered as children of walls
        ElementType::Door => return None,   // rendered as children of walls
        ElementType::Railing => railing_mesh(elem, elevation),
        ElementType::Ramp => ramp_mesh(elem, elevation),
        ElementType::Furniture => unreachable!(), // handled above
        ElementType::Elevator => elevator_mesh(elem, elevation, storey_height),
        ElementType::Proxy => proxy_mesh(elem, elevation),
        ElementType::Member => beam_mesh(elem, elevation), // same geometry as beam
        // An upright plate (`from:` axis) has the body of a wall.
        ElementType::Plate if get_axis(&elem.properties).is_some() => {
            wall_mesh(elem, elevation, None)
        }
        ElementType::Plate => slab_mesh(elem, elevation), // same geometry as slab
        ElementType::Opening => return None,              // rendered as children of walls (void)
        ElementType::Grid => grid_mesh(elem, elevation),
        ElementType::Covering => prism_mesh(
            elem,
            elevation,
            Some(get_number(&elem.properties, "thickness").unwrap_or(geometry::COVERING_THICKNESS)),
        ),
        // Hangs down from its top at `z_offset`.
        ElementType::Footing => {
            let depth = get_number(&elem.properties, "depth")?;
            prism_mesh(elem, elevation - depth, Some(depth))
        }
        // Services bodies carry `z_offset` in their own frame.
        ElementType::Duct
        | ElementType::Pipe
        | ElementType::CableTray
        | ElementType::Terminal
        | ElementType::Equipment
        | ElementType::Accessory => {
            let shape = crate::codegen::mep::shape(elem)?;
            let (vertices, indices) =
                super::solids::shape_mesh(&shape, elevation - z_offset.unwrap_or(0.0));
            Some(SceneMesh {
                vertices,
                indices,
                color: element_color(elem.element_type),
            })
        }
        ElementType::System => return None,
    };

    let mesh = mesh?;

    // Generate child nodes (windows/doors in walls)
    let children = elem
        .children
        .iter()
        .filter_map(|child| child_opening_node(child, elem, elevation))
        .collect();

    Some(SceneNode {
        id: elem.id.clone(),
        element_type: elem.element_type.as_str().to_string(),
        label: format!("{}: {}", elem.element_type.as_str(), elem.id),
        mesh,
        children,
        properties: format_properties(&elem.properties),
        material: None,
    })
}

/// `boundary` polygon (with holes) extruded up from the element base by `height`.
fn prism_mesh(elem: &ResolvedElement, elevation: f64, height: Option<f64>) -> Option<SceneMesh> {
    let outer = get_polygon(&elem.properties, "boundary")?;
    // Only a ceiling is cut by its openings; a wall or proxy prism keeps its
    // openings as children (a niche or window opening is not a full-height void).
    let holes = if elem.element_type == ElementType::Covering {
        holes_with_openings(elem, &outer)
    } else {
        outline_holes(&elem.properties)
    };
    let height = height?;
    let oz = elevation as f32;
    let (vertices, indices) = extrude_polygon_with_holes(&outer, &holes, oz, oz + height as f32);
    Some(SceneMesh {
        vertices,
        indices,
        color: element_color(elem.element_type),
    })
}

fn child_opening_node(
    child: &ResolvedElement,
    parent: &ResolvedElement,
    elevation: f64,
) -> Option<SceneNode> {
    // Openings are voids, cut into their host by `wall_mesh` / `slab_mesh`.
    if child.element_type == ElementType::Opening {
        return None;
    }
    if get_polygon(&child.properties, "boundary").is_some() {
        // Boundary children are prisms in storey coordinates, not wall-relative.
        let elevation = elevation - get_number(&parent.properties, "z_offset").unwrap_or(0.0)
            + get_number(&child.properties, "z_offset").unwrap_or(0.0);
        let mut mesh = prism_mesh(child, elevation, get_number(&child.properties, "height"))?;
        if child.element_type == ElementType::Window {
            mesh.color = [0.5, 0.7, 0.9, 0.5];
        }
        return Some(SceneNode {
            id: child.id.clone(),
            element_type: child.element_type.as_str().to_string(),
            label: format!("{}: {}", child.element_type.as_str(), child.id),
            mesh,
            children: Vec::new(),
            properties: format_properties(&child.properties),
            material: None,
        });
    }
    let mesh = match child.element_type {
        ElementType::Window => window_mesh(child, parent, elevation),
        ElementType::Door => door_mesh(child, parent, elevation),
        _ => None,
    }?;

    let children = match child.element_type {
        ElementType::Window => window_detail_nodes(child, parent, elevation),
        ElementType::Door => door_detail_nodes(child, parent, elevation),
        _ => Vec::new(),
    };

    // `lateral_offset` moves the filler across the wall, a door's `sill_height` up.
    let lateral = get_number(&child.properties, "lateral_offset").unwrap_or(0.0);
    let door_sill = match child.element_type {
        ElementType::Door => get_number(&child.properties, "sill_height").unwrap_or(0.0),
        _ => 0.0,
    };
    let (mut mesh, mut children) = (mesh, children);
    if (lateral != 0.0 || door_sill != 0.0)
        && let Some((_, angle)) = filler_frame(child, parent)
    {
        let shift = [
            (-angle.sin() * lateral) as f32,
            (angle.cos() * lateral) as f32,
            door_sill as f32,
        ];
        for m in std::iter::once(&mut mesh).chain(children.iter_mut().map(|c| &mut c.mesh)) {
            for v in &mut m.vertices {
                for (p, d) in v.position.iter_mut().zip(shift) {
                    *p += d;
                }
            }
        }
    }

    Some(SceneNode {
        id: child.id.clone(),
        element_type: child.element_type.as_str().to_string(),
        label: format!("{}: {}", child.element_type.as_str(), child.id),
        mesh,
        children,
        properties: format_properties(&child.properties),
        material: None,
    })
}

// ---------------------------------------------------------------------------
// Wall opening helpers (for door/window voids)
// ---------------------------------------------------------------------------

struct WallOpening {
    offset: f64,
    width: f64,
    bottom: f64, // 0.0 for doors, sill_height for windows
    top: f64,    // opening height from floor
}

/// Extract door/window openings from wall children, sorted by offset along the wall axis.
fn collect_wall_openings(
    wall: &ResolvedElement,
    axis: &WallAxis,
    wall_height: f64,
) -> Vec<WallOpening> {
    let mut openings: Vec<WallOpening> = wall
        .children
        .iter()
        // Fillers that name an explicit `opening:` are cut by that opening instead;
        // boundary fillers are free prisms.
        .filter(|c| match c.element_type {
            ElementType::Door | ElementType::Window => {
                !c.properties.iter().any(|p| p.key == "opening")
                    && get_polygon(&c.properties, "boundary").is_none()
            }
            ElementType::Opening => true,
            _ => false,
        })
        .filter_map(|c| {
            if get_polygon(&c.properties, "boundary").is_some() {
                return boundary_wall_opening(c, wall, axis, wall_height);
            }
            let offset = get_number(&c.properties, "offset").unwrap_or(0.0);
            let width = get_number(&c.properties, "width").unwrap_or(
                if c.element_type == ElementType::Door {
                    0.9
                } else {
                    1.0
                },
            );
            let height = get_number(&c.properties, "height").unwrap_or(
                if c.element_type == ElementType::Door {
                    2.1
                } else {
                    1.0
                },
            );
            let default_sill = if c.element_type == ElementType::Window {
                0.9
            } else {
                0.0
            };
            let sill = get_number(&c.properties, "sill_height").unwrap_or(default_sill);
            Some(WallOpening {
                offset,
                width,
                bottom: sill,
                top: (sill + height).min(wall_height),
            })
        })
        .collect();
    openings.sort_by(|a, b| {
        a.offset
            .partial_cmp(&b.offset)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    openings
}

/// The void an `Opening` prism (`boundary` + `height`) cuts into its host wall:
/// its plan footprint projected onto the wall axis, and its height range
/// relative to the wall base, both clipped to the wall. None if it misses the wall.
fn boundary_wall_opening(
    opening: &ResolvedElement,
    wall: &ResolvedElement,
    axis: &WallAxis,
    wall_height: f64,
) -> Option<WallOpening> {
    let boundary = get_polygon(&opening.properties, "boundary")?;
    let height = get_number(&opening.properties, "height")?;
    let along = boundary.iter().map(|p| axis.project([p.0, p.1]));
    let start = along.clone().fold(f64::INFINITY, f64::min).max(0.0);
    let end = along.fold(f64::NEG_INFINITY, f64::max).min(axis.length());
    let z_offset = |e: &ResolvedElement| get_number(&e.properties, "z_offset").unwrap_or(0.0);
    let base = z_offset(opening) - z_offset(wall);
    let (bottom, top) = (base.max(0.0), (base + height).min(wall_height));
    (end > start && top > bottom).then_some(WallOpening {
        offset: start,
        width: end - start,
        bottom,
        top,
    })
}

/// Append a box's vertices and indices to running totals, offsetting indices.
#[allow(clippy::too_many_arguments)]
fn append_box(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    z0: f32,
    z1: f32,
) {
    let base_idx = vertices.len() as u32;
    vertices.extend(box_vertices(x0, x1, y0, y1, z0, z1));
    for idx in box_indices() {
        indices.push(idx + base_idx);
    }
}

/// Generate wall segments that leave voids where doors/windows are.
/// Coordinates are in wall-local space: x along wall axis, y across thickness, z vertical.
/// `ends` are where the faces stop at each end (see `wall_joins::WallEnds`).
fn segmented_wall_vertices(
    ends: WallEnds,
    half_t: f32,
    wall_height: f32,
    openings: &[WallOpening],
) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let h = half_t as f64;
    // Full-height piece from face positions `a` to `b` (right, left).
    let piece =
        |vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>, a: (f64, f64), b: (f64, f64)| {
            let quad = [(a.0, -h), (b.0, -h), (b.1, h), (a.1, h)];
            let (v, i) = extrude_polygon(&quad, 0.0, wall_height);
            let base = vertices.len() as u32;
            vertices.extend(v);
            indices.extend(i.into_iter().map(|x| x + base));
        };
    // Face positions (right, left) where the next full-height piece starts.
    let mut cursor = (ends[0], ends[1]);

    for op in openings {
        let op_start = op.offset.max(cursor.0.max(cursor.1));
        // Openings stop where the joined end faces begin, so pieces stay convex.
        let op_end = (op.offset + op.width).min(ends[2].min(ends[3]));
        if op_start >= op_end {
            continue;
        }

        // Full-height segment before this opening
        if op_start > cursor.0.min(cursor.1) {
            piece(&mut vertices, &mut indices, cursor, (op_start, op_start));
        }

        let (x0, x1) = (op_start as f32, op_end as f32);
        // Below-opening segment (for windows with sill_height > 0)
        if op.bottom > 0.0 {
            append_box(
                &mut vertices,
                &mut indices,
                x0,
                x1,
                -half_t,
                half_t,
                0.0,
                op.bottom as f32,
            );
        }

        // Above-opening segment (if opening doesn't reach wall top)
        if (op.top as f32) < wall_height {
            append_box(
                &mut vertices,
                &mut indices,
                x0,
                x1,
                -half_t,
                half_t,
                op.top as f32,
                wall_height,
            );
        }

        cursor = (op_end, op_end);
    }

    // Remaining full-height segment after last opening
    if ends[2].max(ends[3]) > cursor.0.max(cursor.1) {
        piece(&mut vertices, &mut indices, cursor, (ends[2], ends[3]));
    }

    (vertices, indices)
}

// ---------------------------------------------------------------------------
// Mesh generators for each element type
// ---------------------------------------------------------------------------

fn wall_mesh(elem: &ResolvedElement, elevation: f64, ends: Option<&WallEnds>) -> Option<SceneMesh> {
    let axis = WallAxis::of(elem, default_curve_tolerance())?;
    let height = get_number(&elem.properties, "height").unwrap_or(2.8);
    let thickness = get_number(&elem.properties, "thickness").unwrap_or(0.2);

    // Collect door/window openings from children
    let openings = collect_wall_openings(elem, &axis, height);

    if !axis.is_straight() {
        let (vertices, indices) =
            curved_wall_vertices(&axis, thickness, elevation, height, &openings);
        return Some(SceneMesh {
            vertices,
            indices,
            color: element_color(elem.element_type),
        });
    }

    let length = axis.length();
    let (verts, indices) = segmented_wall_vertices(
        ends.copied().unwrap_or([0.0, 0.0, length, length]),
        thickness as f32 / 2.0,
        height as f32,
        &openings,
    );

    Some(SceneMesh {
        vertices: place(verts, axis.frame(0.0), elevation),
        indices,
        color: element_color(elem.element_type),
    })
}

/// Pieces of a curved wall between and around its openings, each the wall
/// outline over its stretch of axis, extruded over its height range.
fn curved_wall_vertices(
    axis: &WallAxis,
    thickness: f64,
    elevation: f64,
    height: f64,
    openings: &[WallOpening],
) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut piece = |s0: f64, s1: f64, z0: f64, z1: f64| {
        let ring = geometry::wall_outline(&axis.sub(s0, s1), thickness);
        let (v, i) = extrude_polygon(&ring, (elevation + z0) as f32, (elevation + z1) as f32);
        let base = vertices.len() as u32;
        vertices.extend(v);
        indices.extend(i.into_iter().map(|x| x + base));
    };
    let mut cursor = 0.0;
    for op in openings {
        let (start, end) = (
            op.offset.max(cursor),
            (op.offset + op.width).min(axis.length()),
        );
        if start >= end {
            continue;
        }
        if start > cursor {
            piece(cursor, start, 0.0, height);
        }
        if op.bottom > 0.0 {
            piece(start, end, 0.0, op.bottom);
        }
        if op.top < height {
            piece(start, end, op.top, height);
        }
        cursor = end;
    }
    if axis.length() > cursor {
        piece(cursor, axis.length(), 0.0, height);
    }
    (vertices, indices)
}

fn slab_mesh(elem: &ResolvedElement, elevation: f64) -> Option<SceneMesh> {
    let boundary = get_polygon(&elem.properties, "boundary")?;
    let thickness = get_number(&elem.properties, "thickness").unwrap_or(0.2);

    if boundary.len() < 3 {
        return None;
    }

    let oz = elevation as f32;

    let holes = holes_with_openings(elem, &boundary);

    let (vertices, indices) = if !holes.is_empty() {
        extrude_polygon_with_holes(&boundary, &holes, oz - thickness as f32, oz)
    } else {
        // Extrude the polygon downward (slab sits on top of elevation)
        extrude_polygon(&boundary, oz - thickness as f32, oz)
    };

    Some(SceneMesh {
        vertices,
        indices,
        color: element_color(ElementType::Slab),
    })
}

/// `holes:` of the outline plus the `Opening` prisms inside it, so a
/// decompiled slab keeps its stair wells and courtyards and a generated
/// penetration shows. The triangulator needs disjoint holes; largest first,
/// so an opening nested in another (Revit writes both) is skipped as
/// already void.
fn holes_with_openings(elem: &ResolvedElement, boundary: &[(f64, f64)]) -> Vec<Vec<(f64, f64)>> {
    let mut holes = outline_holes(&elem.properties);
    let mut openings: Vec<Vec<(f64, f64)>> = elem
        .children
        .iter()
        .filter(|c| c.element_type == ElementType::Opening)
        .filter_map(|c| get_polygon(&c.properties, "boundary"))
        .filter(|o| o.len() >= 3 && o.iter().all(|&(x, y)| point_in_polygon(x, y, boundary)))
        .collect();
    openings.sort_by(|a, b| geometry::polygon_area(b).total_cmp(&geometry::polygon_area(a)));
    for opening in openings {
        if !holes.iter().any(|h| rings_overlap(&opening, h)) {
            holes.push(opening);
        }
    }
    holes
}

fn column_mesh(elem: &ResolvedElement, elevation: f64) -> Option<SceneMesh> {
    let (x, y) = get_point(&elem.properties, "position")?;
    let height = get_number(&elem.properties, "height").unwrap_or(3.0);
    let (profile_type, params) = get_profile(&elem.properties, "profile")?;

    let oz = elevation as f32;
    let h = height as f32;

    match profile_type.as_str() {
        "circle" if !params.is_empty() => {
            // Tessellated cylinder using the reusable cylinder_mesh primitive
            let radius = params[0];
            let (vertices, indices) = cylinder_mesh(x, y, radius, oz, oz + h, 16);
            Some(SceneMesh {
                vertices,
                indices,
                color: element_color(ElementType::Column),
            })
        }
        _ => {
            let (w, d) = if profile_type == "rectangle" && params.len() >= 2 {
                (params[0], params[1])
            } else {
                (0.3, 0.3)
            };
            let rotation = get_number(&elem.properties, "rotation").unwrap_or(0.0);
            Some(SceneMesh {
                vertices: centred_box(x, y, w, d, oz, oz + h, rotation),
                indices: box_indices(),
                color: element_color(ElementType::Column),
            })
        }
    }
}

fn beam_mesh(elem: &ResolvedElement, elevation: f64) -> Option<SceneMesh> {
    let (from, to, z) = elem
        .properties
        .iter()
        .find(|p| p.key == "from")
        .and_then(|p| match &p.value {
            ResolvedValue::Axis { from, to, z, .. } => Some((*from, *to, z.unwrap_or((0.0, 0.0)))),
            _ => None,
        })?;
    let (profile_type, params) = get_profile(&elem.properties, "profile")?;

    let (hw, hh) = match profile_type.as_str() {
        "rectangle" if params.len() >= 2 => (params[0] / 2.0, params[1] / 2.0),
        "circle" if !params.is_empty() => (params[0], params[0]),
        _ => (0.15, 0.25),
    };

    // Box along the (possibly sloped) axis: width horizontal, depth in the vertical
    // plane through the axis, both turned by `rotation` about the axis.
    let d = [to.0 - from.0, to.1 - from.1, z.1 - z.0];
    let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if length < 1e-9 {
        return None;
    }
    let ax = [d[0] / length, d[1] / length, d[2] / length];
    let horizontal = (ax[0] * ax[0] + ax[1] * ax[1]).sqrt();
    let across = if horizontal < 1e-9 {
        [1.0, 0.0, 0.0]
    } else {
        [-ax[1] / horizontal, ax[0] / horizontal, 0.0]
    };
    let up = [
        ax[1] * across[2] - ax[2] * across[1],
        ax[2] * across[0] - ax[0] * across[2],
        ax[0] * across[1] - ax[1] * across[0],
    ];
    let r = get_number(&elem.properties, "rotation")
        .unwrap_or(0.0)
        .to_radians();
    let (c, s) = (r.cos(), r.sin());
    let across_r: [f64; 3] = std::array::from_fn(|i| across[i] * c + up[i] * s);
    let up_r: [f64; 3] = std::array::from_fn(|i| up[i] * c - across[i] * s);
    let origin = [from.0, from.1, elevation + z.0];
    let (hw, hh) = (hw as f32, hh as f32);
    let verts = box_vertices(0.0, length as f32, -hw, hw, -hh, hh);
    let map = |v: [f32; 3], o: [f64; 3]| -> [f32; 3] {
        std::array::from_fn(|i| {
            (o[i] + ax[i] * v[0] as f64 + across_r[i] * v[1] as f64 + up_r[i] * v[2] as f64) as f32
        })
    };
    let vertices = verts
        .into_iter()
        .map(|v| Vertex {
            position: map(v.position, origin),
            normal: map(v.normal, [0.0; 3]),
        })
        .collect();

    Some(SceneMesh {
        vertices,
        indices: box_indices(),
        color: element_color(ElementType::Beam),
    })
}

fn roof_mesh(elem: &ResolvedElement, elevation: f64, storey_height: f64) -> Option<SceneMesh> {
    let boundary = get_polygon(&elem.properties, "boundary")?;
    let ridge_height = get_number(&elem.properties, "ridge_height").unwrap_or(0.0);
    let roof_type = get_identifier(&elem.properties, "type").unwrap_or("flat");
    let ridge_dir = get_identifier(&elem.properties, "ridge_direction").unwrap_or("x");

    if boundary.len() < 3 {
        return None;
    }

    // Place roof base at wall-top level (elevation + storey_height)
    let oz = (elevation + storey_height) as f32;

    match roof_type {
        "gable" | "hip" | "shed" | "mansard" => {
            let (vertices, indices) =
                pitched_roof_mesh(&boundary, oz, ridge_height as f32, roof_type, ridge_dir);
            Some(SceneMesh {
                vertices,
                indices,
                color: element_color(ElementType::Roof),
            })
        }
        _ => {
            let thickness = get_number(&elem.properties, "thickness").unwrap_or(0.15) as f32;
            let holes = outline_holes(&elem.properties);
            let (vertices, indices) = if holes.is_empty() {
                extrude_polygon(&boundary, oz, oz + thickness)
            } else {
                extrude_polygon_with_holes(&boundary, &holes, oz, oz + thickness)
            };
            Some(SceneMesh {
                vertices,
                indices,
                color: element_color(ElementType::Roof),
            })
        }
    }
}

// ── Polygon roof helpers ────────────────────────────────────────────────────

/// Ear-clipping triangulation for a simple polygon (CCW or CW).
/// Returns index triples into `poly`. Works correctly for concave polygons.
pub(super) fn ear_clip_triangulate(poly: &[(f64, f64)]) -> Vec<[usize; 3]> {
    let n = poly.len();
    if n < 3 {
        return Vec::new();
    }
    if n == 3 {
        return vec![[0, 1, 2]];
    }

    // Ensure CCW orientation; reverse indices if CW.
    let area2: f64 = (0..n)
        .map(|i| {
            let (x0, y0) = poly[i];
            let (x1, y1) = poly[(i + 1) % n];
            x0 * y1 - x1 * y0
        })
        .sum();
    let mut idx: Vec<usize> = if area2 > 0.0 {
        (0..n).collect()
    } else {
        (0..n).rev().collect()
    };

    let mut triangles = Vec::new();
    let mut safety = idx.len() * idx.len() + 10; // O(n²) guard

    while idx.len() > 3 && safety > 0 {
        safety -= 1;
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (poly[ia], poly[ib], poly[ic]);

            // Vertex b must be convex (cross product > 0 for CCW polygon)
            let cross = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            if cross <= 1e-10 {
                continue;
            }

            // No other polygon vertex may lie inside triangle (a, b, c)
            let is_ear = idx.iter().all(|&j| {
                if j == ia || j == ib || j == ic {
                    return true;
                }
                !point_in_triangle_2d(poly[j], a, b, c)
            });

            if is_ear {
                triangles.push([ia, ib, ic]);
                idx.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            break;
        } // degenerate / self-intersecting
    }
    if idx.len() == 3 {
        triangles.push([idx[0], idx[1], idx[2]]);
    }
    triangles
}

fn point_in_triangle_2d(p: (f64, f64), a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
    let s = |p1: (f64, f64), p2: (f64, f64), p3: (f64, f64)| -> f64 {
        (p1.0 - p3.0) * (p2.1 - p3.1) - (p2.0 - p3.0) * (p1.1 - p3.1)
    };
    let d1 = s(p, a, b);
    let d2 = s(p, b, c);
    let d3 = s(p, c, a);
    let has_neg = d1 < -1e-10 || d2 < -1e-10 || d3 < -1e-10;
    let has_pos = d1 > 1e-10 || d2 > 1e-10 || d3 > 1e-10;
    !(has_neg && has_pos)
}

/// Whether two simple polygons share interior: a vertex of one lies inside
/// the other, or two edges properly cross.
fn rings_overlap(a: &[(f64, f64)], b: &[(f64, f64)]) -> bool {
    let inside =
        |p: &[(f64, f64)], q: &[(f64, f64)]| p.iter().any(|&(x, y)| point_in_polygon(x, y, q));
    let edges = |p: &[(f64, f64)]| {
        let n = p.len();
        (0..n)
            .map(move |i| (p[i], p[(i + 1) % n]))
            .collect::<Vec<_>>()
    };
    let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    };
    let crosses = |(p1, p2): ((f64, f64), (f64, f64)), (q1, q2): ((f64, f64), (f64, f64))| {
        cross(p1, p2, q1) * cross(p1, p2, q2) < 0.0 && cross(q1, q2, p1) * cross(q1, q2, p2) < 0.0
    };
    let eb = edges(b);
    inside(a, b)
        || inside(b, a)
        || edges(a)
            .iter()
            .any(|&ea| eb.iter().any(|&e| crosses(ea, e)))
}

/// Distance from point (px, py) to a line segment (ax,ay)–(bx,by).
fn dist_point_to_segment(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-10 {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }
    let t = ((px - ax) * dx + (py - ay) * dy).clamp(0.0, len_sq) / len_sq;
    ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt()
}

/// Find the pole of inaccessibility: the interior point that maximises the
/// minimum distance to any polygon edge. Used as the tent-roof apex so that
/// no face degenerates to zero area.
fn polygon_apex(poly: &[(f64, f64)]) -> (f64, f64) {
    let (mut min_x, mut max_x) = (f64::MAX, f64::MIN);
    let (mut min_y, mut max_y) = (f64::MAX, f64::MIN);
    for &(x, y) in poly {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    let step = ((max_x - min_x).min(max_y - min_y) * 0.1).max(0.2);

    let cx = poly.iter().map(|p| p.0).sum::<f64>() / poly.len() as f64;
    let cy = poly.iter().map(|p| p.1).sum::<f64>() / poly.len() as f64;
    let mut best = (cx, cy);
    let mut best_dist = -1.0_f64;

    let mut y = min_y + step * 0.5;
    while y <= max_y {
        let mut x = min_x + step * 0.5;
        while x <= max_x {
            if point_in_polygon(x, y, poly) {
                let d = poly
                    .windows(2)
                    .chain(std::iter::once([poly[poly.len() - 1], poly[0]].as_slice()))
                    .map(|w| dist_point_to_segment(x, y, w[0].0, w[0].1, w[1].0, w[1].1))
                    .fold(f64::MAX, f64::min);
                if d > best_dist {
                    best_dist = d;
                    best = (x, y);
                }
            }
            x += step;
        }
        y += step;
    }
    best
}

/// Tent (pyramid) roof mesh for non-rectangular polygons.
/// Each boundary edge forms one triangular face rising to the apex.
fn polygon_tent_roof(
    boundary: &[(f64, f64)],
    oz: f32,
    ridge_height: f32,
) -> (Vec<Vertex>, Vec<u32>) {
    let (ax, ay) = polygon_apex(boundary);
    let apex_z = oz + ridge_height;
    let n = boundary.len();

    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for i in 0..n {
        let (x1, y1) = (boundary[i].0 as f32, boundary[i].1 as f32);
        let (x2, y2) = (
            boundary[(i + 1) % n].0 as f32,
            boundary[(i + 1) % n].1 as f32,
        );
        let (apx, apy) = (ax as f32, ay as f32);

        // Skip degenerate faces (apex on the edge line)
        let area2 = (x2 - x1) * (apy - y1) - (apx - x1) * (y2 - y1);
        if area2.abs() < 1e-4 {
            continue;
        }

        // Face normal: edge × (apex − v1)
        let (ex, ey) = (x2 - x1, y2 - y1);
        let (tx, ty, tz) = (apx - x1, apy - y1, apex_z - oz);
        let nx = ey * tz;
        let ny = -ex * tz;
        let nz = ex * ty - ey * tx;
        let len = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-6);

        // Ensure normal points upward; flip winding if needed
        let (nx, ny, nz, p1, p2) = if nz < 0.0 {
            (-nx / len, -ny / len, -nz / len, (x2, y2), (x1, y1))
        } else {
            (nx / len, ny / len, nz / len, (x1, y1), (x2, y2))
        };

        let base = vertices.len() as u32;
        vertices.push(Vertex {
            position: [p1.0, p1.1, oz],
            normal: [nx, ny, nz],
        });
        vertices.push(Vertex {
            position: [p2.0, p2.1, oz],
            normal: [nx, ny, nz],
        });
        vertices.push(Vertex {
            position: [apx, apy, apex_z],
            normal: [nx, ny, nz],
        });
        indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    // Bottom face: ear-clipped polygon at oz facing downward (correct for concave footprints)
    let bot_base = vertices.len() as u32;
    for &(x, y) in boundary {
        vertices.push(Vertex {
            position: [x as f32, y as f32, oz],
            normal: [0.0, 0.0, -1.0],
        });
    }
    for tri in ear_clip_triangulate(boundary) {
        // Reverse winding so the face is visible from below (-Z)
        indices.extend_from_slice(&[
            bot_base + tri[0] as u32,
            bot_base + tri[2] as u32,
            bot_base + tri[1] as u32,
        ]);
    }

    (vertices, indices)
}

/// Flat polygon roof (ridge_height = 0) for non-rectangular footprints.
/// Uses ear clipping so concave shapes (H, T, L, U …) are correctly tiled.
fn flat_polygon_roof(boundary: &[(f64, f64)], oz: f32) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let tris = ear_clip_triangulate(boundary);

    // Top face — visible from above (+Z)
    let top_base = vertices.len() as u32;
    for &(x, y) in boundary {
        vertices.push(Vertex {
            position: [x as f32, y as f32, oz],
            normal: [0.0, 0.0, 1.0],
        });
    }
    for tri in &tris {
        indices.extend_from_slice(&[
            top_base + tri[0] as u32,
            top_base + tri[1] as u32,
            top_base + tri[2] as u32,
        ]);
    }

    // Bottom face — visible from below (-Z), offset 1 mm to avoid z-fighting
    let bot_base = vertices.len() as u32;
    for &(x, y) in boundary {
        vertices.push(Vertex {
            position: [x as f32, y as f32, oz - 0.001],
            normal: [0.0, 0.0, -1.0],
        });
    }
    for tri in &tris {
        indices.extend_from_slice(&[
            bot_base + tri[0] as u32,
            bot_base + tri[2] as u32,
            bot_base + tri[1] as u32,
        ]);
    }

    (vertices, indices)
}

/// Generate a pitched roof mesh from the boundary bounding box.
/// Supports gable, hip, shed, and mansard roof types.
/// For non-rectangular polygons (> 4 vertices), uses a polygon tent approach.
fn pitched_roof_mesh(
    boundary: &[(f64, f64)],
    oz: f32,
    ridge_height: f32,
    roof_type: &str,
    ridge_dir: &str,
) -> (Vec<Vertex>, Vec<u32>) {
    // Non-rectangular footprints: use ear-clipping for flat roofs, tent for pitched
    if boundary.len() > 4 {
        if ridge_height.abs() < 1e-6 {
            return flat_polygon_roof(boundary, oz);
        }
        return polygon_tent_roof(boundary, oz, ridge_height);
    }

    // Compute bounding box of the boundary polygon
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for &(x, y) in boundary {
        let (x, y) = (x as f32, y as f32);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }

    let rh = oz + ridge_height;

    // Swap axes: if ridge runs along y, swap x/y for computation then swap back
    let swap = ridge_dir == "y";
    let (x0, x1, y0, y1) = if swap {
        (min_y, max_y, min_x, max_x)
    } else {
        (min_x, max_x, min_y, max_y)
    };
    let mid_y = (y0 + y1) / 2.0;

    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // Helper to emit a vertex, swapping x/y back if needed
    let mut push_vert = |x: f32, y: f32, z: f32, nx: f32, ny: f32, nz: f32| -> u32 {
        let idx = vertices.len() as u32;
        let (px, py, nnx, nny) = if swap { (y, x, ny, nx) } else { (x, y, nx, ny) };
        vertices.push(Vertex {
            position: [px, py, z],
            normal: [nnx, nny, nz],
        });
        idx
    };

    match roof_type {
        "gable" => {
            // South slope
            let slope_h = ((mid_y - y0).powi(2) + ridge_height.powi(2)).sqrt();
            let sn = [0.0, -ridge_height / slope_h, (mid_y - y0) / slope_h];
            let a = push_vert(x0, y0, oz, sn[0], sn[1], sn[2]);
            let b = push_vert(x1, y0, oz, sn[0], sn[1], sn[2]);
            let c = push_vert(x1, mid_y, rh, sn[0], sn[1], sn[2]);
            let d = push_vert(x0, mid_y, rh, sn[0], sn[1], sn[2]);
            indices.extend_from_slice(&[a, b, c, a, c, d]);

            // North slope
            let nn = [0.0, ridge_height / slope_h, (mid_y - y0) / slope_h];
            let a = push_vert(x1, y1, oz, nn[0], nn[1], nn[2]);
            let b = push_vert(x0, y1, oz, nn[0], nn[1], nn[2]);
            let c = push_vert(x0, mid_y, rh, nn[0], nn[1], nn[2]);
            let d = push_vert(x1, mid_y, rh, nn[0], nn[1], nn[2]);
            indices.extend_from_slice(&[a, b, c, a, c, d]);

            // West gable triangle
            let a = push_vert(x0, y0, oz, -1.0, 0.0, 0.0);
            let b = push_vert(x0, y1, oz, -1.0, 0.0, 0.0);
            let c = push_vert(x0, mid_y, rh, -1.0, 0.0, 0.0);
            indices.extend_from_slice(&[a, b, c]);

            // East gable triangle
            let a = push_vert(x1, y1, oz, 1.0, 0.0, 0.0);
            let b = push_vert(x1, y0, oz, 1.0, 0.0, 0.0);
            let c = push_vert(x1, mid_y, rh, 1.0, 0.0, 0.0);
            indices.extend_from_slice(&[a, b, c]);

            // Bottom face
            let a = push_vert(x0, y0, oz, 0.0, 0.0, -1.0);
            let b = push_vert(x1, y0, oz, 0.0, 0.0, -1.0);
            let c = push_vert(x1, y1, oz, 0.0, 0.0, -1.0);
            let d = push_vert(x0, y1, oz, 0.0, 0.0, -1.0);
            indices.extend_from_slice(&[a, b, c, a, c, d]);
        }
        "hip" => {
            // Ridge is shortened: inset from each end by half-width
            let half_w = (y1 - y0) / 2.0;
            let ridge_x0 = (x0 + half_w).min(x0 + (x1 - x0) / 2.0);
            let ridge_x1 = (x1 - half_w).max(x0 + (x1 - x0) / 2.0);

            let slope_h = ((mid_y - y0).powi(2) + ridge_height.powi(2)).sqrt();
            let sn = [0.0, -ridge_height / slope_h, (mid_y - y0) / slope_h];
            let nn = [0.0, ridge_height / slope_h, (mid_y - y0) / slope_h];

            // South slope (trapezoid)
            let a = push_vert(x0, y0, oz, sn[0], sn[1], sn[2]);
            let b = push_vert(x1, y0, oz, sn[0], sn[1], sn[2]);
            let c = push_vert(ridge_x1, mid_y, rh, sn[0], sn[1], sn[2]);
            let d = push_vert(ridge_x0, mid_y, rh, sn[0], sn[1], sn[2]);
            indices.extend_from_slice(&[a, b, c, a, c, d]);

            // North slope (trapezoid)
            let a = push_vert(x1, y1, oz, nn[0], nn[1], nn[2]);
            let b = push_vert(x0, y1, oz, nn[0], nn[1], nn[2]);
            let c = push_vert(ridge_x0, mid_y, rh, nn[0], nn[1], nn[2]);
            let d = push_vert(ridge_x1, mid_y, rh, nn[0], nn[1], nn[2]);
            indices.extend_from_slice(&[a, b, c, a, c, d]);

            // West hip triangle
            let end_slope_h = ((ridge_x0 - x0).powi(2) + ridge_height.powi(2)).sqrt();
            let wn = if end_slope_h > 0.0 {
                [
                    -ridge_height / end_slope_h,
                    0.0,
                    (ridge_x0 - x0) / end_slope_h,
                ]
            } else {
                [-1.0, 0.0, 0.0]
            };
            let a = push_vert(x0, y1, oz, wn[0], wn[1], wn[2]);
            let b = push_vert(x0, y0, oz, wn[0], wn[1], wn[2]);
            let c = push_vert(ridge_x0, mid_y, rh, wn[0], wn[1], wn[2]);
            indices.extend_from_slice(&[a, b, c]);

            // East hip triangle
            let en = if end_slope_h > 0.0 {
                [
                    ridge_height / end_slope_h,
                    0.0,
                    (ridge_x0 - x0) / end_slope_h,
                ]
            } else {
                [1.0, 0.0, 0.0]
            };
            let a = push_vert(x1, y0, oz, en[0], en[1], en[2]);
            let b = push_vert(x1, y1, oz, en[0], en[1], en[2]);
            let c = push_vert(ridge_x1, mid_y, rh, en[0], en[1], en[2]);
            indices.extend_from_slice(&[a, b, c]);

            // Bottom face
            let a = push_vert(x0, y0, oz, 0.0, 0.0, -1.0);
            let b = push_vert(x1, y0, oz, 0.0, 0.0, -1.0);
            let c = push_vert(x1, y1, oz, 0.0, 0.0, -1.0);
            let d = push_vert(x0, y1, oz, 0.0, 0.0, -1.0);
            indices.extend_from_slice(&[a, b, c, a, c, d]);
        }
        "shed" => {
            // Slope from y0 (low) to y1 (high)
            let slope_h = ((y1 - y0).powi(2) + ridge_height.powi(2)).sqrt();
            let sn = [0.0, -ridge_height / slope_h, (y1 - y0) / slope_h];

            // Top slope
            let a = push_vert(x0, y0, oz, sn[0], sn[1], sn[2]);
            let b = push_vert(x1, y0, oz, sn[0], sn[1], sn[2]);
            let c = push_vert(x1, y1, rh, sn[0], sn[1], sn[2]);
            let d = push_vert(x0, y1, rh, sn[0], sn[1], sn[2]);
            indices.extend_from_slice(&[a, b, c, a, c, d]);

            // Left side (x0)
            let a = push_vert(x0, y0, oz, -1.0, 0.0, 0.0);
            let b = push_vert(x0, y1, oz, -1.0, 0.0, 0.0);
            let c = push_vert(x0, y1, rh, -1.0, 0.0, 0.0);
            indices.extend_from_slice(&[a, b, c]);

            // Right side (x1)
            let a = push_vert(x1, y1, oz, 1.0, 0.0, 0.0);
            let b = push_vert(x1, y0, oz, 1.0, 0.0, 0.0);
            let c = push_vert(x1, y1, rh, 1.0, 0.0, 0.0);
            indices.extend_from_slice(&[a, b, c]);

            // Back wall (y1)
            let a = push_vert(x0, y1, oz, 0.0, 1.0, 0.0);
            let b = push_vert(x1, y1, oz, 0.0, 1.0, 0.0);
            let c = push_vert(x1, y1, rh, 0.0, 1.0, 0.0);
            let d = push_vert(x0, y1, rh, 0.0, 1.0, 0.0);
            indices.extend_from_slice(&[a, b, c, a, c, d]);

            // Bottom face
            let a = push_vert(x0, y0, oz, 0.0, 0.0, -1.0);
            let b = push_vert(x1, y0, oz, 0.0, 0.0, -1.0);
            let c = push_vert(x1, y1, oz, 0.0, 0.0, -1.0);
            let d = push_vert(x0, y1, oz, 0.0, 0.0, -1.0);
            indices.extend_from_slice(&[a, b, c, a, c, d]);
        }
        _ => {
            // mansard or unknown pitched: treat like gable
            return pitched_roof_mesh(boundary, oz, ridge_height, "gable", ridge_dir);
        }
    }

    (vertices, indices)
}

fn stair_mesh(elem: &ResolvedElement, elevation: f64, storey_height: f64) -> Option<SceneMesh> {
    let p = &elem.properties;
    let (x, y) = get_point(p, "position").unwrap_or((0.0, 0.0));
    let (sin, cos) = get_number(p, "rotation")
        .unwrap_or(0.0)
        .to_radians()
        .sin_cos();
    let flight = geometry::Flight::of(p, storey_height);
    // Side profile (u along the run, v up) extruded across the width (w).
    let (vertices, indices) = extrude_polygon(&flight.profile(), 0.0, flight.width as f32);
    // (u, v, w) -> plan (x across, y along the run, z up), then rotated.
    let turn = |[u, v, w]: [f32; 3]| {
        let (u, w) = (u as f64, w as f64);
        [(w * cos - u * sin) as f32, (w * sin + u * cos) as f32, v]
    };
    let vertices = vertices
        .into_iter()
        .map(|v| {
            let [px, py, pz] = turn(v.position);
            Vertex {
                position: [px + x as f32, py + y as f32, pz + elevation as f32],
                normal: turn(v.normal),
            }
        })
        .collect();
    Some(SceneMesh {
        vertices,
        indices,
        color: element_color(ElementType::Stair),
    })
}

fn railing_mesh(elem: &ResolvedElement, elevation: f64) -> Option<SceneMesh> {
    let height = get_number(&elem.properties, "height").unwrap_or(1.0);
    let thickness = get_number(&elem.properties, "thickness").unwrap_or(0.05);

    // If axis (from: ... to: ...) is given, use it for positioning and length
    if let Some((from, to)) = get_axis(&elem.properties) {
        let length = geometry::wall_length(from, to);
        let angle = geometry::wall_angle(from, to);
        let half_t = thickness as f32 / 2.0;
        let verts = box_vertices(0.0, length as f32, -half_t, half_t, 0.0, height as f32);
        let indices = box_indices();

        let cos_a = angle.cos() as f32;
        let sin_a = angle.sin() as f32;
        let ox = from.0 as f32;
        let oy = from.1 as f32;
        let oz = elevation as f32;

        let vertices: Vec<Vertex> = verts
            .into_iter()
            .map(|v| {
                let rx = v.position[0] * cos_a - v.position[1] * sin_a + ox;
                let ry = v.position[0] * sin_a + v.position[1] * cos_a + oy;
                let rz = v.position[2] + oz;
                let nx = v.normal[0] * cos_a - v.normal[1] * sin_a;
                let ny = v.normal[0] * sin_a + v.normal[1] * cos_a;
                Vertex {
                    position: [rx, ry, rz],
                    normal: [nx, ny, v.normal[2]],
                }
            })
            .collect();

        return Some(SceneMesh {
            vertices,
            indices,
            color: element_color(ElementType::Railing),
        });
    }

    // Fallback: `length` from `position` in the `rotation` direction
    let length = get_number(&elem.properties, "length").unwrap_or(1.0);
    let (x, y) = get_point(&elem.properties, "position").unwrap_or((0.0, 0.0));
    let rotation = get_number(&elem.properties, "rotation").unwrap_or(0.0);
    let (sin_r, cos_r) = rotation.to_radians().sin_cos();
    let (cx, cy) = (x + cos_r * length / 2.0, y + sin_r * length / 2.0);
    let oz = elevation as f32;
    let verts = centred_box(cx, cy, length, thickness, oz, oz + height as f32, rotation);

    Some(SceneMesh {
        vertices: verts,
        indices: box_indices(),
        color: element_color(ElementType::Railing),
    })
}

fn ramp_mesh(elem: &ResolvedElement, elevation: f64) -> Option<SceneMesh> {
    let width = get_number(&elem.properties, "width").unwrap_or(1.2);
    let thickness = 0.2_f64;

    // If axis (from: ... to: ...) is given, use it for positioning and length
    if let Some((from, to)) = get_axis(&elem.properties) {
        let length = geometry::wall_length(from, to);
        let angle = geometry::wall_angle(from, to);
        let half_w = width as f32 / 2.0;
        let verts = box_vertices(0.0, length as f32, -half_w, half_w, 0.0, thickness as f32);
        let indices = box_indices();

        let cos_a = angle.cos() as f32;
        let sin_a = angle.sin() as f32;
        let ox = from.0 as f32;
        let oy = from.1 as f32;
        let oz = elevation as f32;

        let vertices: Vec<Vertex> = verts
            .into_iter()
            .map(|v| {
                let rx = v.position[0] * cos_a - v.position[1] * sin_a + ox;
                let ry = v.position[0] * sin_a + v.position[1] * cos_a + oy;
                let rz = v.position[2] + oz;
                let nx = v.normal[0] * cos_a - v.normal[1] * sin_a;
                let ny = v.normal[0] * sin_a + v.normal[1] * cos_a;
                Vertex {
                    position: [rx, ry, rz],
                    normal: [nx, ny, v.normal[2]],
                }
            })
            .collect();

        return Some(SceneMesh {
            vertices,
            indices,
            color: element_color(ElementType::Ramp),
        });
    }

    // Fallback: `length` from `position` in the `rotation` direction
    let length = get_number(&elem.properties, "length").unwrap_or(5.0);
    let (x, y) = get_point(&elem.properties, "position").unwrap_or((0.0, 0.0));
    let rotation = get_number(&elem.properties, "rotation").unwrap_or(0.0);
    let (sin_r, cos_r) = rotation.to_radians().sin_cos();
    let (cx, cy) = (x + cos_r * length / 2.0, y + sin_r * length / 2.0);
    let oz = elevation as f32;
    let verts = centred_box(cx, cy, length, width, oz, oz + thickness as f32, rotation);

    Some(SceneMesh {
        vertices: verts,
        indices: box_indices(),
        color: element_color(ElementType::Ramp),
    })
}

fn proxy_mesh(elem: &ResolvedElement, elevation: f64) -> Option<SceneMesh> {
    let (x, y) = get_point(&elem.properties, "position")?;
    let w = get_number(&elem.properties, "width").unwrap_or(1.0);
    let d = get_number(&elem.properties, "depth").unwrap_or(1.0);
    let h = get_number(&elem.properties, "height").unwrap_or(1.0);
    let rotation = get_number(&elem.properties, "rotation").unwrap_or(0.0);
    let oz = elevation as f32;

    Some(SceneMesh {
        vertices: centred_box(x, y, w, d, oz, oz + h as f32, rotation),
        indices: box_indices(),
        color: element_color(ElementType::Proxy),
    })
}

/// Box of `w` x `d` centred on (x, y), turned `rotation_deg` about its centre.
fn centred_box(x: f64, y: f64, w: f64, d: f64, z0: f32, z1: f32, rotation_deg: f64) -> Vec<Vertex> {
    let (hw, hd) = ((w / 2.0) as f32, (d / 2.0) as f32);
    let (s, c) = (rotation_deg.to_radians() as f32).sin_cos();
    let turn = |v: [f32; 3]| [v[0] * c - v[1] * s, v[0] * s + v[1] * c, v[2]];
    box_vertices(-hw, hw, -hd, hd, z0, z1)
        .into_iter()
        .map(|v| {
            let p = turn(v.position);
            Vertex {
                position: [p[0] + x as f32, p[1] + y as f32, p[2]],
                normal: turn(v.normal),
            }
        })
        .collect()
}

fn grid_mesh(elem: &ResolvedElement, elevation: f64) -> Option<SceneMesh> {
    let (px, py) = get_point(&elem.properties, "position").unwrap_or((0.0, 0.0));
    let x_spacing = get_number(&elem.properties, "x_spacing").unwrap_or(1.0);
    let y_spacing = get_number(&elem.properties, "y_spacing").unwrap_or(1.0);
    let x_count = get_number(&elem.properties, "x_count").unwrap_or(2.0) as usize;
    let y_count = get_number(&elem.properties, "y_count").unwrap_or(2.0) as usize;

    let x_total = (x_count.saturating_sub(1)) as f64 * x_spacing;
    let y_total = (y_count.saturating_sub(1)) as f64 * y_spacing;
    let z = elevation as f32;
    let thickness = 0.02_f32; // thin line representation
    let height = 0.01_f32;
    let color = element_color(ElementType::Grid);

    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // X-direction axes (vertical lines in plan)
    for i in 0..x_count {
        let ax = (px + i as f64 * x_spacing) as f32;
        let y0 = py as f32;
        let y1 = (py + y_total) as f32;
        let base = vertices.len() as u32;
        let verts = box_vertices(ax - thickness, ax + thickness, y0, y1, z, z + height);
        vertices.extend_from_slice(&verts);
        for idx in box_indices() {
            indices.push(idx + base);
        }
    }

    // Y-direction axes (horizontal lines in plan)
    for i in 0..y_count {
        let ay = (py + i as f64 * y_spacing) as f32;
        let x0 = px as f32;
        let x1 = (px + x_total) as f32;
        let base = vertices.len() as u32;
        let verts = box_vertices(x0, x1, ay - thickness, ay + thickness, z, z + height);
        vertices.extend_from_slice(&verts);
        for idx in box_indices() {
            indices.push(idx + base);
        }
    }

    if vertices.is_empty() {
        return None;
    }

    Some(SceneMesh {
        vertices,
        indices,
        color,
    })
}

fn furniture_mesh(elem: &ResolvedElement, elevation: f64) -> Option<SceneMesh> {
    let (x, y) = get_point(&elem.properties, "position").unwrap_or((0.0, 0.0));
    let w = get_number(&elem.properties, "width").unwrap_or(1.0);
    let d = get_number(&elem.properties, "depth").unwrap_or(1.0);
    let h = get_number(&elem.properties, "height").unwrap_or(0.8);
    let rotation = get_number(&elem.properties, "rotation").unwrap_or(0.0);
    let oz = elevation as f32;

    Some(SceneMesh {
        vertices: centred_box(x, y, w, d, oz, oz + h as f32, rotation),
        indices: box_indices(),
        color: element_color(ElementType::Furniture),
    })
}

/// Generate scene node for a furniture element, expanding asset parts into child nodes.
fn furniture_asset_node(
    elem: &ResolvedElement,
    elevation: f64,
    asset_libraries: &[model::ResolvedAssetLibrary],
) -> Option<SceneNode> {
    let (x, y) = get_point(&elem.properties, "position").unwrap_or((0.0, 0.0));
    let rotation_deg = get_number(&elem.properties, "rotation").unwrap_or(0.0);
    let rotation_rad = (rotation_deg as f32).to_radians();

    // Look up asset reference
    let asset_id = elem.properties.iter().find_map(|p| {
        if p.key == "asset"
            && let ResolvedValue::Identifier(ref id) = p.value
        {
            return Some(id.as_str());
        }
        None
    });

    let asset = asset_id.and_then(|id| {
        for lib in asset_libraries {
            for a in &lib.assets {
                if a.id == id {
                    return Some(a);
                }
            }
        }
        None
    });

    if let Some(asset) = asset {
        // Create a parent node with empty mesh (bounding box outline)
        let bw = asset_number_prop(&asset.properties, "width").unwrap_or(1.0);
        let bd = asset_number_prop(&asset.properties, "depth").unwrap_or(1.0);
        let bh = asset_number_prop(&asset.properties, "height").unwrap_or(1.0);

        // Bounding box for the parent
        let parent_mesh = SceneMesh {
            vertices: box_vertices(
                x as f32,
                (x + bw) as f32,
                y as f32,
                (y + bd) as f32,
                elevation as f32,
                (elevation + bh) as f32,
            ),
            indices: box_indices(),
            color: [0.0, 0.0, 0.0, 0.05], // nearly transparent bounding box
        };

        // Expand parts into child nodes
        let children: Vec<SceneNode> = asset
            .parts
            .iter()
            .filter_map(|part| {
                let (px, py, pz) = part.position;

                // Parts are built in the furniture's local frame, then turned about
                // its origin and moved to `position`.
                let part_x = px as f32;
                let part_y = py as f32;
                let part_z = elevation as f32 + pz as f32;

                let mesh = match &part.geometry {
                    Some(model::GeometryPrimitive::Box(w, d, h)) => SceneMesh {
                        vertices: box_vertices(
                            part_x,
                            part_x + *w as f32,
                            part_y,
                            part_y + *d as f32,
                            part_z,
                            part_z + *h as f32,
                        ),
                        indices: box_indices(),
                        color: element_color(ElementType::Furniture),
                    },
                    Some(model::GeometryPrimitive::Cylinder(r, h)) => {
                        let (verts, idxs) = cylinder_mesh(
                            part_x as f64 + *r,
                            part_y as f64 + *r,
                            *r,
                            part_z,
                            part_z + *h as f32,
                            16,
                        );
                        SceneMesh {
                            vertices: verts,
                            indices: idxs,
                            color: element_color(ElementType::Furniture),
                        }
                    }
                    Some(model::GeometryPrimitive::Extrude(pts, h)) => {
                        // Offset polygon points by part position
                        let offset_pts: Vec<(f64, f64)> = pts
                            .iter()
                            .map(|(px2, py2)| (part_x as f64 + px2, part_y as f64 + py2))
                            .collect();
                        let (verts, idxs) =
                            extrude_polygon(&offset_pts, part_z, part_z + *h as f32);
                        SceneMesh {
                            vertices: verts,
                            indices: idxs,
                            color: element_color(ElementType::Furniture),
                        }
                    }
                    Some(model::GeometryPrimitive::Import(_)) | None => {
                        return None;
                    }
                };

                let (sin_r, cos_r) = rotation_rad.sin_cos();
                let turn = |v: [f32; 3]| {
                    [
                        v[0] * cos_r - v[1] * sin_r,
                        v[0] * sin_r + v[1] * cos_r,
                        v[2],
                    ]
                };
                let mut mesh = mesh;
                for v in &mut mesh.vertices {
                    let p = turn(v.position);
                    v.position = [p[0] + x as f32, p[1] + y as f32, p[2]];
                    v.normal = turn(v.normal);
                }

                Some(SceneNode {
                    id: format!("{}_{}", elem.id, part.name),
                    element_type: "FurniturePart".to_string(),
                    label: format!("Part: {}", part.name),
                    mesh,
                    children: vec![],
                    properties: vec![],
                    material: None,
                })
            })
            .collect();

        Some(SceneNode {
            id: elem.id.clone(),
            element_type: "Furniture".to_string(),
            label: format!("Furniture: {} ({})", elem.id, asset.id),
            mesh: parent_mesh,
            children,
            properties: format_properties(&elem.properties),
            material: None,
        })
    } else {
        // No asset reference: use simple bounding box (v0.1 behavior)
        let mesh = furniture_mesh(elem, elevation)?;
        Some(SceneNode {
            id: elem.id.clone(),
            element_type: "Furniture".to_string(),
            label: format!("Furniture: {}", elem.id),
            mesh,
            children: vec![],
            properties: format_properties(&elem.properties),
            material: None,
        })
    }
}

fn asset_number_prop(props: &[ResolvedProperty], key: &str) -> Option<f64> {
    props.iter().find_map(|p| {
        if p.key == key
            && let ResolvedValue::Number(n) = &p.value
        {
            Some(*n)
        } else {
            None
        }
    })
}

fn elevator_mesh(elem: &ResolvedElement, elevation: f64, storey_height: f64) -> Option<SceneMesh> {
    let (x, y) = get_point(&elem.properties, "position")?;
    let shaft_width = get_number(&elem.properties, "shaft_width").unwrap_or(1.8);
    let shaft_depth = get_number(&elem.properties, "shaft_depth").unwrap_or(2.0);
    // Elevator shaft spans from current elevation to the top of the storey
    let height = storey_height;

    let verts = box_vertices(
        x as f32,
        (x + shaft_width) as f32,
        y as f32,
        (y + shaft_depth) as f32,
        elevation as f32,
        (elevation + height) as f32,
    );

    Some(SceneMesh {
        vertices: verts,
        indices: box_indices(),
        color: element_color(ElementType::Elevator),
    })
}

fn window_mesh(
    child: &ResolvedElement,
    parent: &ResolvedElement,
    elevation: f64,
) -> Option<SceneMesh> {
    let frame = filler_frame(child, parent)?;
    let wall_thickness = get_number(&parent.properties, "thickness").unwrap_or(0.2);

    let offset = get_number(&child.properties, "offset").unwrap_or(0.0);
    let width = get_number(&child.properties, "width").unwrap_or(1.0);
    let height = get_number(&child.properties, "height").unwrap_or(1.0);
    let sill = get_number(&child.properties, "sill_height").unwrap_or(0.9);

    let half_t = wall_thickness / 2.0;
    let depth = get_number(&child.properties, "depth").unwrap_or(0.05); // thin glass pane
    let verts = box_vertices(
        offset as f32,
        (offset + width) as f32,
        -(depth / 2.0) as f32,
        (depth / 2.0) as f32,
        sill as f32,
        (sill + height) as f32,
    );
    let indices = box_indices();

    let vertices = place(verts, frame, elevation);

    let _ = half_t; // wall_thickness used for context

    Some(SceneMesh {
        vertices,
        indices,
        color: [0.5, 0.7, 0.9, 0.5], // translucent blue for glass
    })
}

fn door_mesh(
    child: &ResolvedElement,
    parent: &ResolvedElement,
    elevation: f64,
) -> Option<SceneMesh> {
    let frame = filler_frame(child, parent)?;

    let offset = get_number(&child.properties, "offset").unwrap_or(0.0);
    let width = get_number(&child.properties, "width").unwrap_or(0.9);
    let height = get_number(&child.properties, "height").unwrap_or(2.1);

    let depth = get_number(&child.properties, "depth").unwrap_or(0.05);
    let verts = box_vertices(
        offset as f32,
        (offset + width) as f32,
        -(depth / 2.0) as f32,
        (depth / 2.0) as f32,
        0.0,
        height as f32,
    );
    let indices = box_indices();

    let vertices = place(verts, frame, elevation);

    Some(SceneMesh {
        vertices,
        indices,
        color: [0.55, 0.35, 0.15, 1.0], // brown for doors
    })
}

// ---------------------------------------------------------------------------
// Door / window detail geometry (frames, mullions)
// ---------------------------------------------------------------------------

/// Wall-local frame of a door or window on `parent`: the origin and angle that
/// map local x (distance along the wall) to plan coordinates, taken at the
/// filler's centre so it sits on the tangent of a curved wall.
fn filler_frame(child: &ResolvedElement, parent: &ResolvedElement) -> Option<((f64, f64), f64)> {
    Some(filler_axis_frame(
        child,
        &WallAxis::of(parent, default_curve_tolerance())?,
    ))
}

/// Rotate wall-local vertices by the frame angle, move them to its origin and
/// lift them to `elevation`.
fn place(verts: Vec<Vertex>, ((ox, oy), angle): ((f64, f64), f64), elevation: f64) -> Vec<Vertex> {
    let (sin_a, cos_a) = (angle.sin() as f32, angle.cos() as f32);
    let (ox, oy, oz) = (ox as f32, oy as f32, elevation as f32);
    verts
        .into_iter()
        .map(|v| Vertex {
            position: [
                v.position[0] * cos_a - v.position[1] * sin_a + ox,
                v.position[0] * sin_a + v.position[1] * cos_a + oy,
                v.position[2] + oz,
            ],
            normal: [
                v.normal[0] * cos_a - v.normal[1] * sin_a,
                v.normal[0] * sin_a + v.normal[1] * cos_a,
                v.normal[2],
            ],
        })
        .collect()
}

/// Create a box mesh in wall-local coordinates, placed in the filler `frame`.
#[allow(clippy::too_many_arguments)]
fn rotated_box_mesh(
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    z0: f32,
    z1: f32,
    frame: ((f64, f64), f64),
    elevation: f64,
    color: [f32; 4],
) -> Option<SceneMesh> {
    Some(SceneMesh {
        vertices: place(box_vertices(x0, x1, y0, y1, z0, z1), frame, elevation),
        indices: box_indices(),
        color,
    })
}

/// Generate mullion cross-bar children for a window (horizontal + vertical bars).
fn window_detail_nodes(
    child: &ResolvedElement,
    parent: &ResolvedElement,
    elevation: f64,
) -> Vec<SceneNode> {
    let Some(frame) = filler_frame(child, parent) else {
        return Vec::new();
    };
    let offset = get_number(&child.properties, "offset").unwrap_or(0.0) as f32;
    let width = get_number(&child.properties, "width").unwrap_or(1.0) as f32;
    let height = get_number(&child.properties, "height").unwrap_or(1.0) as f32;
    let sill = get_number(&child.properties, "sill_height").unwrap_or(0.9) as f32;

    let bar = 0.03_f32; // mullion bar cross-section
    let bar_d = 0.06_f32; // bar depth (thicker than glass pane so it's visible)
    let frame_color: [f32; 4] = [0.9, 0.9, 0.9, 1.0]; // white frame

    let mut nodes = Vec::new();

    // Horizontal mullion at mid-height
    if let Some(mesh) = rotated_box_mesh(
        offset,
        offset + width,
        -bar_d / 2.0,
        bar_d / 2.0,
        sill + height / 2.0 - bar / 2.0,
        sill + height / 2.0 + bar / 2.0,
        frame,
        elevation,
        frame_color,
    ) {
        nodes.push(SceneNode {
            id: format!("{}-hmullion", child.id),
            element_type: "WindowFrame".to_string(),
            label: "Window mullion".to_string(),
            mesh,
            children: Vec::new(),
            properties: vec![],
            material: None,
        });
    }

    // Vertical mullion at mid-width
    if let Some(mesh) = rotated_box_mesh(
        offset + width / 2.0 - bar / 2.0,
        offset + width / 2.0 + bar / 2.0,
        -bar_d / 2.0,
        bar_d / 2.0,
        sill,
        sill + height,
        frame,
        elevation,
        frame_color,
    ) {
        nodes.push(SceneNode {
            id: format!("{}-vmullion", child.id),
            element_type: "WindowFrame".to_string(),
            label: "Window mullion".to_string(),
            mesh,
            children: Vec::new(),
            properties: vec![],
            material: None,
        });
    }

    nodes
}

/// Generate door frame children (left, right, top bars).
fn door_detail_nodes(
    child: &ResolvedElement,
    parent: &ResolvedElement,
    elevation: f64,
) -> Vec<SceneNode> {
    let Some(frame) = filler_frame(child, parent) else {
        return Vec::new();
    };
    let offset = get_number(&child.properties, "offset").unwrap_or(0.0) as f32;
    let width = get_number(&child.properties, "width").unwrap_or(0.9) as f32;
    let height = get_number(&child.properties, "height").unwrap_or(2.1) as f32;

    let frame_w = 0.06_f32; // frame bar width
    let frame_d = 0.10_f32; // frame depth (thicker than door panel)
    let frame_color: [f32; 4] = [0.25, 0.15, 0.05, 1.0]; // dark brown frame

    let mut nodes = Vec::new();

    // Left frame bar
    if let Some(mesh) = rotated_box_mesh(
        offset - frame_w,
        offset,
        -frame_d / 2.0,
        frame_d / 2.0,
        0.0,
        height,
        frame,
        elevation,
        frame_color,
    ) {
        nodes.push(SceneNode {
            id: format!("{}-lframe", child.id),
            element_type: "DoorFrame".to_string(),
            label: "Door frame".to_string(),
            mesh,
            children: Vec::new(),
            properties: vec![],
            material: None,
        });
    }

    // Right frame bar
    if let Some(mesh) = rotated_box_mesh(
        offset + width,
        offset + width + frame_w,
        -frame_d / 2.0,
        frame_d / 2.0,
        0.0,
        height,
        frame,
        elevation,
        frame_color,
    ) {
        nodes.push(SceneNode {
            id: format!("{}-rframe", child.id),
            element_type: "DoorFrame".to_string(),
            label: "Door frame".to_string(),
            mesh,
            children: Vec::new(),
            properties: vec![],
            material: None,
        });
    }

    // Top frame bar (lintel)
    if let Some(mesh) = rotated_box_mesh(
        offset - frame_w,
        offset + width + frame_w,
        -frame_d / 2.0,
        frame_d / 2.0,
        height,
        height + frame_w,
        frame,
        elevation,
        frame_color,
    ) {
        nodes.push(SceneNode {
            id: format!("{}-tframe", child.id),
            element_type: "DoorFrame".to_string(),
            label: "Door frame".to_string(),
            mesh,
            children: Vec::new(),
            properties: vec![],
            material: None,
        });
    }

    nodes
}

// ---------------------------------------------------------------------------
// Geometry primitives
// ---------------------------------------------------------------------------

/// Generate vertices for an axis-aligned box with correct face normals.
fn box_vertices(x0: f32, x1: f32, y0: f32, y1: f32, z0: f32, z1: f32) -> Vec<Vertex> {
    // 6 faces × 4 vertices = 24 vertices
    vec![
        // Front face (y = y1, normal +Y)
        Vertex {
            position: [x0, y1, z0],
            normal: [0.0, 1.0, 0.0],
        },
        Vertex {
            position: [x1, y1, z0],
            normal: [0.0, 1.0, 0.0],
        },
        Vertex {
            position: [x1, y1, z1],
            normal: [0.0, 1.0, 0.0],
        },
        Vertex {
            position: [x0, y1, z1],
            normal: [0.0, 1.0, 0.0],
        },
        // Back face (y = y0, normal -Y)
        Vertex {
            position: [x1, y0, z0],
            normal: [0.0, -1.0, 0.0],
        },
        Vertex {
            position: [x0, y0, z0],
            normal: [0.0, -1.0, 0.0],
        },
        Vertex {
            position: [x0, y0, z1],
            normal: [0.0, -1.0, 0.0],
        },
        Vertex {
            position: [x1, y0, z1],
            normal: [0.0, -1.0, 0.0],
        },
        // Left face (x = x0, normal -X)
        Vertex {
            position: [x0, y0, z0],
            normal: [-1.0, 0.0, 0.0],
        },
        Vertex {
            position: [x0, y1, z0],
            normal: [-1.0, 0.0, 0.0],
        },
        Vertex {
            position: [x0, y1, z1],
            normal: [-1.0, 0.0, 0.0],
        },
        Vertex {
            position: [x0, y0, z1],
            normal: [-1.0, 0.0, 0.0],
        },
        // Right face (x = x1, normal +X)
        Vertex {
            position: [x1, y1, z0],
            normal: [1.0, 0.0, 0.0],
        },
        Vertex {
            position: [x1, y0, z0],
            normal: [1.0, 0.0, 0.0],
        },
        Vertex {
            position: [x1, y0, z1],
            normal: [1.0, 0.0, 0.0],
        },
        Vertex {
            position: [x1, y1, z1],
            normal: [1.0, 0.0, 0.0],
        },
        // Top face (z = z1, normal +Z)
        Vertex {
            position: [x0, y0, z1],
            normal: [0.0, 0.0, 1.0],
        },
        Vertex {
            position: [x0, y1, z1],
            normal: [0.0, 0.0, 1.0],
        },
        Vertex {
            position: [x1, y1, z1],
            normal: [0.0, 0.0, 1.0],
        },
        Vertex {
            position: [x1, y0, z1],
            normal: [0.0, 0.0, 1.0],
        },
        // Bottom face (z = z0, normal -Z)
        Vertex {
            position: [x0, y1, z0],
            normal: [0.0, 0.0, -1.0],
        },
        Vertex {
            position: [x0, y0, z0],
            normal: [0.0, 0.0, -1.0],
        },
        Vertex {
            position: [x1, y0, z0],
            normal: [0.0, 0.0, -1.0],
        },
        Vertex {
            position: [x1, y1, z0],
            normal: [0.0, 0.0, -1.0],
        },
    ]
}

/// Triangle indices for a box (6 faces × 2 triangles × 3 indices = 36 indices).
fn box_indices() -> Vec<u32> {
    let mut indices = Vec::with_capacity(36);
    for face in 0..6u32 {
        let base = face * 4;
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    indices
}

/// Extrude a 2D polygon between z_bottom and z_top.
/// Returns (vertices, indices) for the extruded solid.
fn extrude_polygon(points: &[(f64, f64)], z_bottom: f32, z_top: f32) -> (Vec<Vertex>, Vec<u32>) {
    let n = points.len();
    if n < 3 {
        return (Vec::new(), Vec::new());
    }

    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // Top and bottom faces — ear clipping (correct for concave polygons)
    let tris = ear_clip_triangulate(points);

    let top_base = vertices.len() as u32;
    for &(x, y) in points {
        vertices.push(Vertex {
            position: [x as f32, y as f32, z_top],
            normal: [0.0, 0.0, 1.0],
        });
    }
    for tri in &tris {
        indices.extend_from_slice(&[
            top_base + tri[0] as u32,
            top_base + tri[1] as u32,
            top_base + tri[2] as u32,
        ]);
    }

    let bot_base = vertices.len() as u32;
    for &(x, y) in points {
        vertices.push(Vertex {
            position: [x as f32, y as f32, z_bottom],
            normal: [0.0, 0.0, -1.0],
        });
    }
    for tri in &tris {
        // Reversed winding for downward-facing face
        indices.extend_from_slice(&[
            bot_base + tri[0] as u32,
            bot_base + tri[2] as u32,
            bot_base + tri[1] as u32,
        ]);
    }

    // Side faces
    for i in 0..n {
        let j = (i + 1) % n;
        let (x0, y0) = (points[i].0 as f32, points[i].1 as f32);
        let (x1, y1) = (points[j].0 as f32, points[j].1 as f32);

        // Compute outward normal for this edge (2D perpendicular, right-hand)
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).sqrt();
        let (nx, ny) = if len > 1e-9 {
            (dy / len, -dx / len)
        } else {
            (0.0, 1.0)
        };
        let normal = [nx, ny, 0.0_f32];

        let side_base = vertices.len() as u32;
        vertices.push(Vertex {
            position: [x0, y0, z_bottom],
            normal,
        });
        vertices.push(Vertex {
            position: [x1, y1, z_bottom],
            normal,
        });
        vertices.push(Vertex {
            position: [x1, y1, z_top],
            normal,
        });
        vertices.push(Vertex {
            position: [x0, y0, z_top],
            normal,
        });

        indices.extend_from_slice(&[
            side_base,
            side_base + 1,
            side_base + 2,
            side_base,
            side_base + 2,
            side_base + 3,
        ]);
    }

    (vertices, indices)
}

/// Generate a tessellated cylinder mesh.
/// Creates a cylinder approximated by `segments` sides, centered at (cx, cy).
pub(crate) fn cylinder_mesh(
    cx: f64,
    cy: f64,
    radius: f64,
    z_bottom: f32,
    z_top: f32,
    segments: u32,
) -> (Vec<Vertex>, Vec<u32>) {
    let points: Vec<(f64, f64)> = (0..segments)
        .map(|i| {
            let angle = 2.0 * std::f64::consts::PI * i as f64 / segments as f64;
            (cx + radius * angle.cos(), cy + radius * angle.sin())
        })
        .collect();
    extrude_polygon(&points, z_bottom, z_top)
}

/// Extrude a polygon with interior holes between z_bottom and z_top.
/// The outer boundary is extruded, and hole boundaries are cut from top/bottom faces.
pub(crate) fn extrude_polygon_with_holes(
    outer: &[(f64, f64)],
    holes: &[Vec<(f64, f64)>],
    z_bottom: f32,
    z_top: f32,
) -> (Vec<Vertex>, Vec<u32>) {
    let n_outer = outer.len();
    if n_outer < 3 {
        return (Vec::new(), Vec::new());
    }

    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // Top and bottom faces share one triangulation.
    let triangles = triangulate_polygon_with_holes(outer, holes);

    // Collect all unique points used in triangulation
    let mut all_points: Vec<(f64, f64)> = outer.to_vec();
    for hole in holes {
        all_points.extend(hole);
    }

    // Top face
    let top_normal = [0.0_f32, 0.0, 1.0];
    let top_base = vertices.len() as u32;
    for &(x, y) in &all_points {
        vertices.push(Vertex {
            position: [x as f32, y as f32, z_top],
            normal: top_normal,
        });
    }
    for tri in &triangles {
        indices.extend_from_slice(&[
            top_base + tri[0] as u32,
            top_base + tri[1] as u32,
            top_base + tri[2] as u32,
        ]);
    }

    // Bottom face (reversed winding)
    let bot_normal = [0.0_f32, 0.0, -1.0];
    let bot_base = vertices.len() as u32;
    for &(x, y) in &all_points {
        vertices.push(Vertex {
            position: [x as f32, y as f32, z_bottom],
            normal: bot_normal,
        });
    }
    for tri in &triangles {
        indices.extend_from_slice(&[
            bot_base + tri[0] as u32,
            bot_base + tri[2] as u32,
            bot_base + tri[1] as u32,
        ]);
    }

    // Side faces — outer boundary
    add_side_faces(outer, z_bottom, z_top, &mut vertices, &mut indices);

    // Side faces — hole boundaries (reversed normals so they point inward)
    for hole in holes {
        add_side_faces_reversed(hole, z_bottom, z_top, &mut vertices, &mut indices);
    }

    (vertices, indices)
}

/// Add side faces for a polygon boundary (outward-facing normals).
fn add_side_faces(
    points: &[(f64, f64)],
    z_bottom: f32,
    z_top: f32,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let n = points.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let (x0, y0) = (points[i].0 as f32, points[i].1 as f32);
        let (x1, y1) = (points[j].0 as f32, points[j].1 as f32);

        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).sqrt();
        let (nx, ny) = if len > 1e-9 {
            (dy / len, -dx / len)
        } else {
            (0.0, 1.0)
        };
        let normal = [nx, ny, 0.0_f32];

        let base = vertices.len() as u32;
        vertices.push(Vertex {
            position: [x0, y0, z_bottom],
            normal,
        });
        vertices.push(Vertex {
            position: [x1, y1, z_bottom],
            normal,
        });
        vertices.push(Vertex {
            position: [x1, y1, z_top],
            normal,
        });
        vertices.push(Vertex {
            position: [x0, y0, z_top],
            normal,
        });

        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Add side faces for a hole boundary (inward-facing normals).
fn add_side_faces_reversed(
    points: &[(f64, f64)],
    z_bottom: f32,
    z_top: f32,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    let n = points.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let (x0, y0) = (points[i].0 as f32, points[i].1 as f32);
        let (x1, y1) = (points[j].0 as f32, points[j].1 as f32);

        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).sqrt();
        let (nx, ny) = if len > 1e-9 {
            (-dy / len, dx / len) // reversed normal (inward)
        } else {
            (0.0, -1.0)
        };
        let normal = [nx, ny, 0.0_f32];

        let base = vertices.len() as u32;
        vertices.push(Vertex {
            position: [x0, y0, z_bottom],
            normal,
        });
        vertices.push(Vertex {
            position: [x1, y1, z_bottom],
            normal,
        });
        vertices.push(Vertex {
            position: [x1, y1, z_top],
            normal,
        });
        vertices.push(Vertex {
            position: [x0, y0, z_top],
            normal,
        });

        // Reversed winding
        indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    }
}

/// Triangulate a (possibly concave) polygon with holes (earcut). Indices run
/// over `outer` followed by each hole in order. Holes may have either winding.
pub(super) fn triangulate_polygon_with_holes(
    outer: &[(f64, f64)],
    holes: &[Vec<(f64, f64)>],
) -> Vec<[usize; 3]> {
    let mut flat: Vec<f64> = outer.iter().flat_map(|&(x, y)| [x, y]).collect();
    let mut hole_starts = Vec::new();
    for hole in holes {
        hole_starts.push(flat.len() / 2);
        flat.extend(hole.iter().flat_map(|&(x, y)| [x, y]));
    }
    let indices = earcutr::earcut(&flat, &hole_starts, 2).unwrap_or_default();
    indices.as_chunks::<3>().0.to_vec()
}

/// Default colors for each element type (RGBA).
pub(crate) fn element_color(et: ElementType) -> [f32; 4] {
    crate::semantic::registry::get_descriptor(et).scene_color
}

// ---------------------------------------------------------------------------
// Property helpers
// ---------------------------------------------------------------------------

/// Format element properties as human-readable key-value pairs for UI display.
fn format_properties(properties: &[ResolvedProperty]) -> Vec<(String, String)> {
    properties
        .iter()
        .filter_map(|p| {
            let val = match &p.value {
                ResolvedValue::Number(n) => Some(format_prop_number(*n)),
                ResolvedValue::String(s) => Some(s.clone()),
                ResolvedValue::Boolean(b) => Some(b.to_string()),
                ResolvedValue::Identifier(s) => Some(s.clone()),
                ResolvedValue::Point(x, y) => Some(format!(
                    "({}, {})",
                    format_prop_number(*x),
                    format_prop_number(*y)
                )),
                ResolvedValue::Polygon(pts) => Some(format!("{} vertices", pts.len())),
                ResolvedValue::Path3D(pts) => Some(format!("{} points", pts.len())),
                ResolvedValue::PolygonWithHoles { outer, holes } => {
                    Some(format!("{} vertices, {} holes", outer.len(), holes.len()))
                }
                ResolvedValue::Profile {
                    profile_type,
                    params,
                } => {
                    let ps: Vec<String> = params.iter().map(|n| format_prop_number(*n)).collect();
                    Some(format!("{}({})", profile_type, ps.join(", ")))
                }
                ResolvedValue::Axis { from, to, via, .. } => {
                    let base = format!(
                        "({}, {}) to ({}, {})",
                        format_prop_number(from.0),
                        format_prop_number(from.1),
                        format_prop_number(to.0),
                        format_prop_number(to.1),
                    );
                    match via {
                        Some(v) => Some(format!(
                            "{} via ({}, {})",
                            base,
                            format_prop_number(v.0),
                            format_prop_number(v.1),
                        )),
                        None => Some(base),
                    }
                }
                ResolvedValue::EnumList(vals) => Some(vals.join(", ")),
                ResolvedValue::NumberList(vals) => Some(
                    vals.iter()
                        .map(|v| format_prop_number(*v))
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
                ResolvedValue::List(_) => None,
                ResolvedValue::Point3D(x, y, z) => Some(format!(
                    "({}, {}, {})",
                    format_prop_number(*x),
                    format_prop_number(*y),
                    format_prop_number(*z),
                )),
                ResolvedValue::GeometryPrimitive(_) => None, // skip complex geometry
            };
            val.map(|v| (p.key.clone(), v))
        })
        .collect()
}

fn format_prop_number(n: f64) -> String {
    if (n - n.round()).abs() < 1e-9 {
        format!("{}", n as i64)
    } else {
        format!("{:.3}", n)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn get_identifier<'a>(properties: &'a [ResolvedProperty], key: &str) -> Option<&'a str> {
    properties
        .iter()
        .find(|p| p.key == key)
        .and_then(|p| match &p.value {
            ResolvedValue::Identifier(s) => Some(s.as_str()),
            _ => None,
        })
}

pub(crate) fn get_axis(properties: &[ResolvedProperty]) -> Option<((f64, f64), (f64, f64))> {
    properties
        .iter()
        .find(|p| p.key == "from")
        .and_then(|p| match &p.value {
            ResolvedValue::Axis { from, to, .. } => Some((*from, *to)),
            _ => None,
        })
}

/// Holes of a slab or roof: those of a `{ outer, holes }` boundary, else
/// those of a separate `holes:` property.
pub(crate) fn outline_holes(properties: &[ResolvedProperty]) -> Vec<Vec<(f64, f64)>> {
    let holes = get_polygon_with_holes(properties, "boundary");
    if holes.is_empty() {
        get_polygon_with_holes(properties, "holes")
    } else {
        holes
    }
}

pub(crate) fn get_polygon_with_holes(
    properties: &[ResolvedProperty],
    key: &str,
) -> Vec<Vec<(f64, f64)>> {
    properties
        .iter()
        .find(|p| p.key == key)
        .and_then(|p| match &p.value {
            ResolvedValue::PolygonWithHoles { holes, .. } => Some(holes.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

pub(crate) fn get_polygon(properties: &[ResolvedProperty], key: &str) -> Option<Vec<(f64, f64)>> {
    properties
        .iter()
        .find(|p| p.key == key)
        .and_then(|p| match &p.value {
            ResolvedValue::Polygon(pts) => Some(pts.clone()),
            ResolvedValue::PolygonWithHoles { outer, .. } => Some(outer.clone()),
            _ => None,
        })
}

/// Plan outline of a wall along its axis (`WallAxis`).
pub(crate) fn wall_outline(elem: &ResolvedElement) -> Option<Vec<(f64, f64)>> {
    let axis = WallAxis::of(elem, default_curve_tolerance())?;
    let thickness = get_number(&elem.properties, "thickness").unwrap_or(0.2);
    Some(geometry::wall_outline(&axis, thickness))
}

pub(crate) fn get_point(properties: &[ResolvedProperty], key: &str) -> Option<(f64, f64)> {
    properties
        .iter()
        .find(|p| p.key == key)
        .and_then(|p| match &p.value {
            ResolvedValue::Point(x, y) => Some((*x, *y)),
            _ => None,
        })
}

pub(crate) fn get_profile(
    properties: &[ResolvedProperty],
    key: &str,
) -> Option<(String, Vec<f64>)> {
    properties
        .iter()
        .find(|p| p.key == key)
        .and_then(|p| match &p.value {
            ResolvedValue::Profile {
                profile_type,
                params,
            } => Some((profile_type.clone(), params.clone())),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cst_to_ast;
    use crate::semantic;

    #[test]
    fn test_scene_wall() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.24 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        assert_eq!(scene.nodes.len(), 1);
        assert_eq!(scene.nodes[0].id, "W1");
        assert_eq!(scene.nodes[0].element_type, "Wall");
        assert!(!scene.nodes[0].mesh.vertices.is_empty());
        assert!(!scene.nodes[0].mesh.indices.is_empty());
        // 24 vertices (6 faces × 4) for a box
        assert_eq!(scene.nodes[0].mesh.vertices.len(), 24);
        // 36 indices (6 faces × 2 tris × 3)
        assert_eq!(scene.nodes[0].mesh.indices.len(), 36);
    }

    #[test]
    fn test_scene_layered_material() {
        let source = std::fs::read_to_string("../../tests/fixtures/layered_materials.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        let wall = scene.nodes.iter().find(|n| n.id == "W1").unwrap();
        let rows: Vec<_> = wall
            .properties
            .iter()
            .filter(|(k, _)| k.starts_with("Material Layers"))
            .map(|(k, v)| format!("{k} = {v}"))
            .collect();
        assert_eq!(
            rows,
            [
                "Material Layers · 1. mineral_wool = 0.14 m",
                "Material Layers · 2. concrete = 0.15 m",
                "Material Layers · 3. plaster = 0.01 m",
            ]
        );
        // The wall shows its first layer; the column's concrete has its own colour.
        let column = scene.nodes.iter().find(|n| n.id == "C1").unwrap();
        assert_eq!(column.mesh.color[0], 0xb3 as f32 / 255.0);
        assert_eq!(
            wall.material
                .map(|i| scene.materials[i as usize].name.as_str()),
            Some("mineral_wool")
        );
    }

    #[test]
    fn test_scene_slab() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Slab: S1
    boundary: [(0,0), (6,0), (6,5), (0,5)]
    thickness: 0.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        assert_eq!(scene.nodes.len(), 1);
        assert_eq!(scene.nodes[0].id, "S1");
        assert!(!scene.nodes[0].mesh.vertices.is_empty());
    }

    #[test]
    fn test_scene_column_beam() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Column: COL1
    position: (3, 2)
    height: 3.0 m
    profile: rectangle(0.3, 0.3)
  Beam: B1
    from: (0, 2) to: (6, 2)
    profile: rectangle(0.3, 0.5)
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        assert_eq!(scene.nodes.len(), 2);
        let types: Vec<&str> = scene
            .nodes
            .iter()
            .map(|n| n.element_type.as_str())
            .collect();
        assert!(types.contains(&"Column"));
        assert!(types.contains(&"Beam"));
    }

    #[test]
    fn test_scene_wall_with_window() {
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
        let scene = generate_scene(&model);

        assert_eq!(scene.nodes.len(), 1);
        assert_eq!(scene.nodes[0].id, "W1");
        // Window should be a child node
        assert_eq!(scene.nodes[0].children.len(), 1);
        assert_eq!(scene.nodes[0].children[0].id, "WIN1");
        assert_eq!(scene.nodes[0].children[0].element_type, "Window");
        // Wall should be segmented (more than single box of 24 vertices)
        // 3 segments: left wall (0-1.5), below+above window (1.5-2.7), right wall (2.7-6)
        assert!(
            scene.nodes[0].mesh.vertices.len() > 24,
            "wall with window should have segmented geometry, got {} vertices",
            scene.nodes[0].mesh.vertices.len()
        );
    }

    #[test]
    fn test_scene_wall_with_door_opening() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.2 m
    Door: D1
      offset: 2.0 m
      width: 0.9 m
      height: 2.1 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        assert_eq!(scene.nodes[0].id, "W1");
        let wall = &scene.nodes[0];
        // Door child node present
        assert_eq!(wall.children.len(), 1);
        assert_eq!(wall.children[0].element_type, "Door");
        // Wall is segmented: left (0-2), above-door (2-2.9), right (2.9-6) = 3 segments = 72 vertices
        assert!(
            wall.mesh.vertices.len() > 24,
            "wall with door should be segmented, got {} vertices",
            wall.mesh.vertices.len()
        );
        // Exactly 3 segments: left full-height + above-door + right full-height
        assert_eq!(wall.mesh.vertices.len(), 3 * 24);
    }

    #[test]
    fn test_scene_wall_with_multiple_openings() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (8, 0)
    height: 2.8 m
    thickness: 0.2 m
    Door: D1
      offset: 1.0 m
      width: 0.9 m
      height: 2.1 m
    Window: WIN1
      offset: 4.0 m
      width: 1.2 m
      height: 1.4 m
      sill_height: 0.9 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        let wall = &scene.nodes[0];
        assert_eq!(wall.id, "W1");
        assert_eq!(wall.children.len(), 2);
        // Segments: left(0-1) + above-door(1-1.9) + middle(1.9-4) + below-window(4-5.2) + above-window(4-5.2) + right(5.2-8)
        // = 6 segments = 144 vertices
        assert_eq!(wall.mesh.vertices.len(), 6 * 24);
    }

    #[test]
    fn test_scene_wall_no_children_unchanged() {
        // Wall without children should be a single box (fast path)
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        assert_eq!(scene.nodes[0].mesh.vertices.len(), 24);
        assert_eq!(scene.nodes[0].mesh.indices.len(), 36);
    }

    #[test]
    fn test_scene_all_elements() {
        let source = std::fs::read_to_string("../../tests/fixtures/all_elements.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        // Should have multiple nodes (walls, slab, column, beam, stair, railing, ramp, furniture, roof)
        assert!(
            scene.nodes.len() >= 9,
            "expected at least 9 nodes, got {}",
            scene.nodes.len()
        );

        let types: Vec<&str> = scene
            .nodes
            .iter()
            .map(|n| n.element_type.as_str())
            .collect();
        assert!(types.contains(&"Wall"), "missing Wall");
        assert!(types.contains(&"Slab"), "missing Slab");
        assert!(types.contains(&"Column"), "missing Column");
        assert!(types.contains(&"Beam"), "missing Beam");
        assert!(types.contains(&"Stair"), "missing Stair");
        assert!(types.contains(&"Railing"), "missing Railing");
        assert!(types.contains(&"Ramp"), "missing Ramp");
        assert!(types.contains(&"Furniture"), "missing Furniture");
        assert!(types.contains(&"Roof"), "missing Roof");
    }

    #[test]
    fn test_scene_two_storey_house() {
        let source = std::fs::read_to_string("../../tests/fixtures/two_storey_house.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        // Should have many nodes (walls, slabs, beams, columns, stair, railing, roof)
        assert!(
            scene.nodes.len() >= 30,
            "expected at least 30 nodes for two-storey house, got {}",
            scene.nodes.len()
        );

        let types: Vec<&str> = scene
            .nodes
            .iter()
            .map(|n| n.element_type.as_str())
            .collect();
        assert!(types.contains(&"Wall"), "missing Wall");
        assert!(types.contains(&"Slab"), "missing Slab");
        assert!(types.contains(&"Column"), "missing Column");
        assert!(types.contains(&"Beam"), "missing Beam");
        assert!(types.contains(&"Stair"), "missing Stair");
        assert!(types.contains(&"Railing"), "missing Railing");
        assert!(types.contains(&"Roof"), "missing Roof");

        // All nodes should have valid mesh data
        for node in &scene.nodes {
            assert!(
                !node.mesh.vertices.is_empty(),
                "node {} ({}) has no vertices",
                node.id,
                node.element_type
            );
            assert!(
                !node.mesh.indices.is_empty(),
                "node {} ({}) has no indices",
                node.id,
                node.element_type
            );
        }

        // Verify scene is serializable
        let json = serde_json::to_string(&scene).unwrap();
        assert!(!json.is_empty());
    }

    #[test]
    fn test_scene_serialization() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (4, 0)
    height: 2.8 m
    thickness: 0.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        let json = serde_json::to_string(&scene).unwrap();
        assert!(json.contains("\"id\":\"W1\""));
        assert!(json.contains("\"vertices\""));
        assert!(json.contains("\"indices\""));

        // Verify round-trip
        let deserialized: SceneGraph = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.nodes.len(), scene.nodes.len());
    }

    #[test]
    fn test_box_vertices_count() {
        let verts = box_vertices(0.0, 1.0, 0.0, 1.0, 0.0, 1.0);
        assert_eq!(verts.len(), 24);
        let indices = box_indices();
        assert_eq!(indices.len(), 36);
    }

    #[test]
    fn test_extrude_polygon() {
        let points = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        let (verts, indices) = extrude_polygon(&points, 0.0, 1.0);
        // Top: 4 verts, Bottom: 4 verts, Sides: 4 edges × 4 verts = 16
        assert_eq!(verts.len(), 24);
        // Top: 2 tris, Bottom: 2 tris, Sides: 4 × 2 tris = 8 => 12 tris × 3 = 36
        assert_eq!(indices.len(), 36);
    }

    #[test]
    fn test_elevation_offset() {
        let source = "\
# Site: S
## Building: B
### Storey: FF
  elevation: 3.2 m
  Wall: W1
    from: (0, 0) to: (4, 0)
    height: 2.8 m
    thickness: 0.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        // All Z coordinates should be offset by elevation (3.2)
        let min_z = scene.nodes[0]
            .mesh
            .vertices
            .iter()
            .map(|v| v.position[2])
            .fold(f32::INFINITY, f32::min);
        assert!(
            (min_z - 3.2).abs() < 0.01,
            "min Z should be ~3.2, got {}",
            min_z
        );
    }

    #[test]
    fn test_circular_column_cylinder_mesh() {
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
        let scene = generate_scene(&model);

        assert_eq!(scene.nodes.len(), 1);
        assert_eq!(scene.nodes[0].id, "COL1");
        assert_eq!(scene.nodes[0].element_type, "Column");

        let mesh = &scene.nodes[0].mesh;

        // Cylinder with 16 segments: top (16 verts) + bottom (16 verts) + sides (16 * 4 = 64 verts)
        assert_eq!(
            mesh.vertices.len(),
            96,
            "cylinder should have 96 vertices (16-segment)"
        );

        // Vertices should be roughly circular around (3, 2)
        // Check that min/max X encompasses the cylinder
        let xs: Vec<f32> = mesh.vertices.iter().map(|v| v.position[0]).collect();
        let min_x = xs.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_x = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            (min_x - 2.85).abs() < 0.02,
            "min X should be ~2.85 (center 3 - radius 0.15), got {}",
            min_x
        );
        assert!(
            (max_x - 3.15).abs() < 0.02,
            "max X should be ~3.15 (center 3 + radius 0.15), got {}",
            max_x
        );

        // Check Z range
        let zs: Vec<f32> = mesh.vertices.iter().map(|v| v.position[2]).collect();
        let min_z = zs.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_z = zs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            (min_z - 0.0).abs() < 0.01,
            "min Z should be ~0, got {}",
            min_z
        );
        assert!(
            (max_z - 3.0).abs() < 0.01,
            "max Z should be ~3.0, got {}",
            max_z
        );
    }

    #[test]
    fn test_rectangular_column_box_mesh() {
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
        let scene = generate_scene(&model);

        assert_eq!(scene.nodes.len(), 1);
        let mesh = &scene.nodes[0].mesh;
        // Rectangular column still uses box: 24 vertices, 36 indices
        assert_eq!(mesh.vertices.len(), 24);
        assert_eq!(mesh.indices.len(), 36);
    }

    #[test]
    fn test_window_has_mullion_children() {
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
        let scene = generate_scene(&model);

        let wall = &scene.nodes[0];
        assert_eq!(wall.children.len(), 1);
        let win = &wall.children[0];
        assert_eq!(win.element_type, "Window");
        // Window should have 2 mullion children (horizontal + vertical)
        assert_eq!(
            win.children.len(),
            2,
            "window should have 2 mullion children"
        );
        for mullion in &win.children {
            assert_eq!(mullion.element_type, "WindowFrame");
            assert!(!mullion.mesh.vertices.is_empty());
            assert!(!mullion.mesh.indices.is_empty());
            // Mullion bars should be opaque white
            assert!(
                (mullion.mesh.color[3] - 1.0).abs() < 0.01,
                "mullions should be opaque"
            );
        }
    }

    #[test]
    fn test_door_has_frame_children() {
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
      offset: 3.0 m
      width: 0.9 m
      height: 2.1 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        let wall = &scene.nodes[0];
        assert_eq!(wall.children.len(), 1);
        let door = &wall.children[0];
        assert_eq!(door.element_type, "Door");
        // Door should have 3 frame children (left, right, top)
        assert_eq!(door.children.len(), 3, "door should have 3 frame children");
        for frame in &door.children {
            assert_eq!(frame.element_type, "DoorFrame");
            assert!(!frame.mesh.vertices.is_empty());
            assert!(!frame.mesh.indices.is_empty());
            // Frame bars should be opaque dark brown
            assert!(
                (frame.mesh.color[3] - 1.0).abs() < 0.01,
                "frame should be opaque"
            );
        }
    }

    #[test]
    fn elements_take_the_look_of_their_material() {
        let source = "\
Material: precast
  ifc_name: \"Concrete, Precast\"

# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.24 m
    material: precast
    Window: WI1
      offset: 1.0 m
      width: 1.2 m
      height: 1.2 m
      sill: 0.9 m
    Door: D1
      offset: 4.0 m
      width: 0.9 m
      height: 2.1 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);
        let look = |node: &SceneNode| &scene.materials[node.material.unwrap() as usize];

        let wall = &scene.nodes[0];
        assert_eq!(look(wall).name, "Concrete, Precast");
        assert_eq!(wall.mesh.color, look(wall).color);
        let child = |t: &str| wall.children.iter().find(|c| c.element_type == t).unwrap();
        assert_eq!(look(child("Window")).name, "Glass");
        assert!(child("Window").mesh.color[3] < 1.0);
        assert_eq!(look(&child("Door").children[0]).name, "Oak");
    }

    #[test]
    fn test_door_and_window_visually_distinct() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (10, 0)
    height: 2.8 m
    thickness: 0.24 m
    Window: WIN1
      offset: 1.0 m
      width: 1.2 m
      height: 1.4 m
      sill_height: 0.9 m
    Door: D1
      offset: 5.0 m
      width: 0.9 m
      height: 2.1 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        let wall = &scene.nodes[0];
        assert_eq!(wall.children.len(), 2);

        let win = wall
            .children
            .iter()
            .find(|c| c.element_type == "Window")
            .unwrap();
        let door = wall
            .children
            .iter()
            .find(|c| c.element_type == "Door")
            .unwrap();

        // Colors must be different
        assert_ne!(
            win.mesh.color, door.mesh.color,
            "window and door should have different colors"
        );

        // Window has translucent glass (alpha < 1), door is opaque (alpha = 1)
        assert!(
            win.mesh.color[3] < 1.0,
            "window glass should be translucent"
        );
        assert!(
            (door.mesh.color[3] - 1.0).abs() < 0.01,
            "door panel should be opaque"
        );

        // Window has mullion children, door has frame children
        assert_eq!(
            win.children.len(),
            2,
            "window should have mullion cross-bars"
        );
        assert_eq!(door.children.len(), 3, "door should have frame bars");

        // Child types must differ
        assert_eq!(win.children[0].element_type, "WindowFrame");
        assert_eq!(door.children[0].element_type, "DoorFrame");
    }

    // --- Phase 7c tests ---

    #[test]
    fn test_cylinder_mesh_primitive() {
        let (verts, indices) = cylinder_mesh(0.0, 0.0, 1.0, 0.0, 3.0, 16);
        // 16 segments: top (16) + bottom (16) + sides (16*4) = 96 verts
        assert_eq!(verts.len(), 96);
        // top (14*3) + bottom (14*3) + sides (16*6) = 42 + 42 + 96 = 180
        assert_eq!(indices.len(), 180);

        // Verify all vertices within expected bounds
        for v in &verts {
            let x = v.position[0];
            let y = v.position[1];
            let z = v.position[2];
            assert!(x >= -1.01 && x <= 1.01, "x out of bounds: {}", x);
            assert!(y >= -1.01 && y <= 1.01, "y out of bounds: {}", y);
            assert!(z >= -0.01 && z <= 3.01, "z out of bounds: {}", z);
        }
    }

    #[test]
    fn test_extrude_polygon_with_holes_primitive() {
        let outer = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let holes = vec![vec![(3.0, 3.0), (7.0, 3.0), (7.0, 7.0), (3.0, 7.0)]];
        let (verts, indices) = extrude_polygon_with_holes(&outer, &holes, 0.0, 0.2);
        assert!(!verts.is_empty(), "should produce vertices");
        assert!(!indices.is_empty(), "should produce indices");
        // All indices should be valid
        let max_idx = verts.len() as u32;
        for &idx in &indices {
            assert!(
                idx < max_idx,
                "index {} out of bounds (max {})",
                idx,
                max_idx
            );
        }
    }

    #[test]
    fn test_scene_curved_wall() {
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
        let scene = generate_scene(&model);

        assert_eq!(scene.nodes.len(), 1);
        assert_eq!(scene.nodes[0].id, "W-CURVE");
        assert_eq!(scene.nodes[0].element_type, "Wall");

        let mesh = &scene.nodes[0].mesh;
        // Curved wall should have more vertices than a straight wall (arc segments)
        assert!(
            mesh.vertices.len() > 24,
            "curved wall should have more than 24 vertices, got {}",
            mesh.vertices.len()
        );
        assert!(!mesh.indices.is_empty());
    }

    #[test]
    fn test_scene_polygon_with_holes() {
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
        let scene = generate_scene(&model);

        assert_eq!(scene.nodes.len(), 1);
        assert_eq!(scene.nodes[0].id, "S-HOLES");

        let mesh = &scene.nodes[0].mesh;
        assert!(
            !mesh.vertices.is_empty(),
            "slab with holes should have vertices"
        );
        assert!(
            !mesh.indices.is_empty(),
            "slab with holes should have indices"
        );
    }

    /// Area of the top face (normal +z) of a mesh.
    fn top_area(mesh: &SceneMesh) -> f64 {
        mesh.indices
            .chunks(3)
            .map(|t| {
                t.iter()
                    .map(|&i| mesh.vertices[i as usize])
                    .collect::<Vec<_>>()
            })
            .filter(|v| v[0].normal[2] > 0.5)
            .map(|v| {
                let [a, b, c] = [v[0].position, v[1].position, v[2].position]
                    .map(|p| (p[0] as f64, p[1] as f64));
                ((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)).abs() / 2.0
            })
            .sum()
    }

    /// A ceiling fills its space minus the columns, at the space height.
    #[test]
    fn test_scene_covering_default_outline() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
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
        let (model, _) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        let scene = generate_scene(&model);
        let node = scene.nodes.iter().find(|n| n.id == "CV").unwrap();
        assert!((top_area(&node.mesh) - (24.0 - 0.16)).abs() < 1e-3);
        let z_min = node
            .mesh
            .vertices
            .iter()
            .map(|v| v.position[2])
            .fold(f32::MAX, f32::min);
        assert!((z_min - 2.8).abs() < 1e-4, "{z_min}");
    }

    /// A strip footing under a wall hangs `depth` down from the wall base.
    #[test]
    fn test_scene_footing_under_wall() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 1 m
  Wall: W
    from: (0, 0) to: (4, 0)
    height: 3 m
    thickness: 0.3 m
  Footing: FT
    under: W
    width: 0.6 m
    depth: 0.5 m
";
        let (model, _) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        let scene = generate_scene(&model);
        let node = scene.nodes.iter().find(|n| n.id == "FT").unwrap();
        assert!((top_area(&node.mesh) - 4.6 * 0.6).abs() < 1e-3);
        let z: Vec<f32> = node.mesh.vertices.iter().map(|v| v.position[2]).collect();
        let (lo, hi) = z
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        assert!(
            (lo - 0.5).abs() < 1e-4 && (hi - 1.0).abs() < 1e-4,
            "{lo} {hi}"
        );
    }

    /// Openings are voids: not drawn, cut into a slab as holes. A concave hole
    /// and one nested inside it (as Revit writes) must not corrupt the faces.
    #[test]
    fn slab_openings_become_holes() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Slab: SL1
    boundary: [(0,0), (10,0), (10,10), (0,10)]
    thickness: 0.2 m
    Opening: OP2
      boundary: [(4.5,2.5), (5.5,2.5), (5.5,3.5), (4.5,3.5)]
      z_offset: -0.2 m
      height: 0.2 m
    Opening: OP1
      boundary: [(2,2), (6,2), (6,6), (4,6), (4,4), (2,4)]
      z_offset: -0.2 m
      height: 0.2 m
    Opening: OP3
      boundary: [(5,1), (5.2,1), (5.2,8), (5,8)]
      z_offset: -0.2 m
      height: 0.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let slab = &generate_scene(&model).nodes[0];
        assert!(slab.children.is_empty(), "openings are not drawn");
        // 100 m² minus the 12 m² L-shaped opening; the nested one (listed first)
        // is already void and the crossing one is skipped.
        assert!(
            (top_area(&slab.mesh) - 88.0).abs() < 1e-3,
            "{}",
            top_area(&slab.mesh)
        );
    }

    /// A flat roof takes its `thickness` and the holes of its `holes:` property.
    #[test]
    fn flat_roof_thickness_and_holes() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  height: 3 m
  Roof: R1
    type: flat
    boundary: [(0,0), (10,0), (10,10), (0,10)]
    ridge_height: 0 m
    thickness: 0.4 m
    holes: { outer: [(0,0), (10,0), (10,10), (0,10)], holes: [[(2,2), (6,2), (6,5), (2,5)]] }
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, diagnostics) = semantic::analyze(&ast);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let roof = &generate_scene(&model).nodes[0];
        assert!((top_area(&roof.mesh) - 88.0).abs() < 1e-3);
        let zs = roof.mesh.vertices.iter().map(|v| v.position[2]);
        let (lo, hi) = zs.fold((f32::MAX, f32::MIN), |(lo, hi), z| (lo.min(z), hi.max(z)));
        assert!((hi - lo - 0.4).abs() < 1e-4, "{lo}..{hi}");
    }

    /// An `Opening` prism in a wall cuts a void over its footprint along the axis.
    #[test]
    fn wall_boundary_opening_cuts_wall() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (6, 0)
    height: 3 m
    thickness: 0.2 m
    Opening: OP1
      boundary: [(2,-0.1), (3,-0.1), (3,0.1), (2,0.1)]
      z_offset: 1 m
      height: 1 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let wall = &generate_scene(&model).nodes[0];
        assert!(wall.children.is_empty(), "openings are not drawn");
        // Left, below, above and right of the opening.
        assert_eq!(wall.mesh.vertices.len(), 4 * 24);
    }

    #[test]
    fn test_scene_l_shaped_space_boundary() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
#### Space: LRoom
  usage: office
  boundary: [(0,0), (10,0), (10,5), (5,5), (5,8), (0,8)]
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);

        // The space should have a polygon boundary in the model
        let space = &model.sites[0].buildings[0].storeys[0].spaces[0];
        let has_boundary = space.properties.iter().any(|p| p.key == "boundary");
        assert!(has_boundary, "space should have boundary property");
    }

    #[test]
    fn test_scene_furniture_with_asset() {
        let source = concat!(
            "# AssetLibrary: Test\n",
            "## Asset: Desk\n",
            "  width: 1.2 m\n",
            "  depth: 0.6 m\n",
            "  height: 0.75 m\n",
            "### Part: Top\n",
            "  geometry: box(1.2, 0.6, 0.03)\n",
            "  position: (0, 0, 0.72)\n",
            "### Part: Leg1\n",
            "  geometry: box(0.04, 0.6, 0.72)\n",
            "  position: (0.02, 0.1, 0)\n",
            "### Part: Leg2\n",
            "  geometry: cylinder(0.02, 0.72)\n",
            "  position: (1.1, 0.1, 0)\n",
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
        let scene = generate_scene(&model);

        // Should have one furniture node
        let desk_node = scene.nodes.iter().find(|n| n.id == "DESK-01");
        assert!(desk_node.is_some(), "should have DESK-01 node");
        let desk = desk_node.unwrap();

        // Should have 3 child nodes (Top, Leg1, Leg2)
        assert_eq!(desk.children.len(), 3, "desk should have 3 part children");
        assert_eq!(desk.children[0].id, "DESK-01_Top");
        assert_eq!(desk.children[1].id, "DESK-01_Leg1");
        assert_eq!(desk.children[2].id, "DESK-01_Leg2");

        // Each child should have mesh vertices
        for child in &desk.children {
            assert!(
                !child.mesh.vertices.is_empty(),
                "part {} should have vertices",
                child.id
            );
            assert!(
                !child.mesh.indices.is_empty(),
                "part {} should have indices",
                child.id
            );
        }
    }

    #[test]
    fn test_scene_furniture_without_asset() {
        let source = concat!(
            "# Site: S\n",
            "## Building: B\n",
            "### Storey: GF\n",
            "  elevation: 0 m\n",
            "  Furniture: CABINET-01\n",
            "    position: (1.0, 1.0)\n",
        );
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        let cabinet = scene.nodes.iter().find(|n| n.id == "CABINET-01");
        assert!(cabinet.is_some(), "should have CABINET-01 node");
        let cab = cabinet.unwrap();
        assert_eq!(
            cab.children.len(),
            0,
            "plain furniture should have no children"
        );
        assert!(
            !cab.mesh.vertices.is_empty(),
            "should have vertices for bounding box"
        );
    }

    #[test]
    fn test_scene_furniture_assets_fixture() {
        let source = std::fs::read_to_string(format!(
            "{}/../../tests/fixtures/furniture_assets.bml",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        // Should have: 4 walls, 1 slab, 4 furniture items
        assert!(
            scene.nodes.len() >= 9,
            "should have at least 9 nodes, got {}",
            scene.nodes.len()
        );

        // DESK-01 should have 4 part children (Desktop, LeftLeg, RightLeg, DrawerUnit)
        let desk = scene.nodes.iter().find(|n| n.id == "DESK-01").unwrap();
        assert_eq!(desk.children.len(), 4, "DESK-01 should have 4 parts");

        // CHAIR-01 should have 3 part children (Seat, Base, Backrest)
        let chair = scene.nodes.iter().find(|n| n.id == "CHAIR-01").unwrap();
        assert_eq!(chair.children.len(), 3, "CHAIR-01 should have 3 parts");

        // CABINET-01 is plain furniture, no children
        let cabinet = scene.nodes.iter().find(|n| n.id == "CABINET-01").unwrap();
        assert_eq!(cabinet.children.len(), 0, "CABINET-01 should have 0 parts");
    }

    #[test]
    fn test_scene_node_properties() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.24 m
  Column: COL1
    position: (3, 2)
    height: 3.0 m
    profile: rectangle(0.3, 0.3)
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        // Wall should have properties
        let wall = scene.nodes.iter().find(|n| n.id == "W1").unwrap();
        assert!(!wall.properties.is_empty(), "Wall should have properties");

        let prop_keys: Vec<&str> = wall.properties.iter().map(|(k, _)| k.as_str()).collect();
        assert!(
            prop_keys.contains(&"from"),
            "Wall should have 'from' property"
        );
        assert!(
            prop_keys.contains(&"height"),
            "Wall should have 'height' property"
        );
        assert!(
            prop_keys.contains(&"thickness"),
            "Wall should have 'thickness' property"
        );

        // Check 'height' is formatted correctly
        let height = wall.properties.iter().find(|(k, _)| k == "height").unwrap();
        assert_eq!(height.1, "2.8", "Height should be '2.8'");

        // Column should have properties
        let col = scene.nodes.iter().find(|n| n.id == "COL1").unwrap();
        assert!(!col.properties.is_empty(), "Column should have properties");

        let col_keys: Vec<&str> = col.properties.iter().map(|(k, _)| k.as_str()).collect();
        assert!(
            col_keys.contains(&"position"),
            "Column should have 'position' property"
        );
        assert!(
            col_keys.contains(&"height"),
            "Column should have 'height' property"
        );
        assert!(
            col_keys.contains(&"profile"),
            "Column should have 'profile' property"
        );
    }

    #[test]
    fn test_scene_child_properties() {
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
      offset: 1.0 m
      width: 1.2 m
      height: 1.0 m
      sill_height: 0.9 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene = generate_scene(&model);

        let wall = scene.nodes.iter().find(|n| n.id == "W1").unwrap();
        assert!(!wall.children.is_empty(), "Wall should have window child");

        let window = &wall.children[0];
        assert_eq!(window.id, "WIN1");
        assert!(
            !window.properties.is_empty(),
            "Window should have properties"
        );

        let win_keys: Vec<&str> = window.properties.iter().map(|(k, _)| k.as_str()).collect();
        assert!(
            win_keys.contains(&"offset"),
            "Window should have 'offset' property"
        );
        assert!(
            win_keys.contains(&"width"),
            "Window should have 'width' property"
        );
        assert!(
            win_keys.contains(&"sill_height"),
            "Window should have 'sill_height' property"
        );
    }

    #[test]
    fn test_format_prop_number() {
        assert_eq!(format_prop_number(2.8), "2.8");
        assert_eq!(format_prop_number(3.0), "3");
        assert_eq!(format_prop_number(0.24), "0.24");
        assert_eq!(format_prop_number(0.0), "0");
        assert_eq!(format_prop_number(10.5), "10.5");
    }

    #[test]
    fn vertex_serializes_without_f64_promotion_noise() {
        // -15.2642 is not representable in f32, so widening it to f64 for JSON
        // prints the whole approximation. That is what used to reach the wire.
        let v = Vertex {
            position: [-15.2642, 18.0942, 0.0],
            normal: [-0.009_517_763, -0.999_940_3, -0.005_377_846],
        };
        let json = serde_json::to_string(&v).unwrap();
        assert!(
            !json.contains("15.264200210571289"),
            "f64 promotion digits leaked into the payload: {json}"
        );
        assert_eq!(
            json,
            r#"{"position":[-15.2642,18.0942,0.0],"normal":[-0.0095,-0.9999,-0.0054]}"#
        );
    }

    #[test]
    fn vertex_round_trips_within_a_tenth_of_a_millimetre() {
        let v = Vertex {
            position: [1.234_56, -7.654_32, 19.099_99],
            normal: [0.0, 0.0, 1.0],
        };
        let back: Vertex = serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
        for k in 0..3 {
            assert!(
                (back.position[k] - v.position[k]).abs() <= 1e-4,
                "axis {k}: {} vs {}",
                back.position[k],
                v.position[k]
            );
        }
    }

    #[test]
    fn round4_keeps_zero_unsigned() {
        assert_eq!(round4(-0.000_01), 0.0);
        assert!(
            !serde_json::to_string(&round4(-0.000_01))
                .unwrap()
                .starts_with('-')
        );
    }

    #[test]
    fn scene_bounds_survive_serialization() {
        // The whole point: rounding must not move the model.
        let mesh = SceneMesh {
            vertices: vec![
                Vertex {
                    position: [-26.276, -26.869, -1.02],
                    normal: [0.0, 0.0, 1.0],
                },
                Vertex {
                    position: [14.524, 25.106, 19.1],
                    normal: [0.0, 0.0, 1.0],
                },
            ],
            indices: vec![0, 1, 0],
            color: [1.0, 1.0, 1.0, 1.0],
        };
        let node = SceneNode {
            id: "n".into(),
            element_type: "Wall".into(),
            label: "n".into(),
            mesh,
            children: vec![],
            properties: vec![],
            material: None,
        };
        let graph = SceneGraph {
            nodes: vec![node],
            materials: vec![],
        };
        let back: SceneGraph =
            serde_json::from_str(&serde_json::to_string(&graph).unwrap()).unwrap();
        let v = &back.nodes[0].mesh.vertices;
        assert_eq!(v[0].position, [-26.276, -26.869, -1.02]);
        assert_eq!(v[1].position, [14.524, 25.106, 19.1]);
    }

    /// A curtain wall draws its parts and no body of its own; an upright
    /// plate stands on its axis like a wall.
    #[test]
    fn test_scene_curtain_wall() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: CW
    from: (0, 0) to: (3, 0)
    height: 3 m
    thickness: 0.03 m
    mullions: 1.5 m
    Door: CW-D
      offset: 0 m
      width: 1.5 m
      height: 3 m
";
        let (model, _) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        let scene = generate_scene(&model);
        let ids: Vec<&str> = scene.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(!ids.contains(&"CW"), "{ids:?}");
        // The edge mullion beside the door stays whole.
        assert!(
            ["CW-D", "CW-M1", "CW-M3"].iter().all(|id| ids.contains(id)),
            "{ids:?}"
        );
        let panel = scene.nodes.iter().find(|n| n.id == "CW-P1-2").unwrap();
        let (lo, hi) = panel
            .mesh
            .vertices
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), v| {
                (a.min(v.position[1]), b.max(v.position[1]))
            });
        assert!((hi - lo - 0.03).abs() < 1e-4, "{lo} {hi}");
    }

    #[test]
    fn test_scene_mep_bend_and_terminal() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 1 m
  Pipe: P
    diameter: 0.1 m
    path: [(0, 0, 2), (1, 0, 2), (1, 1, 2)]
  Terminal: T
    type: washbasin
    position: (3, 3)
";
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        assert!(diags.is_empty(), "{diags:?}");
        let scene = generate_scene(&model);
        let bbox = |id: &str| {
            let node = scene.nodes.iter().find(|n| n.id == id).unwrap();
            let mut lo = [f32::MAX; 3];
            let mut hi = [f32::MIN; 3];
            for v in &node.mesh.vertices {
                for k in 0..3 {
                    lo[k] = lo[k].min(v.position[k]);
                    hi[k] = hi[k].max(v.position[k]);
                }
            }
            (lo, hi)
        };
        // Bend radius 1.5 diameters: the elbow spans 0.15 m plus half the
        // pipe on each leg, at storey elevation + 2 m.
        let (lo, hi) = bbox("P-B1");
        let near = |a: f32, b: f64| (a as f64 - b).abs() < 2e-3;
        assert!(near(lo[0], 0.85) && near(hi[0], 1.05), "{lo:?} {hi:?}");
        assert!(near(lo[1], -0.05) && near(hi[1], 0.15), "{lo:?} {hi:?}");
        let (lo, hi) = bbox("T");
        assert!(hi[0] > lo[0] && hi[1] > lo[1] && hi[2] > lo[2]);
    }

    /// Plan and height bounds of a mesh.
    fn mesh_bbox(m: &SceneMesh) -> ([f32; 3], [f32; 3]) {
        m.vertices
            .iter()
            .fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), v| {
                (
                    std::array::from_fn(|i| lo[i].min(v.position[i])),
                    std::array::from_fn(|i| hi[i].max(v.position[i])),
                )
            })
    }

    #[test]
    fn arc_wall_scene_unchanged() {
        let source = std::fs::read_to_string("../../tests/fixtures/phase7c_geometry.bml").unwrap();
        let (model, _) = semantic::analyze(&cst_to_ast::parse(&source).unwrap());
        let scene = generate_scene(&model);
        let wall = scene.nodes.iter().find(|n| n.id == "W-CURVE-01").unwrap();
        // Recorded before walls moved onto `WallAxis`.
        let (lo, hi) = mesh_bbox(&wall.mesh);
        let want = ([11.9077, -0.0385, 0.0], [18.0923, 2.1, 3.0]);
        for i in 0..3 {
            assert!((lo[i] - want.0[i]).abs() < 0.005, "{lo:?}");
            assert!((hi[i] - want.1[i]).abs() < 0.005, "{hi:?}");
        }
    }

    #[test]
    fn door_on_spline_wall_follows_curve() {
        let source = "\
# Site: S
## Building: B
### Storey: L1
  elevation: 0 m
  Wall: W-S
    path: [(0, 0), smooth (4, 1.5), smooth (8, 1), (12, 3)]
    height: 3 m
    thickness: 0.2 m
    Door: D-1
      offset: 3 m
      width: 1 m
      height: 2.1 m
";
        let (model, _) = semantic::analyze(&cst_to_ast::parse(source).unwrap());
        let wall = model.elements().find(|e| e.id == "W-S").unwrap();
        let axis = crate::geometry::axis::WallAxis::of(wall, 0.005).unwrap();
        let scene = generate_scene(&model);
        let node = scene.nodes.iter().find(|n| n.id == "W-S").unwrap();
        // The wall follows the curve and leaves a void for the door.
        let (lo, hi) = mesh_bbox(&node.mesh);
        assert!(lo[1] < 0.0 && hi[1] > 3.0, "{lo:?} {hi:?}");
        let door = node.children.iter().find(|c| c.id == "D-1").unwrap();
        let (lo, hi) = mesh_bbox(&door.mesh);
        let c = axis.point_at(3.5);
        let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        assert!(
            (centre[0] as f64 - c[0]).abs() < 0.005 && (centre[1] as f64 - c[1]).abs() < 0.005,
            "{centre:?} vs {c:?}"
        );
        // Rotated to the tangent: 1 m along it, the 0.05 m leaf across it.
        let t = axis.tangent_at(3.5);
        let extent = |d: [f64; 2]| {
            let s = door
                .mesh
                .vertices
                .iter()
                .map(|v| v.position[0] as f64 * d[0] + v.position[1] as f64 * d[1]);
            s.clone().fold(f64::MIN, f64::max) - s.fold(f64::MAX, f64::min)
        };
        assert!((extent(t) - 1.0).abs() < 0.005, "{}", extent(t));
        assert!((extent([-t[1], t[0]]) - 0.05).abs() < 0.005);
        // The wall has a void where the door is: no wall vertex inside it.
        let p = axis.point_at(3.5);
        assert!(!node.mesh.vertices.iter().any(|v| {
            (v.position[0] as f64 - p[0]).hypot(v.position[1] as f64 - p[1]) < 0.3
                && v.position[2] < 2.0
        }));
    }
}
