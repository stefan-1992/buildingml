use std::collections::{BTreeMap, HashMap, HashSet};

use ifc_rs::prelude::{IfcValue, LabelValue, RealValue};

use crate::scene::graph::outline_holes;
use crate::scene::{SceneMaterial, appearance};
use crate::semantic::model::{
    self, ElementType, MaterialType, ResolvedElement, ResolvedProperty, ResolvedSpace,
    ResolvedStorey, ResolvedValue, SemanticModel, contents, number as number_prop, string_or_ident,
};
use crate::semantic::registry;

use super::geometry;
use super::mep;
use super::pset_mapping;
use super::step_writer::{
    Frame, Profile, Shape, Solid, StepWriter, generate_ifc_guid, length, refs, sf, step_str, sub,
};
use super::wall_joins::{self, JoinEnd, WallEnds};
use crate::config::default_curve_tolerance;
use crate::geometry::axis::WallAxis;
use crate::geometry::curve::{Curve, Curve2, Segment};
use crate::semantic::model::ElementCurve;

/// Errors during IFC code generation.
#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    #[error("wall `{0}` is missing required property `{1}`")]
    MissingWallProperty(String, &'static str),

    #[error("slab `{0}` is missing required property `{1}`")]
    MissingSlabProperty(String, &'static str),

    #[error("element `{0}` is missing required property `{1}`")]
    MissingElementProperty(String, &'static str),

    #[error("no spatial structure found in model")]
    NoSpatialStructure,
}

/// Warnings emitted during IFC code generation (non-fatal issues).
#[derive(Debug, Clone)]
pub struct CodegenWarning {
    pub element_id: String,
    pub message: String,
}

impl std::fmt::Display for CodegenWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "codegen warning for `{}`: {}",
            self.element_id, self.message
        )
    }
}

/// Result of IFC code generation, including the output and any warnings.
pub struct CodegenOutput {
    pub ifc_string: String,
    pub warnings: Vec<CodegenWarning>,
}

// ============================================================================
// Main IFC generation
// ============================================================================

/// Generate an IFC4 STEP file from a semantic model.
pub fn generate_ifc(model: &SemanticModel) -> Result<String, CodegenError> {
    generate_ifc_with_warnings(model).map(|output| output.ifc_string)
}

/// Generate an IFC4 STEP file from a semantic model, returning warnings alongside the output.
///
/// The file is written entity by entity: every element gets a placement relative to
/// its storey, a 'Body' representation of extruded solids, and its property sets.
/// Elements that carry a `guid:` keep it as their GlobalId.
pub fn generate_ifc_with_warnings(model: &SemanticModel) -> Result<CodegenOutput, CodegenError> {
    if model.sites.is_empty() {
        return Err(CodegenError::NoSpatialStructure);
    }
    let mut g = Gen {
        w: StepWriter::new(),
        model,
        context: 0,
        body: 0,
        axis: None,
        warnings: Vec::new(),
        guids: HashSet::new(),
        written: HashMap::new(),
        parts: Vec::new(),
        fills: Vec::new(),
        connects: Vec::new(),
        openings: HashMap::new(),
        ids: HashSet::new(),
        materials: Vec::new(),
        material_index: HashMap::new(),
        material_ids: HashMap::new(),
        layer_set_ids: HashMap::new(),
        elevation: 0.0,
        building: None,
        ports: Vec::new(),
        terminals: Vec::new(),
        systems: BTreeMap::new(),
    };
    // `part_of:` must name one of these to become an aggregate part.
    for e in model.elements() {
        g.ids.insert(e.id.clone());
    }
    g.project()?;
    g.ports();
    g.systems();
    g.aggregates();
    g.fills();
    g.connections();
    g.materials();
    let ifc_string = format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ViewDefinition [DesignTransferView]'),'2;1');\n\
         FILE_NAME('model.ifc','2026-01-01T00:00:00',(''),(''),'BuildingML','BuildingML Compiler','');\n\
         FILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n{}ENDSEC;\nEND-ISO-10303-21;\n",
        g.w.lines
    );
    Ok(CodegenOutput {
        ifc_string,
        warnings: g.warnings,
    })
}

struct Gen<'m> {
    w: StepWriter,
    model: &'m SemanticModel,
    /// 'Model' representation context.
    context: u64,
    /// 'Body' representation sub-context.
    body: u64,
    /// 'Axis' representation sub-context, written on first use.
    axis: Option<u64>,
    warnings: Vec<CodegenWarning>,
    guids: HashSet<String>,
    /// Entity id of every element written so far.
    written: HashMap<String, u64>,
    /// (whole element id, part entity id) for IfcRelAggregates.
    parts: Vec<(String, u64)>,
    /// (`opening:` id, filler entity id) of windows and doors outside their host.
    fills: Vec<(String, u64)>,
    /// (element id, entity id, realizes the connection, `connects:` ids).
    connects: Vec<(String, u64, bool, Vec<String>)>,
    /// Entity id of every opening written so far, the targets of `fills`.
    openings: HashMap<String, u64>,
    /// Ids of all top-level and space elements, to validate `part_of:`.
    ids: HashSet<String>,
    /// (`material:` name, layer set usage, entity ids) for IfcRelAssociatesMaterial.
    /// The usage holds the IfcMaterialLayerSetUsage attributes after the set
    /// (see `layer_usage`); `None` associates the material or layer set itself.
    materials: Vec<(String, Option<String>, Vec<u64>)>,
    /// Index into `materials` by (name, usage).
    material_index: HashMap<(String, Option<String>), usize>,
    /// Entity id of each IfcMaterial by material name.
    material_ids: HashMap<String, u64>,
    /// Entity id of each IfcMaterialLayerSet by material name.
    layer_set_ids: HashMap<String, u64>,
    /// Elevation of the storey being written (0 for building and site).
    elevation: f64,
    /// IfcBuilding being written, served by the systems of its elements.
    building: Option<u64>,
    /// Connection points of ducts, pipes and cable trays, for `ports`.
    ports: Vec<RunPort>,
    /// Terminals and equipment that free run ends connect to.
    terminals: Vec<Terminal>,
    /// (`system:` name, member entity ids, buildings served).
    systems: BTreeMap<String, (Vec<u64>, Vec<u64>)>,
}

/// IFC `predefined_type` of each `System:` element, by id.
fn system_types(model: &SemanticModel) -> HashMap<&str, String> {
    model
        .elements()
        .filter(|e| e.element_type == ElementType::System)
        .filter_map(|e| {
            let t = string_or_ident(&e.properties, "predefined_type")?;
            Some((e.id.as_str(), t.to_ascii_uppercase()))
        })
        .collect()
}

/// A connection point of a run element.
struct RunPort {
    host: u64,
    /// Placement of the host's storey.
    storey: u64,
    /// Point in storey coordinates, and the storey elevation.
    at: [f64; 3],
    elevation: f64,
    port_type: &'static str,
    system: Option<String>,
}

/// A terminal or equipment body, in storey coordinates.
struct Terminal {
    entity: u64,
    storey: u64,
    lo: [f64; 3],
    hi: [f64; 3],
    elevation: f64,
}

/// Per-storey state while its elements are written.
struct StoreyCtx<'a> {
    placement: u64,
    /// Storey `height` property (default 3 m).
    height: f64,
    /// Base of roofs without `z_offset`: the storey height when the storey has
    /// walls, else 0 (a dedicated roof storey sits on the walls below).
    roof_base: f64,
    storeys: &'a [ResolvedStorey],
    /// Elements contained in the storey (IfcRelContainedInSpatialStructure).
    contained: Vec<u64>,
    /// Ends of the walls a join changed (`wall_joins`).
    wall_ends: HashMap<String, WallEnds>,
}

impl Gen<'_> {
    fn warn(&mut self, id: &str, message: impl Into<String>) {
        self.warnings.push(CodegenWarning {
            element_id: id.to_string(),
            message: message.into(),
        });
    }

    /// The element's `guid:` when present and unused, else a fresh one.
    fn guid(&mut self, props: &[ResolvedProperty]) -> String {
        if let Some(g) = string_prop(props, "guid")
            && g.len() == 22
            && self.guids.insert(g.to_string())
        {
            return g.to_string();
        }
        loop {
            let g = generate_ifc_guid();
            if self.guids.insert(g.clone()) {
                return g;
            }
        }
    }

    fn rel(&mut self, class: &str, args: &str) -> u64 {
        let guid = self.guid(&[]);
        self.w.add(&format!("{}('{}',$,$,$,{})", class, guid, args))
    }

    fn project(&mut self) -> Result<(), CodegenError> {
        let units = [
            self.w.add("IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.)"),
            self.w.add("IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.)"),
            self.w.add("IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.)"),
            self.w.add("IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.)"),
        ];
        let units = self
            .w
            .add(&format!("IFCUNITASSIGNMENT(({}))", refs(&units)));
        let wcs = self.w.placement3(&Frame::identity());
        let ctx = self.w.add(&format!(
            "IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#{},$)",
            wcs
        ));
        self.context = ctx;
        self.body = self.w.add(&format!(
            "IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Body','Model',*,*,*,*,#{},$,.MODEL_VIEW.,$)",
            ctx
        ));
        let guid = self.guid(&[]);
        let project = self.w.add(&format!(
            "IFCPROJECT('{}',$,'BuildingML Project',$,$,$,$,(#{}),#{})",
            guid, ctx, units
        ));

        let model = self.model;
        let mut sites = Vec::new();
        for site in &model.sites {
            let pl = self.w.local_placement(None, &Frame::identity());
            let guid = self.guid(&site.properties);
            let site_id = self.w.add(&format!(
                "IFCSITE('{}',$,{},$,$,#{},$,$,.ELEMENT.,$,$,$,$,$)",
                guid,
                step_str(&site.name),
                pl
            ));
            sites.push(site_id);
            let mut buildings = Vec::new();
            for building in &site.buildings {
                let bpl = self.w.local_placement(Some(pl), &Frame::identity());
                let guid = self.guid(&building.properties);
                let building_id = self.w.add(&format!(
                    "IFCBUILDING('{}',$,{},$,$,#{},$,$,.ELEMENT.,$,$,$)",
                    guid,
                    step_str(&building.name),
                    bpl
                ));
                buildings.push(building_id);
                self.building = Some(building_id);
                let mut storeys = Vec::new();
                for storey in &building.storeys {
                    storeys.push(self.storey(storey, bpl, &building.storeys)?);
                }
                let spaces = self.container(
                    building_id,
                    bpl,
                    3.0,
                    &building.spaces,
                    &building.elements,
                    &building.storeys,
                )?;
                storeys.extend(spaces);
                if !storeys.is_empty() {
                    self.rel(
                        "IFCRELAGGREGATES",
                        &format!("#{},({})", building_id, refs(&storeys)),
                    );
                }
            }
            self.building = None;
            let spaces = self.container(site_id, pl, 3.0, &site.spaces, &site.elements, &[])?;
            buildings.extend(spaces);
            if !buildings.is_empty() {
                self.rel(
                    "IFCRELAGGREGATES",
                    &format!("#{},({})", site_id, refs(&buildings)),
                );
            }
        }
        self.rel(
            "IFCRELAGGREGATES",
            &format!("#{},({})", project, refs(&sites)),
        );
        Ok(())
    }

    fn storey(
        &mut self,
        storey: &ResolvedStorey,
        building_placement: u64,
        all: &[ResolvedStorey],
    ) -> Result<u64, CodegenError> {
        let elevation = number_prop(&storey.properties, "elevation").unwrap_or(0.0);
        let height = number_prop(&storey.properties, "height").unwrap_or(3.0);
        let placement = self.w.local_placement(
            Some(building_placement),
            &Frame::upright([0.0, 0.0, elevation], 0.0),
        );
        let guid = self.guid(&storey.properties);
        self.elevation = elevation;
        let storey_id = self.w.add(&format!(
            "IFCBUILDINGSTOREY('{}',$,{},$,$,#{},$,$,.ELEMENT.,{})",
            guid,
            step_str(string_prop(&storey.properties, "ifc_name").unwrap_or(&storey.name)),
            placement,
            sf(elevation)
        ));
        let spaces = self.container(
            storey_id,
            placement,
            height,
            &storey.spaces,
            &storey.elements,
            all,
        )?;
        self.elevation = 0.0;
        if !spaces.is_empty() {
            self.rel(
                "IFCRELAGGREGATES",
                &format!("#{},({})", storey_id, refs(&spaces)),
            );
        }
        Ok(storey_id)
    }

    /// Write the spaces and elements of a storey, building or site. Elements are
    /// contained in their space, the rest in `container`; returns the space ids
    /// for the caller to aggregate.
    fn container(
        &mut self,
        container: u64,
        placement: u64,
        height: f64,
        spaces: &[ResolvedSpace],
        elements: &[ResolvedElement],
        storeys: &[ResolvedStorey],
    ) -> Result<Vec<u64>, CodegenError> {
        let has_walls = contents(spaces, elements).any(|e| e.element_type == ElementType::Wall);
        let joins = wall_joins::join_walls(contents(spaces, elements));
        let mut ctx = StoreyCtx {
            placement,
            height,
            roof_base: if has_walls { height } else { 0.0 },
            storeys,
            contained: Vec::new(),
            wall_ends: joins.ends,
        };

        let mut space_ids = Vec::new();
        for space in spaces {
            let space_id = self.space(space, &ctx);
            space_ids.extend(space_id);
            let outer = std::mem::take(&mut ctx.contained);
            for elem in &space.elements {
                self.element(elem, &mut ctx)?;
            }
            let in_space = std::mem::replace(&mut ctx.contained, outer);
            let coverings: Vec<u64> = space
                .elements
                .iter()
                .filter(|e| e.element_type == ElementType::Covering)
                .filter_map(|e| self.written.get(&e.id).copied())
                .collect();
            if let Some(id) = space_id
                && !coverings.is_empty()
            {
                self.rel(
                    "IFCRELCOVERSSPACES",
                    &format!("#{},({})", id, refs(&coverings)),
                );
            }
            match space_id {
                Some(id) if !in_space.is_empty() => {
                    self.rel(
                        "IFCRELCONTAINEDINSPATIALSTRUCTURE",
                        &format!("({}),#{}", refs(&in_space), id),
                    );
                }
                _ => ctx.contained.extend(in_space),
            }
        }
        for elem in elements {
            self.element(elem, &mut ctx)?;
        }

        if !ctx.contained.is_empty() {
            self.rel(
                "IFCRELCONTAINEDINSPATIALSTRUCTURE",
                &format!("({}),#{}", refs(&ctx.contained), container),
            );
        }
        for j in joins.joins {
            if let (Some(a), Some(b)) =
                (self.written.get(&j.relating), self.written.get(&j.related))
            {
                let args = format!(
                    "$,#{a},#{b},(),(),{},{}",
                    connection_type(j.related_end),
                    connection_type(j.relating_end)
                );
                self.rel("IFCRELCONNECTSPATHELEMENTS", &args);
            }
        }
        Ok(space_ids)
    }

    fn space(&mut self, space: &ResolvedSpace, ctx: &StoreyCtx) -> Option<u64> {
        let props = &space.properties;
        let (outer, holes) = if let Some(b) = polygon_prop(props, "boundary") {
            b
        } else if let Some(area) = number_prop(props, "area") {
            let s = area.sqrt();
            (vec![(0.0, 0.0), (s, 0.0), (s, s), (0.0, s)], vec![])
        } else {
            self.warn(
                &space.name,
                "space has no boundary or area; skipping IFC geometry generation",
            );
            return None;
        };
        let base = number_prop(props, "z_offset").unwrap_or(0.0);
        let height = number_prop(props, "height").unwrap_or(ctx.height);
        let pl = self
            .w
            .local_placement(Some(ctx.placement), &Frame::identity());
        let profile = region_profile(space.curve("boundary"), outer, holes, |q| q);
        let shape = self
            .w
            .body(self.body, &[Solid::vertical(profile, base, height)]);
        let guid = self.guid(props);
        let name = string_prop(props, "ifc_name").unwrap_or(&space.name);
        let long_name = string_prop(props, "long_name").map_or("$".to_string(), step_str);
        Some(self.w.add(&format!(
            "IFCSPACE('{}',$,{},$,$,#{},#{},{},.ELEMENT.,$,$)",
            guid,
            step_str(name),
            pl,
            shape,
            long_name
        )))
    }

    /// Write one product entity. `tail` holds the attributes after Tag.
    #[allow(clippy::too_many_arguments)]
    fn product(
        &mut self,
        class: &str,
        id: &str,
        props: &[ResolvedProperty],
        object_type: Option<&str>,
        placement: u64,
        shape: Option<u64>,
        tail: &str,
    ) -> u64 {
        let guid = self.guid(props);
        let name = string_prop(props, "ifc_name").unwrap_or(id);
        let shape = shape.map_or("$".to_string(), |s| format!("#{}", s));
        let object_type = object_type.map_or("$".to_string(), step_str);
        self.w.add(&format!(
            "{}('{}',$,{},$,{},#{},{},{}{})",
            class,
            guid,
            step_str(name),
            object_type,
            placement,
            shape,
            step_str(id),
            tail
        ))
    }

    /// Queue `entity` for an IfcRelAssociatesMaterial with material `m`.
    fn associate(&mut self, entity: u64, m: &str, usage: Option<String>) {
        let key = (m.to_string(), usage);
        match self.material_index.get(&key) {
            Some(&i) => self.materials[i].2.push(entity),
            None => {
                self.material_index
                    .insert(key.clone(), self.materials.len());
                self.materials.push((key.0, key.1, vec![entity]));
            }
        }
    }

    /// Associate an element's `material:`; a layered one with the layer set
    /// usage its body allows.
    fn associate_element(&mut self, elem: &ResolvedElement, entity: u64) {
        let Some(m) = string_or_ident(&elem.properties, "material") else {
            return;
        };
        let layered = self.model.material(m).and_then(|d| d.layers()).is_some();
        let usage = if layered { layer_usage(elem) } else { None };
        self.associate(entity, m, usage);
    }

    /// Write a shaped product with its property sets; returns its entity id.
    fn shaped(&mut self, elem: &ResolvedElement, shape: &Shape, ctx: &StoreyCtx) -> u64 {
        let pl = self.w.local_placement(Some(ctx.placement), &shape.frame);
        let body = match axis_curve(elem, &shape.frame) {
            Some(axis) => {
                let axis_ctx = self.axis_context();
                self.w
                    .body_with_axis(self.body, axis_ctx, &shape.solids, &axis)
            }
            None => self.w.body(self.body, &shape.solids),
        };
        let (class, object_type, tail) = class_and_tail(elem);
        let id = self.product(
            &class,
            &elem.id,
            &elem.properties,
            object_type.as_deref(),
            pl,
            Some(body),
            &tail,
        );
        self.register(elem, id, &class);
        id
    }

    /// The 'Axis' sub-context, written the first time a wall needs it.
    fn axis_context(&mut self) -> u64 {
        if let Some(id) = self.axis {
            return id;
        }
        let id = self.w.add(&format!(
            "IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Axis','Model',*,*,*,*,#{},$,.GRAPH_VIEW.,$)",
            self.context
        ));
        self.axis = Some(id);
        id
    }

    /// Record a written element for references to it, and queue its
    /// material, property sets and `connects:`.
    fn register(&mut self, elem: &ResolvedElement, id: u64, class: &str) {
        self.written.insert(elem.id.clone(), id);
        self.associate_element(elem, id);
        self.psets(elem, id, class);
        let targets = model::refs(&elem.properties, "connects");
        if !targets.is_empty() {
            let realizes = REALIZING_CLASSES.contains(&class);
            let targets = targets.into_iter().map(str::to_string).collect();
            self.connects.push((elem.id.clone(), id, realizes, targets));
        }
    }

    /// Product without a body (`representation: none`), placed at the storey origin.
    fn bodiless(&mut self, elem: &ResolvedElement, ctx: &StoreyCtx) -> u64 {
        let pl = self
            .w
            .local_placement(Some(ctx.placement), &Frame::identity());
        let (class, object_type, tail) = class_and_tail(elem);
        let id = self.product(
            &class,
            &elem.id,
            &elem.properties,
            object_type.as_deref(),
            pl,
            None,
            &tail,
        );
        self.register(elem, id, &class);
        id
    }

    fn element(&mut self, elem: &ResolvedElement, ctx: &mut StoreyCtx) -> Result<(), CodegenError> {
        match elem.element_type {
            ElementType::Grid => {
                // A `boundary:` gives the grid a prism body (an object exported
                // as IfcGrid, e.g. a drain grating).
                let shape = match polygon_prop(&elem.properties, "boundary") {
                    Some(_) => self.shape(elem, ctx)?,
                    None => None,
                };
                let id = self.grid(elem, ctx, shape.as_ref());
                ctx.contained.push(id);
                return Ok(());
            }
            ElementType::Furniture if string_or_ident(&elem.properties, "asset").is_some() => {
                self.furniture_asset(elem, ctx);
                return Ok(());
            }
            // Written as IfcDistributionSystem by `systems`.
            ElementType::System => return Ok(()),
            _ => {}
        }
        if no_body(elem) {
            let id = self.bodiless(elem, ctx);
            self.place(elem, id, ctx);
            self.services(elem, id, ctx);
            return self.children(elem, id, &Frame::identity(), ctx);
        }
        let Some(shape) = self.shape(elem, ctx)? else {
            return Ok(());
        };
        let host = self.shaped(elem, &shape, ctx);
        self.place(elem, host, ctx);
        self.services(elem, host, ctx);
        self.children(elem, host, &shape.frame, ctx)
    }

    /// Record a services element's ports, body and system for `ports` and
    /// `systems`.
    fn services(&mut self, elem: &ResolvedElement, entity: u64, ctx: &StoreyCtx) {
        let system = string_or_ident(&elem.properties, "system");
        for at in mep::ports(elem) {
            self.ports.push(RunPort {
                host: entity,
                storey: ctx.placement,
                at,
                elevation: self.elevation,
                port_type: mep::port_type(elem.element_type),
                system: system.map(str::to_string),
            });
        }
        if elem.element_type != ElementType::Accessory
            && let Some((lo, hi)) = mep::body_box(elem)
        {
            self.terminals.push(Terminal {
                entity,
                storey: ctx.placement,
                lo,
                hi,
                elevation: self.elevation,
            });
        }
        let Some(name) = system else {
            return;
        };
        let (members, buildings) = self.systems.entry(name.to_string()).or_default();
        members.push(entity);
        if let Some(b) = self.building
            && !buildings.contains(&b)
        {
            buildings.push(b);
        }
    }

    /// IfcDistributionPort at every run end, nested in its element. Two ports
    /// of the same type within 1 mm connect, each port at most once; a free
    /// end inside a terminal or equipment body connects to a port added to
    /// that body.
    fn ports(&mut self) {
        const TOL: f64 = 1e-3;
        let ports = std::mem::take(&mut self.ports);
        let world: Vec<[f64; 3]> = ports
            .iter()
            .map(|p| [p.at[0], p.at[1], p.at[2] + p.elevation])
            .collect();
        let cell = |w: [f64; 3]| w.map(|v| (v / TOL).floor() as i64);
        let mut grid: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
        for (i, w) in world.iter().enumerate() {
            grid.entry(cell(*w)).or_default().push(i);
        }
        // Other ports within TOL of port `i`, in index order.
        let near = |i: usize| {
            let c = cell(world[i]);
            let mut out: Vec<usize> = (0..27)
                .filter_map(|n| {
                    let d = [n / 9, n / 3 % 3, n % 3].map(|v| v as i64 - 1);
                    grid.get(&[c[0] + d[0], c[1] + d[1], c[2] + d[2]])
                })
                .flatten()
                .copied()
                .filter(|&j| j != i && length(sub(world[i], world[j])) < TOL)
                .collect();
            out.sort_unstable();
            out
        };
        let mut paired = vec![false; ports.len()];
        let types = system_types(self.model);
        let mut nested: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        let mut nest = |host: u64, port: u64| nested.entry(host).or_default().push(port);
        let mut written = vec![0; ports.len()];
        let mut pairs = Vec::new();
        for (i, p) in ports.iter().enumerate() {
            let system = p
                .system
                .as_deref()
                .and_then(|s| types.get(s))
                .map_or("$".to_string(), |t| format!(".{t}."));
            let port = self.port(p.storey, p.at, p.port_type, &system);
            nest(p.host, port);
            written[i] = port;
            // Unpaired ports of the same type within TOL; a later one pairs with this one.
            let free: Vec<usize> = near(i)
                .into_iter()
                .filter(|&j| !paired[j] && ports[j].port_type == p.port_type)
                .collect();
            if let Some(&j) = free.first() {
                if j < i {
                    paired[j] = true;
                    paired[i] = true;
                    pairs.push((written[j], port));
                }
                continue;
            }
            let world = world[i];
            let inside = |t: &Terminal| {
                (0..3).all(|k| {
                    let e = if k == 2 { t.elevation } else { 0.0 };
                    (t.lo[k] + e - 0.01..=t.hi[k] + e + 0.01).contains(&world[k])
                })
            };
            if let Some(t) = self.terminals.iter().find(|t| inside(t)) {
                let (entity, storey) = (t.entity, t.storey);
                let local = [world[0], world[1], world[2] - t.elevation];
                let end = self.port(storey, local, p.port_type, &system);
                nest(entity, end);
                pairs.push((port, end));
            }
        }
        for (host, ports) in nested {
            self.rel("IFCRELNESTS", &format!("#{},({})", host, refs(&ports)));
        }
        for (a, b) in pairs {
            self.rel("IFCRELCONNECTSPORTS", &format!("#{a},#{b},$"));
        }
    }

    fn port(&mut self, storey: u64, at: [f64; 3], port_type: &str, system: &str) -> u64 {
        let pl = self
            .w
            .local_placement(Some(storey), &Frame::upright(at, 0.0));
        let guid = self.guid(&[]);
        self.w.add(&format!(
            "IFCDISTRIBUTIONPORT('{guid}',$,$,$,$,#{pl},$,.SOURCEANDSINK.,.{port_type}.,{system})"
        ))
    }

    /// An IfcDistributionSystem per `system:` name with its members, serving
    /// the buildings they are in.
    fn systems(&mut self) {
        let types = system_types(self.model);
        for (name, (members, buildings)) in std::mem::take(&mut self.systems) {
            let def = self
                .model
                .elements()
                .find(|e| e.element_type == ElementType::System && e.id == name);
            let props = def.map_or(&[][..], |e| &e.properties[..]);
            let guid = self.guid(props);
            let long = string_prop(props, "ifc_name").map_or("$".to_string(), step_str);
            let kind = types
                .get(name.as_str())
                .map_or("NOTDEFINED", String::as_str);
            let sys = self.w.add(&format!(
                "IFCDISTRIBUTIONSYSTEM('{guid}',$,{},$,$,{long},.{kind}.)",
                step_str(&name)
            ));
            self.rel(
                "IFCRELASSIGNSTOGROUP",
                &format!("({}),$,#{sys}", refs(&members)),
            );
            if !buildings.is_empty() {
                self.rel(
                    "IFCRELSERVICESBUILDINGS",
                    &format!("#{sys},({})", refs(&buildings)),
                );
            }
        }
    }

    /// Contain an element in the current spatial structure, or record it as a
    /// part of its `part_of:` whole.
    fn place(&mut self, elem: &ResolvedElement, entity: u64, ctx: &mut StoreyCtx) {
        if elem.element_type == ElementType::Opening {
            self.openings.insert(elem.id.clone(), entity);
        }
        if elem.element_type != ElementType::Opening
            && let Some(o) = string_or_ident(&elem.properties, "opening")
        {
            self.fills.push((o.to_string(), entity));
        }
        match string_or_ident(&elem.properties, "part_of") {
            Some(w) if self.ids.contains(w) => self.parts.push((w.to_string(), entity)),
            Some(w) => {
                self.warn(
                    &elem.id,
                    format!("`part_of: {}` names no element; contained in the storey", w),
                );
                ctx.contained.push(entity);
            }
            None => ctx.contained.push(entity),
        }
    }

    /// IfcRelAggregates from each whole to its parts.
    fn aggregates(&mut self) {
        let mut by_whole: Vec<(u64, Vec<u64>)> = Vec::new();
        for (w, part) in std::mem::take(&mut self.parts) {
            let Some(&whole) = self.written.get(&w) else {
                continue; // the whole was skipped (warned when written)
            };
            match by_whole.iter_mut().find(|(id, _)| *id == whole) {
                Some((_, ps)) => ps.push(part),
                None => by_whole.push((whole, vec![part])),
            }
        }
        for (whole, parts) in by_whole {
            self.rel(
                "IFCRELAGGREGATES",
                &format!("#{},({})", whole, refs(&parts)),
            );
        }
    }

    /// `connects:` relations. A fastener or accessory with two or more
    /// targets realizes the connection of the first with each other one;
    /// anything else is connected to each target.
    fn connections(&mut self) {
        // Pairs already connected without a realizing element, either way round.
        let mut plain = HashSet::new();
        for (id, entity, realizes, targets) in std::mem::take(&mut self.connects) {
            let mut found = Vec::new();
            for t in &targets {
                match self.written.get(t) {
                    Some(&e) if e != entity => {
                        if !found.contains(&e) {
                            found.push(e);
                        }
                    }
                    _ => self.warn(&id, format!("`connects:` names no other element `{t}`")),
                }
            }
            if realizes && found.len() >= 2 {
                for other in &found[1..] {
                    self.rel(
                        "IFCRELCONNECTSWITHREALIZINGELEMENTS",
                        &format!("$,#{},#{other},(#{entity}),$", found[0]),
                    );
                }
            } else {
                for other in found {
                    if plain.insert((entity.min(other), entity.max(other))) {
                        self.rel("IFCRELCONNECTSELEMENTS", &format!("$,#{entity},#{other}"));
                    }
                }
            }
        }
    }

    /// IfcRelFillsElement for windows and doors written outside their host, e.g.
    /// a door of an upper storey in a multi-storey wall.
    fn fills(&mut self) {
        for (o, filler) in std::mem::take(&mut self.fills) {
            match self.openings.get(&o) {
                Some(&opening) => {
                    self.rel("IFCRELFILLSELEMENT", &format!("#{},#{}", opening, filler));
                }
                None => self.warn(&o, "`opening:` names no written opening"),
            }
        }
    }

    /// Associate each queued `material:` with its elements. A layered material
    /// becomes an IfcMaterialLayerSet of its layers' IfcMaterials, referenced
    /// through an IfcMaterialLayerSetUsage where the element's body has one.
    fn materials(&mut self) {
        for (name, usage, elems) in std::mem::take(&mut self.materials) {
            let set = self.material_or_layer_set(&name);
            let target = match usage {
                Some(u) => self
                    .w
                    .add(&format!("IFCMATERIALLAYERSETUSAGE(#{},{})", set, u)),
                None => set,
            };
            self.rel(
                "IFCRELASSOCIATESMATERIAL",
                &format!("({}),#{}", refs(&elems), target),
            );
        }
    }

    /// The IfcMaterialLayerSet of a layered material, else its IfcMaterial.
    /// Both are written once per name, so layers shared between sets and
    /// materials also used on their own share one IfcMaterial. A layer naming
    /// a layered material (E306) becomes a plain IfcMaterial.
    fn material_or_layer_set(&mut self, name: &str) -> u64 {
        if let Some(&id) = self.layer_set_ids.get(name) {
            return id;
        }
        let model = self.model;
        let def = model.material(name);
        let Some(layers) = def.and_then(|d| d.layers()) else {
            return self.material(name);
        };
        let layers: Vec<String> = layers
            .iter()
            .map(|layer| {
                let mat = self.material(&layer.name);
                let id = self.w.add(&format!(
                    "IFCMATERIALLAYER(#{},{},$,{},$,$,$)",
                    mat,
                    sf(layer.thickness.max(0.0)),
                    step_str(material_ifc_name(model, &layer.name))
                ));
                format!("#{}", id)
            })
            .collect();
        let id = self.w.add(&format!(
            "IFCMATERIALLAYERSET(({}),{},$)",
            layers.join(","),
            step_str(material_ifc_name(model, name))
        ));
        self.layer_set_ids.insert(name.to_string(), id);
        id
    }

    /// One IfcMaterial with its preview look as surface style.
    fn material(&mut self, name: &str) -> u64 {
        if let Some(&id) = self.material_ids.get(name) {
            return id;
        }
        let def = self.model.material(name);
        let ifc_name = material_ifc_name(self.model, name).to_string();
        let category = match def.map(|m| &m.material_type) {
            Some(MaterialType::Homogeneous {
                category: Some(c), ..
            }) => step_str(c),
            _ => "$".to_string(),
        };
        let mat = self.w.add(&format!(
            "IFCMATERIAL({},$,{})",
            step_str(&ifc_name),
            category
        ));
        let look = match def {
            Some(d) => appearance::of_material(d),
            None => appearance::guess(name),
        };
        if let Some(look) = look {
            self.material_style(mat, ifc_name, &look);
        }
        self.material_ids.insert(name.to_string(), mat);
        mat
    }

    /// The preview look of a material as its IfcSurfaceStyle, attached through
    /// IfcMaterialDefinitionRepresentation so other viewers show the same colour.
    fn material_style(&mut self, material: u64, name: String, look: &SceneMaterial) {
        let [r, g, b, a] = look.color.map(f64::from);
        let colour = self
            .w
            .add(&format!("IFCCOLOURRGB($,{},{},{})", sf(r), sf(g), sf(b)));
        let rendering = self.w.add(&format!(
            "IFCSURFACESTYLERENDERING(#{},{},$,$,$,$,IFCNORMALISEDRATIOMEASURE({}),IFCSPECULAREXPONENT({}),.NOTDEFINED.)",
            colour,
            sf(1.0 - a),
            sf(look.specular.into()),
            sf(look.shininess.into())
        ));
        let style = self.w.add(&format!(
            "IFCSURFACESTYLE({},.BOTH.,(#{}))",
            step_str(&name),
            rendering
        ));
        let item = self.w.add(&format!("IFCSTYLEDITEM($,(#{}),$)", style));
        let rep = self.w.add(&format!(
            "IFCSTYLEDREPRESENTATION(#{},$,$,(#{}))",
            self.body, item
        ));
        self.w.add(&format!(
            "IFCMATERIALDEFINITIONREPRESENTATION($,$,(#{}),#{})",
            rep, material
        ));
    }

    /// Element geometry. `None` (with a warning) when it cannot be built.
    fn shape(
        &mut self,
        elem: &ResolvedElement,
        ctx: &StoreyCtx,
    ) -> Result<Option<Shape>, CodegenError> {
        let p = &elem.properties;
        let id = &elem.id;
        let missing = |key: &'static str| CodegenError::MissingElementProperty(id.clone(), key);
        let base = number_prop(p, "z_offset").unwrap_or(0.0);
        let rotation = number_prop(p, "rotation").unwrap_or(0.0).to_radians();
        let at = |x: f64, y: f64, angle: f64| Frame::upright([x, y, base], angle);
        let vertical = |profile, depth| vec![Solid::vertical(profile, 0.0, depth)];
        let et = elem.element_type;

        // Explicit prism body: plan boundary extruded upward (types whose own
        // geometry is a boundary handle it below).
        let native_boundary = registry::get_descriptor(et).has_native_boundary();
        if !native_boundary && let Some((outer, holes)) = polygon_prop(p, "boundary") {
            let height = number_prop(p, "height").ok_or_else(|| missing("height"))?;
            // Walls keep their axis frame so children can be placed along it.
            let frame = match axis_prop(p) {
                Some(a) if et == ElementType::Wall => at(a.from.0, a.from.1, a.angle()),
                _ => at(0.0, 0.0, 0.0),
            };
            let profile = region_profile(elem.curve("boundary"), outer, holes, to_local(&frame));
            return Ok(Some(Shape {
                frame,
                solids: vertical(profile, height),
            }));
        }

        if let Some(shape) = path_sweep(elem) {
            return Ok(Some(shape));
        }

        // An upright plate (`from:` axis) has the body of a wall.
        let body = match et {
            ElementType::Plate if axis_prop(p).is_some() => ElementType::Wall,
            _ => et,
        };
        let shape = match body {
            ElementType::Wall => {
                let axis = WallAxis::of(elem, default_curve_tolerance())
                    .ok_or_else(|| CodegenError::MissingWallProperty(id.clone(), "from"))?;
                let height = number_prop(p, "height")
                    .ok_or_else(|| CodegenError::MissingWallProperty(id.clone(), "height"))?;
                let t = number_prop(p, "thickness").unwrap_or(0.2);
                let ((x, y), angle) = axis.frame(0.0);
                let frame = at(x, y, angle);
                let exact = match elem.curve("path") {
                    Some(ElementCurve::Path2(_)) if !axis.is_straight() => axis.faces(t).ok(),
                    _ => None,
                };
                let profile = if let Some(faces) = exact {
                    Profile::Curve {
                        outer: faces.map(to_local(&frame)),
                        holes: vec![],
                    }
                } else if !axis.is_straight() {
                    Profile::Poly {
                        outer: geometry::wall_outline(&axis, t)
                            .into_iter()
                            .map(|q| frame.plan_to_local(q, base))
                            .collect(),
                        holes: vec![],
                    }
                } else {
                    let len = axis.length();
                    let h = t / 2.0;
                    match ctx.wall_ends.get(id) {
                        Some(&[sr, sl, er, el]) => Profile::Poly {
                            outer: vec![(sr, -h), (er, -h), (el, h), (sl, h)],
                            holes: vec![],
                        },
                        None => Profile::Rect {
                            cx: len / 2.0,
                            cy: 0.0,
                            w: len,
                            d: t,
                        },
                    }
                };
                Shape {
                    frame,
                    solids: vertical(profile, height),
                }
            }
            ElementType::Slab | ElementType::Plate => {
                let profile = outline_profile(elem)
                    .ok_or_else(|| CodegenError::MissingSlabProperty(id.clone(), "boundary"))?;
                let t = number_prop(p, "thickness").unwrap_or(0.2);
                Shape {
                    frame: at(0.0, 0.0, 0.0),
                    solids: vertical(profile, -t),
                }
            }
            ElementType::Footing => {
                let profile = outline_profile(elem).ok_or_else(|| missing("boundary"))?;
                let depth = number_prop(p, "depth").ok_or_else(|| missing("depth"))?;
                Shape {
                    frame: at(0.0, 0.0, 0.0),
                    solids: vertical(profile, -depth),
                }
            }
            ElementType::Covering => {
                let profile = outline_profile(elem).ok_or_else(|| missing("boundary"))?;
                let t = number_prop(p, "thickness").unwrap_or(geometry::COVERING_THICKNESS);
                Shape {
                    frame: at(0.0, 0.0, 0.0),
                    solids: vertical(profile, t),
                }
            }
            ElementType::Roof => {
                let (outer, holes) = outline_prop(p).ok_or_else(|| missing("boundary"))?;
                let base = number_prop(p, "z_offset").unwrap_or(ctx.roof_base);
                let roof_type = string_or_ident(p, "type").unwrap_or("flat");
                let solid = if roof_type == "flat" {
                    let t = number_prop(p, "thickness").unwrap_or(0.15);
                    let profile = region_profile(elem.curve("boundary"), outer, holes, |q| q);
                    Solid::vertical(profile, 0.0, t)
                } else {
                    pitched_roof(
                        &outer,
                        number_prop(p, "ridge_height").unwrap_or(1.0),
                        string_or_ident(p, "ridge_direction").unwrap_or("x"),
                        roof_type,
                    )
                };
                Shape {
                    frame: Frame::upright([0.0, 0.0, base], 0.0),
                    solids: vec![solid],
                }
            }
            ElementType::Column => {
                let (x, y) = point_prop(p, "position").ok_or_else(|| missing("position"))?;
                let height = number_prop(p, "height").ok_or_else(|| missing("height"))?;
                let profile = section(p, (0.3, 0.3));
                Shape {
                    frame: at(x, y, rotation),
                    solids: vertical(profile, height),
                }
            }
            ElementType::Beam | ElementType::Member => {
                let profile = section(p, member_section(et));
                if let Some(a) = axis_prop(p) {
                    let p0 = [a.from.0, a.from.1, base + a.z.0];
                    let p1 = [a.to.0, a.to.1, base + a.z.1];
                    let d = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
                    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    if len < 1e-9 {
                        self.warn(
                            id,
                            "member axis has zero length; skipping IFC geometry generation",
                        );
                        return Ok(None);
                    }
                    Shape {
                        frame: Frame::along(p0, d, rotation),
                        solids: vertical(profile, len),
                    }
                } else if let (Some((x, y)), Some(h)) =
                    (point_prop(p, "position"), number_prop(p, "height"))
                {
                    Shape {
                        frame: at(x, y, rotation),
                        solids: vertical(profile, h),
                    }
                } else {
                    self.warn(
                        id,
                        "member has no axis (from/to); cannot be faithfully round-tripped — skipping IFC geometry generation",
                    );
                    return Ok(None);
                }
            }
            ElementType::Stair => {
                let (x, y) = point_prop(p, "position").unwrap_or((0.0, 0.0));
                let flight = geometry::Flight::of(p, ctx.height);
                // Side profile in the local YZ plane, extruded across the width (local X).
                let position = Frame {
                    origin: [0.0; 3],
                    z: [1.0, 0.0, 0.0],
                    x: [0.0, 1.0, 0.0],
                };
                Shape {
                    frame: at(x, y, rotation),
                    solids: vec![Solid {
                        profile: Profile::Poly {
                            outer: flight.profile(),
                            holes: vec![],
                        },
                        position,
                        depth: flight.width,
                        revolve: None,
                        directrix: None,
                    }],
                }
            }
            ElementType::Ramp | ElementType::Railing => {
                let ramp = et == ElementType::Ramp;
                let across = if ramp {
                    number_prop(p, "width").unwrap_or(1.2)
                } else {
                    number_prop(p, "thickness").unwrap_or(0.05)
                };
                let height = if ramp {
                    0.2
                } else {
                    number_prop(p, "height").unwrap_or(1.0)
                };
                let (frame, len) = match axis_prop(p) {
                    Some(a) => (at(a.from.0, a.from.1, a.angle()), a.length()),
                    None => {
                        let (x, y) = point_prop(p, "position").unwrap_or((0.0, 0.0));
                        let len = number_prop(p, "length").unwrap_or(if ramp { 5.0 } else { 1.0 });
                        (at(x, y, rotation), len)
                    }
                };
                let profile = Profile::Rect {
                    cx: len / 2.0,
                    cy: 0.0,
                    w: len,
                    d: across,
                };
                Shape {
                    frame,
                    solids: vertical(profile, height),
                }
            }
            ElementType::Furniture | ElementType::Proxy => {
                let (x, y) = point_prop(p, "position")
                    .or((et == ElementType::Furniture).then_some((0.0, 0.0)))
                    .ok_or_else(|| missing("position"))?;
                let default_h = if et == ElementType::Furniture {
                    0.8
                } else {
                    1.0
                };
                let w = number_prop(p, "width").unwrap_or(1.0);
                let d = number_prop(p, "depth").unwrap_or(1.0);
                let h = number_prop(p, "height").unwrap_or(default_h);
                let profile = Profile::Rect {
                    cx: 0.0,
                    cy: 0.0,
                    w,
                    d,
                };
                Shape {
                    frame: at(x, y, rotation),
                    solids: vertical(profile, h),
                }
            }
            ElementType::Elevator => {
                let (x, y) = point_prop(p, "position").ok_or_else(|| missing("position"))?;
                let w = number_prop(p, "shaft_width").unwrap_or(1.8);
                let d = number_prop(p, "shaft_depth").unwrap_or(2.0);
                let h = elevator_height(p, ctx.height, ctx.storeys);
                let profile = Profile::Rect {
                    cx: w / 2.0,
                    cy: d / 2.0,
                    w,
                    d,
                };
                Shape {
                    frame: at(x, y, rotation),
                    solids: vertical(profile, h),
                }
            }
            // Children of hosts, written by `children`.
            ElementType::Window | ElementType::Door | ElementType::Opening => {
                self.warn(
                    id,
                    "window, door and opening must be nested in a host element; skipped",
                );
                return Ok(None);
            }
            ElementType::Duct
            | ElementType::Pipe
            | ElementType::CableTray
            | ElementType::Terminal
            | ElementType::Equipment
            | ElementType::Accessory => match mep::shape(elem) {
                Some(shape) => shape,
                None => {
                    self.warn(id, "missing or degenerate geometry; skipped");
                    return Ok(None);
                }
            },
            ElementType::Grid => unreachable!("grids are written by `grid`"),
            ElementType::System => unreachable!("systems are written by `systems`"),
        };
        Ok(Some(shape))
    }

    /// Openings, windows and doors of a host. Parametric children are placed in the
    /// host frame (`offset` along the axis, `sill_height` up, `lateral_offset` across);
    /// children with a `boundary` are prisms in storey coordinates.
    fn children(
        &mut self,
        host: &ResolvedElement,
        host_id: u64,
        frame: &Frame,
        ctx: &mut StoreyCtx,
    ) -> Result<(), CodegenError> {
        let thickness = number_prop(&host.properties, "thickness").unwrap_or(0.2);
        let is_wall = host.element_type == ElementType::Wall;
        // Fillers of a curved wall sit on the axis tangent at their centre.
        let curved = is_wall
            .then(|| WallAxis::of(host, default_curve_tolerance()))
            .flatten()
            .filter(|a| !a.is_straight());
        let frame_of = |c: &ResolvedElement| match &curved {
            Some(axis) => {
                let ((x, y), angle) = filler_axis_frame(c, axis);
                Frame::upright([x, y, frame.origin[2]], angle)
            }
            None => *frame,
        };

        // Explicit openings first, so fillers can reference them.
        for child in host
            .children
            .iter()
            .filter(|c| c.element_type == ElementType::Opening)
        {
            let p = &child.properties;
            let shape = if no_body(child) {
                let id = self.bodiless(child, ctx);
                self.rel("IFCRELVOIDSELEMENT", &format!("#{},#{}", host_id, id));
                self.openings.insert(child.id.clone(), id);
                continue;
            } else if polygon_prop(p, "boundary").is_some() {
                self.shape(child, ctx)?
            } else if is_wall {
                let mut s = filler_shape(child, &frame_of(child), thickness, thickness);
                let depth = number_prop(p, "depth").unwrap_or(thickness);
                if let Some(axis) = &curved
                    && depth >= thickness
                {
                    bend_through(&mut s, axis, child, depth);
                }
                Some(s)
            } else if let Some((x, y)) = point_prop(p, "position") {
                let base = number_prop(p, "z_offset").unwrap_or(0.0);
                let w = number_prop(p, "width").unwrap_or(1.0);
                let d = number_prop(p, "depth").unwrap_or(w);
                let h = number_prop(p, "height").unwrap_or(1.0);
                Some(Shape {
                    frame: Frame::upright([x, y, base], 0.0),
                    solids: vec![Solid::vertical(
                        Profile::Rect {
                            cx: 0.0,
                            cy: 0.0,
                            w,
                            d,
                        },
                        0.0,
                        h,
                    )],
                })
            } else {
                self.warn(
                    &child.id,
                    "opening needs `boundary`, or `position` outside walls; skipped",
                );
                None
            };
            if let Some(shape) = shape {
                let id = self.shaped(child, &shape, ctx);
                self.rel("IFCRELVOIDSELEMENT", &format!("#{},#{}", host_id, id));
                self.openings.insert(child.id.clone(), id);
            }
        }

        for child in &host.children {
            let p = &child.properties;
            if !matches!(child.element_type, ElementType::Window | ElementType::Door) {
                continue;
            }
            if no_body(child) {
                let filler = self.bodiless(child, ctx);
                ctx.contained.push(filler);
                if let Some(o) = string_or_ident(p, "opening") {
                    self.fills.push((o.to_string(), filler));
                }
                continue;
            }
            let has_boundary = polygon_prop(p, "boundary").is_some();
            if !is_wall && !has_boundary {
                self.warn(
                    &child.id,
                    "window or door outside a wall needs `boundary`; skipped",
                );
                continue;
            }
            let shape = if has_boundary {
                match self.shape(child, ctx)? {
                    Some(s) => s,
                    None => continue,
                }
            } else {
                let depth = number_prop(p, "depth").unwrap_or(0.05);
                filler_shape(child, &frame_of(child), depth, thickness)
            };
            let filler = self.shaped(child, &shape, ctx);
            ctx.contained.push(filler);

            let named = string_or_ident(p, "opening");
            // An opening written later (on another storey) is filled in the final `fills` pass
            if let Some(r) = named
                && !self.openings.contains_key(r)
                && self.ids.contains(r)
            {
                self.fills.push((r.to_string(), filler));
                continue;
            }
            let opening = match named.and_then(|r| self.openings.get(r)) {
                Some(&o) => o,
                None => {
                    // Generated opening: the filler's footprint through the whole host, or
                    // its own prism when it has a `boundary` (no host depth to extend to).
                    let shape = if has_boundary {
                        shape
                    } else {
                        through_opening(child, curved.as_ref(), &frame_of(child), thickness)
                    };
                    let pl = self.w.local_placement(Some(ctx.placement), &shape.frame);
                    let body = self.w.body(self.body, &shape.solids);
                    let props: Vec<ResolvedProperty> = string_prop(p, "opening_guid")
                        .map(|g| ResolvedProperty {
                            key: "guid".to_string(),
                            value: ResolvedValue::String(g.to_string()),
                            span: child.span,
                        })
                        .into_iter()
                        .collect();
                    let name = format!("Opening_{}", child.id);
                    let o = self.product(
                        "IFCOPENINGELEMENT",
                        &name,
                        &props,
                        None,
                        pl,
                        Some(body),
                        ",$",
                    );
                    self.rel("IFCRELVOIDSELEMENT", &format!("#{},#{}", host_id, o));
                    o
                }
            };
            self.rel("IFCRELFILLSELEMENT", &format!("#{},#{}", opening, filler));
        }
        Ok(())
    }

    /// Furniture placed from an asset: one IfcFurnishingElement per part.
    fn furniture_asset(&mut self, elem: &ResolvedElement, ctx: &mut StoreyCtx) {
        let p = &elem.properties;
        let asset_id = string_or_ident(p, "asset").unwrap_or_default();
        let Some(asset) = find_asset(&self.model.asset_libraries, asset_id) else {
            self.warn(
                &elem.id,
                format!("asset `{}` not found; furniture skipped", asset_id),
            );
            return;
        };
        let (x, y) = point_prop(p, "position").unwrap_or((0.0, 0.0));
        let base = number_prop(p, "z_offset").unwrap_or(0.0);
        let rotation = number_prop(p, "rotation").unwrap_or(0.0).to_radians();
        let frame = Frame::upright([x, y, base], rotation);
        for part in &asset.parts {
            let name = format!("{}_{}", elem.id, part.name);
            let (px, py, pz) = part.position;
            let solid = match &part.geometry {
                Some(model::GeometryPrimitive::Box(w, d, h)) => Solid::vertical(
                    Profile::Rect {
                        cx: px + w / 2.0,
                        cy: py + d / 2.0,
                        w: *w,
                        d: *d,
                    },
                    pz,
                    *h,
                ),
                Some(model::GeometryPrimitive::Cylinder(r, h)) => Solid::vertical(
                    Profile::Circle {
                        cx: px + r,
                        cy: py + r,
                        r: *r,
                    },
                    pz,
                    *h,
                ),
                Some(model::GeometryPrimitive::Extrude(pts, h)) => Solid::vertical(
                    Profile::Poly {
                        outer: pts.iter().map(|(a, b)| (px + a, py + b)).collect(),
                        holes: vec![],
                    },
                    pz,
                    *h,
                ),
                Some(model::GeometryPrimitive::Import(path)) => {
                    self.warn(
                        &name,
                        format!("external geometry import `{}` not supported in IFC output; part skipped", path),
                    );
                    continue;
                }
                None => {
                    self.warn(&name, "asset part has no geometry; skipped in IFC output");
                    continue;
                }
            };
            let pl = self.w.local_placement(Some(ctx.placement), &frame);
            let body = self.w.body(self.body, &[solid]);
            let id = self.product("IFCFURNISHINGELEMENT", &name, &[], None, pl, Some(body), "");
            // The part's material, else the furniture's, as the BML preview draws it.
            if let Some(m) = part.material.as_deref().or(string_or_ident(p, "material")) {
                self.associate(id, m, None);
            }
            ctx.contained.push(id);
        }
    }

    /// IfcGrid with axes from `x_count`/`x_spacing` and `y_count`/`y_spacing`,
    /// and `shape` as its body.
    fn grid(&mut self, elem: &ResolvedElement, ctx: &StoreyCtx, shape: Option<&Shape>) -> u64 {
        let p = &elem.properties;
        let (x, y) = point_prop(p, "position").unwrap_or((0.0, 0.0));
        let base = number_prop(p, "z_offset").unwrap_or(0.0);
        let x_spacing = number_prop(p, "x_spacing").unwrap_or(1.0);
        let y_spacing = number_prop(p, "y_spacing").unwrap_or(1.0);
        let x_count = number_prop(p, "x_count").unwrap_or(2.0) as usize;
        let y_count = number_prop(p, "y_count").unwrap_or(2.0) as usize;
        let x_total = x_count.saturating_sub(1) as f64 * x_spacing;
        let y_total = y_count.saturating_sub(1) as f64 * y_spacing;

        // A body prism is placed at the storey origin, so the axes carry `position`.
        let (ox, oy) = if shape.is_some() { (x, y) } else { (0.0, 0.0) };
        let mut u_axes = Vec::new();
        for i in 0..x_count {
            let ax = ox + i as f64 * x_spacing;
            let line = self.w.polyline2((ax, oy), (ax, oy + y_total));
            let label = ((b'A' + (i % 26) as u8) as char).to_string();
            u_axes.push(
                self.w
                    .add(&format!("IFCGRIDAXIS('{}',#{},.T.)", label, line)),
            );
        }
        let mut v_axes = Vec::new();
        for i in 0..y_count {
            let ay = oy + i as f64 * y_spacing;
            let line = self.w.polyline2((ox, ay), (ox + x_total, ay));
            v_axes.push(
                self.w
                    .add(&format!("IFCGRIDAXIS('{}',#{},.T.)", i + 1, line)),
            );
        }
        let frame = shape.map_or(Frame::upright([x, y, base], 0.0), |s| s.frame);
        let pl = self.w.local_placement(Some(ctx.placement), &frame);
        let body = shape.map_or("$".to_string(), |s| {
            format!("#{}", self.w.body(self.body, &s.solids))
        });
        let guid = self.guid(p);
        let name = string_prop(p, "ifc_name").unwrap_or(&elem.id);
        self.w.add(&format!(
            "IFCGRID('{}',$,{},$,$,#{},{},({}),({}),$,{})",
            guid,
            step_str(name),
            pl,
            body,
            refs(&u_axes),
            refs(&v_axes),
            predefined_type(p).unwrap_or_else(|| "$".into())
        ))
    }

    /// Property sets mapped from BML properties (see `pset_mapping`).
    fn psets(&mut self, elem: &ResolvedElement, element: u64, class: &str) {
        let mut sets: Vec<(&'static str, Vec<u64>)> = Vec::new();
        let curtain = class == "IFCCURTAINWALL";
        for prop in &elem.properties {
            let Some((mut pset, name, value)) = pset_mapping::map_property(elem.element_type, prop)
            else {
                continue;
            };
            if curtain {
                // Pset_CurtainWallCommon has no LoadBearing.
                if name == "LoadBearing" {
                    continue;
                }
                pset = "Pset_CurtainWallCommon";
            }
            let v = self.w.add(&format!(
                "IFCPROPERTYSINGLEVALUE('{}',$,{},$)",
                name,
                ifc_value_to_step_string(&value)
            ));
            match sets.iter_mut().find(|(n, _)| *n == pset) {
                Some((_, ids)) => ids.push(v),
                None => sets.push((pset, vec![v])),
            }
        }
        for (pset, ids) in sets {
            let guid = self.guid(&[]);
            let set = self.w.add(&format!(
                "IFCPROPERTYSET('{}',$,'{}',$,({}))",
                guid,
                pset,
                refs(&ids)
            ));
            self.rel(
                "IFCRELDEFINESBYPROPERTIES",
                &format!("(#{}),#{}", element, set),
            );
        }
    }
}

/// `(offset, width, sill_height, height)` of a window, door or opening in its wall.
pub(crate) fn filler_extent(elem: &ResolvedElement) -> (f64, f64, f64, f64) {
    let p = &elem.properties;
    let door = elem.element_type == ElementType::Door;
    let window = elem.element_type == ElementType::Window;
    (
        number_prop(p, "offset").unwrap_or(0.0),
        number_prop(p, "width").unwrap_or(if door { 0.9 } else { 1.0 }),
        number_prop(p, "sill_height").unwrap_or(if window { 0.9 } else { 0.0 }),
        number_prop(p, "height").unwrap_or(if door { 2.1 } else { 1.0 }),
    )
}

/// Plan origin and angle of `axis` at the centre of a window, door or opening.
pub(crate) fn filler_axis_frame(elem: &ResolvedElement, axis: &WallAxis) -> ((f64, f64), f64) {
    let (offset, width, ..) = filler_extent(elem);
    axis.frame(offset + width / 2.0)
}

/// Window, door or opening in a wall frame: `offset`..`offset + width` along the axis,
/// `sill_height` up (windows default 0.9 m), `depth` across, centred on `lateral_offset`.
fn filler_shape(elem: &ResolvedElement, frame: &Frame, depth: f64, wall_thickness: f64) -> Shape {
    let p = &elem.properties;
    let (offset, width, sill, height) = filler_extent(elem);
    let lateral = number_prop(p, "lateral_offset").unwrap_or(0.0);
    let depth = if elem.element_type == ElementType::Opening {
        number_prop(p, "depth").unwrap_or(wall_thickness)
    } else {
        depth
    };
    Shape {
        frame: *frame,
        solids: vec![Solid::vertical(
            Profile::Rect {
                cx: offset + width / 2.0,
                cy: lateral,
                w: width,
                d: depth,
            },
            sill,
            height,
        )],
    }
}

/// Generated opening of a window or door: its footprint through the whole wall.
fn through_opening(
    elem: &ResolvedElement,
    curved: Option<&WallAxis>,
    frame: &Frame,
    thickness: f64,
) -> Shape {
    let mut s = filler_shape(elem, frame, thickness, thickness);
    if let Some(axis) = curved {
        bend_through(&mut s, axis, elem, thickness);
    } else if let Profile::Rect { cy, .. } = &mut s.solids[0].profile {
        *cy = 0.0;
    }
    s
}

/// Replaces the straight box of an opening through a curved wall with the wall
/// outline over its stretch of axis, a little deeper than the wall, so the curved
/// faces leave no sliver.
fn bend_through(shape: &mut Shape, axis: &WallAxis, elem: &ResolvedElement, depth: f64) {
    const MARGIN: f64 = 0.01;
    let (offset, width, ..) = filler_extent(elem);
    let frame = shape.frame;
    shape.solids[0].profile = Profile::Poly {
        outer: geometry::wall_outline(&axis.sub(offset, offset + width), depth + 2.0 * MARGIN)
            .into_iter()
            .map(|q| frame.plan_to_local(q, frame.origin[2]))
            .collect(),
        holes: vec![],
    };
}

/// Pitched roof: triangular (shed: right-angled) cross-section extruded along the ridge.
fn pitched_roof(
    boundary: &[(f64, f64)],
    ridge_height: f64,
    ridge_direction: &str,
    roof_type: &str,
) -> Solid {
    let min_x = boundary.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let max_x = boundary
        .iter()
        .map(|p| p.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = boundary.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let max_y = boundary
        .iter()
        .map(|p| p.1)
        .fold(f64::NEG_INFINITY, f64::max);
    // Local Z runs along the ridge, local X across it, local Y = Z x X points up.
    let (cross_width, ridge_length, position) = if ridge_direction == "y" {
        (
            max_x - min_x,
            max_y - min_y,
            Frame {
                origin: [(min_x + max_x) / 2.0, min_y, 0.0],
                z: [0.0, 1.0, 0.0],
                x: [-1.0, 0.0, 0.0],
            },
        )
    } else {
        (
            max_y - min_y,
            max_x - min_x,
            Frame {
                origin: [min_x, (min_y + max_y) / 2.0, 0.0],
                z: [1.0, 0.0, 0.0],
                x: [0.0, 1.0, 0.0],
            },
        )
    };
    let half = cross_width / 2.0;
    let apex = if roof_type == "shed" {
        (half, ridge_height)
    } else {
        (0.0, ridge_height)
    };
    Solid {
        profile: Profile::Poly {
            outer: vec![(-half, 0.0), (half, 0.0), apex],
            holes: vec![],
        },
        position,
        depth: ridge_length,
        revolve: None,
        directrix: None,
    }
}

/// IFC class written for an element type without an `ifc_class:` override.
pub(crate) fn default_ifc_class(et: ElementType) -> String {
    registry::get_descriptor(et).ifc_entity.to_ascii_uppercase()
}

/// IfcConnectionTypeEnum value of a wall join end.
fn connection_type(end: JoinEnd) -> &'static str {
    match end {
        JoinEnd::Start => ".ATSTART.",
        JoinEnd::End => ".ATEND.",
        JoinEnd::Path => ".ATPATH.",
    }
}

/// `representation: none`: the element has no body.
pub(crate) fn no_body(elem: &ResolvedElement) -> bool {
    string_or_ident(&elem.properties, "representation") == Some("none")
}

/// IfcMaterialLayerSetUsage attributes after the layer set for an element's
/// body, `None` when only the set itself can be associated. The first layer is
/// on the right of a wall axis (the exterior of a counter-clockwise outline)
/// and on top of a slab, plate or flat roof.
fn layer_usage(elem: &ResolvedElement) -> Option<String> {
    let p = &elem.properties;
    match elem.element_type {
        ElementType::Wall | ElementType::Plate
            if axis_prop(p).is_some() && polygon_prop(p, "boundary").is_none() =>
        {
            let t = number_prop(p, "thickness").unwrap_or(0.2);
            Some(format!(".AXIS2.,.POSITIVE.,{},$", sf(-t / 2.0)))
        }
        ElementType::Slab | ElementType::Plate => Some(".AXIS3.,.NEGATIVE.,0.,$".to_string()),
        ElementType::Roof if string_or_ident(p, "type").unwrap_or("flat") == "flat" => {
            let t = number_prop(p, "thickness").unwrap_or(0.15);
            Some(format!(".AXIS3.,.NEGATIVE.,{},$", sf(t)))
        }
        ElementType::Covering => {
            let t = number_prop(p, "thickness").unwrap_or(geometry::COVERING_THICKNESS);
            Some(format!(".AXIS3.,.NEGATIVE.,{},$", sf(t)))
        }
        _ => None,
    }
}

/// `ifc_name` of material `name`, else the name itself.
fn material_ifc_name<'a>(model: &'a SemanticModel, name: &'a str) -> &'a str {
    model.material(name).map_or(name, |m| m.ifc_name())
}

/// Classes whose `connects:` realize a connection between other elements.
const REALIZING_CLASSES: &[&str] = &[
    "IFCDISCRETEACCESSORY",
    "IFCMECHANICALFASTENER",
    "IFCFASTENER",
];

/// IFC class, ObjectType and the attributes after Tag. `ifc_class:` overrides the
/// default class, `predefined_type:` the PredefinedType (else a roof's or stair's
/// `type`, else `$`).
fn class_and_tail(elem: &ResolvedElement) -> (String, Option<String>, String) {
    let p = &elem.properties;
    let services = mep::class(elem);
    let default =
        services.map_or_else(|| default_ifc_class(elem.element_type), |c| c.0.to_string());
    let class = string_prop(p, "ifc_class").map_or(default, |c| c.to_ascii_uppercase());
    let base = class.strip_suffix("STANDARDCASE").unwrap_or(&class);
    let (count, slot) = predefined_type_slot(&class);
    let mut attrs = vec!["$".to_string(); count];
    let opt = |key| number_prop(p, key).map_or("$".to_string(), sf);
    if base == "IFCWINDOW" || base == "IFCDOOR" {
        attrs[0] = opt("height");
        attrs[1] = opt("width");
    }
    if base == "IFCSTAIRFLIGHT"
        && let Some(n) = number_prop(p, "riser_count")
    {
        let n = format!("{}", n.round() as i64);
        attrs[..4].clone_from_slice(&[n.clone(), n, opt("riser_height"), opt("tread_depth")]);
    }
    let services = services.filter(|c| c.0 == class);
    let own = predefined_type(p);
    let object_type = (own.is_none() && services.is_some())
        .then(|| mep::object_type(elem))
        .flatten();
    let predefined = own.or_else(|| match base {
        "IFCROOF" => Some(roof_predefined_type(p).to_string()),
        "IFCSTAIR" => stair_predefined_type(p).map(str::to_string),
        _ => services.map(|c| format!(".{}.", c.1)),
    });
    if let (Some(i), Some(t)) = (slot, predefined) {
        attrs[i] = t;
    }
    let tail = attrs.iter().map(|a| format!(",{}", a)).collect();
    (class, object_type, tail)
}

/// Number of attributes after Tag and the index of PredefinedType among them
/// (IFC4). Classes not listed have PredefinedType as their only one.
pub(crate) fn predefined_type_slot(class: &str) -> (usize, Option<usize>) {
    match class.strip_suffix("STANDARDCASE").unwrap_or(class) {
        "IFCWINDOW" | "IFCDOOR" => (5, Some(2)),
        "IFCSTAIRFLIGHT" => (5, Some(4)),
        "IFCELEMENTASSEMBLY" => (2, Some(1)),
        "IFCPILE" => (2, Some(0)),
        "IFCMECHANICALFASTENER" => (3, Some(2)),
        "IFCREINFORCINGBAR" => (6, Some(4)),
        "IFCREINFORCINGMESH" => (10, Some(9)),
        "IFCTENDON" => (9, Some(1)),
        "IFCTENDONANCHOR" | "IFCTENDONDUCT" => (2, Some(1)),
        // IFC4 classes without PredefinedType
        c if c == "IFCFURNISHINGELEMENT"
            || c == "IFCELEMENT"
            || (c.starts_with("IFCFLOW") && c != "IFCFLOWMETER" && c != "IFCFLOWINSTRUMENT")
            || c == "IFCENERGYCONVERSIONDEVICE"
            || c == "IFCDISTRIBUTIONELEMENT"
            || c == "IFCDISTRIBUTIONFLOWELEMENT"
            || c == "IFCDISTRIBUTIONCONTROLELEMENT"
            || c == "IFCCIVILELEMENT"
            || c == "IFCVIRTUALELEMENT" =>
        {
            (0, None)
        }
        _ => (1, Some(0)),
    }
}

/// `predefined_type:` as an IFC enumeration value.
fn predefined_type(p: &[ResolvedProperty]) -> Option<String> {
    string_or_ident(p, "predefined_type").map(|t| format!(".{}.", t.to_ascii_uppercase()))
}

/// IfcStairTypeEnum value of a stair's `type`. A stair is one straight
/// flight unless `semantic/stair.rs` expanded it; that sets `predefined_type:`.
fn stair_predefined_type(p: &[ResolvedProperty]) -> Option<&'static str> {
    Some(match string_or_ident(p, "type")? {
        "straight" | "l_shaped" | "u_shaped" => ".STRAIGHT_RUN_STAIR.",
        "spiral" => ".SPIRAL_STAIR.",
        _ => return None,
    })
}

/// IfcRoofTypeEnum value of a roof's `type`.
fn roof_predefined_type(p: &[ResolvedProperty]) -> &'static str {
    match string_or_ident(p, "type").unwrap_or("flat") {
        "flat" => ".FLAT_ROOF.",
        "gable" => ".GABLE_ROOF.",
        "hip" => ".HIP_ROOF.",
        "shed" => ".SHED_ROOF.",
        "mansard" => ".MANSARD_ROOF.",
        _ => ".NOTDEFINED.",
    }
}

/// Default section (width, depth) of a beam or member.
fn member_section(et: ElementType) -> (f64, f64) {
    if et == ElementType::Beam {
        (0.3, 0.5)
    } else {
        (0.1, 0.05)
    }
}

/// Body of a railing, beam or member on a `path:` at `z_offset`. A
/// rectangular section is built like a curved wall: the plan outline of the
/// path at the section width, extruded up by the railing height or over the
/// beam depth centred on `z_offset`. A round section is a swept disk along
/// the path. The path lies in plan (`path:` is 2D), so no sweep climbs.
pub(crate) fn path_sweep(elem: &ResolvedElement) -> Option<Shape> {
    let et = elem.element_type;
    let p = &elem.properties;
    if !matches!(
        et,
        ElementType::Railing | ElementType::Beam | ElementType::Member
    ) {
        return None;
    }
    let path = exact_path(elem).filter(|_| find(p, "path").is_some())?;
    let frame = Frame::upright([0.0, 0.0, number_prop(p, "z_offset").unwrap_or(0.0)], 0.0);
    let (width, z0, depth) = match et {
        ElementType::Railing => {
            let h = number_prop(p, "height").unwrap_or(1.0);
            (number_prop(p, "thickness").unwrap_or(0.05), 0.0, h)
        }
        _ => match section(p, member_section(et)) {
            Profile::Rect { w, d, .. } => (w, -d / 2.0, d),
            disk => {
                return Some(Shape {
                    frame,
                    solids: vec![Solid::swept(disk, path.map(|q| [q[0], q[1], 0.0]))],
                });
            }
        },
    };
    let axis = WallAxis::of(elem, default_curve_tolerance())?;
    let profile = match axis.faces(width) {
        Ok(outer) => Profile::Curve {
            outer,
            holes: vec![],
        },
        Err(_) => Profile::Poly {
            outer: geometry::wall_outline(&axis, width),
            holes: vec![],
        },
    };
    Some(Shape {
        frame,
        solids: vec![Solid::vertical(profile, z0, depth)],
    })
}

/// Exact plan axis of a linear element: its `path:` curve (lines for a plain
/// polygon) or its `from:`/`via:`/`to:` arc.
fn exact_path(elem: &ResolvedElement) -> Option<Curve2> {
    let line = |a: (f64, f64), b: (f64, f64)| Segment::Line([a.0, a.1], [b.0, b.1]);
    let segments = match (elem.curve("path"), find(&elem.properties, "path")) {
        (Some(ElementCurve::Path2(c)), _) => return Some(c.clone()),
        (_, Some(ResolvedValue::Polygon(pts))) if pts.len() >= 2 => {
            pts.windows(2).map(|w| line(w[0], w[1])).collect()
        }
        _ => match find(&elem.properties, "from")? {
            ResolvedValue::Axis {
                from,
                to,
                via: Some(via),
                ..
            } => vec![Segment::Arc {
                from: [from.0, from.1],
                via: [via.0, via.1],
                to: [to.0, to.1],
            }],
            ResolvedValue::Axis { from, to, .. } => vec![line(*from, *to)],
            _ => return None,
        },
    };
    Some(Curve {
        segments,
        closed: false,
    })
}

/// Column/beam cross-section: `profile: rectangle(w, d)` or `circle(r)`.
fn section(p: &[ResolvedProperty], default: (f64, f64)) -> Profile {
    let rect = |w, d| Profile::Rect {
        cx: 0.0,
        cy: 0.0,
        w,
        d,
    };
    match profile_prop(p, "profile") {
        Some(("rectangle", params)) if params.len() >= 2 => rect(params[0], params[1]),
        Some(("circle", params)) if !params.is_empty() => Profile::Circle {
            cx: 0.0,
            cy: 0.0,
            r: params[0],
        },
        _ => rect(default.0, default.1),
    }
}

/// Look up an asset by ID across all asset libraries.
fn find_asset<'a>(
    libraries: &'a [model::ResolvedAssetLibrary],
    id: &str,
) -> Option<&'a model::ResolvedAsset> {
    libraries
        .iter()
        .flat_map(|l| l.assets.iter())
        .find(|a| a.id == id)
}

/// Elevator shaft height from `from_storey` (bottom) to the top of `to_storey`,
/// else the storey height.
fn elevator_height(p: &[ResolvedProperty], storey_height: f64, all: &[ResolvedStorey]) -> f64 {
    let storey = |key| string_or_ident(p, key).and_then(|n| all.iter().find(|s| s.name == n));
    if let (Some(from), Some(to)) = (storey("from_storey"), storey("to_storey")) {
        let elev = |s: &ResolvedStorey| number_prop(&s.properties, "elevation").unwrap_or(0.0);
        let top = elev(to) + number_prop(&to.properties, "height").unwrap_or(3.0);
        return top - elev(from);
    }
    storey_height
}

/// Format an IfcValue as a STEP value expression.
fn ifc_value_to_step_string(value: &IfcValue) -> String {
    match value {
        IfcValue::Label(LabelValue(s)) => {
            // Access inner String directly — StringPrimitive::Display adds quotes.
            match s.0.as_str() {
                "TRUE" => "IFCBOOLEAN(.T.)".to_string(),
                "FALSE" => "IFCBOOLEAN(.F.)".to_string(),
                other => format!("IFCLABEL({})", step_str(other)),
            }
        }
        IfcValue::Real(RealValue(r)) => format!("IFCREAL({})", sf(r.clone().into())),
        IfcValue::ThermalTransmittance(t) => {
            format!("IFCTHERMALTRANSMITTANCEMEASURE({})", sf(t.0.clone().into()))
        }
        _ => "IFCLABEL('?')".to_string(),
    }
}

// --- Property extraction helpers ---

struct AxisProp {
    from: (f64, f64),
    to: (f64, f64),
    /// Heights of `from` and `to` (0 for 2D points).
    z: (f64, f64),
}

impl AxisProp {
    fn angle(&self) -> f64 {
        geometry::wall_angle(self.from, self.to)
    }
    fn length(&self) -> f64 {
        geometry::wall_length(self.from, self.to)
    }
}

fn find<'a>(props: &'a [ResolvedProperty], key: &str) -> Option<&'a ResolvedValue> {
    props.iter().find(|p| p.key == key).map(|p| &p.value)
}

fn axis_prop(props: &[ResolvedProperty]) -> Option<AxisProp> {
    match find(props, "from")? {
        ResolvedValue::Axis { from, to, z, .. } => Some(AxisProp {
            from: *from,
            to: *to,
            z: z.unwrap_or((0.0, 0.0)),
        }),
        _ => None,
    }
}

type PolygonWithHoles = (Vec<(f64, f64)>, Vec<Vec<(f64, f64)>>);

fn polygon_prop(props: &[ResolvedProperty], key: &str) -> Option<PolygonWithHoles> {
    match find(props, key)? {
        ResolvedValue::PolygonWithHoles { outer, holes } if outer.len() >= 3 => {
            Some((outer.clone(), holes.clone()))
        }
        ResolvedValue::Polygon(pts) if pts.len() >= 3 => Some((pts.clone(), vec![])),
        _ => None,
    }
}

/// Profile of a plan region, mapped by `f` (plan to profile coordinates): the
/// exact `boundary` curve when there is one, with holes from the curve, else
/// from `holes`; otherwise the `outer` / `holes` polygons.
fn region_profile(
    curve: Option<&ElementCurve>,
    outer: Vec<(f64, f64)>,
    holes: Vec<Vec<(f64, f64)>>,
    f: impl Fn([f64; 2]) -> [f64; 2] + Copy,
) -> Profile {
    let Some(ElementCurve::Region {
        outer: exact,
        holes: curves,
    }) = curve
    else {
        let poly = |pts: &[(f64, f64)]| {
            pts.iter()
                .map(|q| {
                    let l = f([q.0, q.1]);
                    (l[0], l[1])
                })
                .collect()
        };
        return Profile::Poly {
            outer: poly(&outer),
            holes: holes.iter().map(|h| poly(h)).collect(),
        };
    };
    let holes = if curves.is_empty() {
        holes.iter().map(|h| ring_curve(h)).collect()
    } else {
        curves.clone()
    };
    Profile::Curve {
        outer: exact.map(f),
        holes: holes.iter().map(|h| h.map(f)).collect(),
    }
}

/// Closed polygon as a curve of straight lines.
fn ring_curve(pts: &[(f64, f64)]) -> Curve2 {
    let mut pts: Vec<[f64; 2]> = pts.iter().map(|p| [p.0, p.1]).collect();
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    let n = pts.len();
    Curve {
        segments: (0..n)
            .map(|i| Segment::Line(pts[i], pts[(i + 1) % n]))
            .collect(),
        closed: true,
    }
}

/// Plan point to the local XY of `frame` (an upright element frame).
fn to_local(frame: &Frame) -> impl Fn([f64; 2]) -> [f64; 2] + Copy {
    let frame = *frame;
    move |q| {
        let l = frame.plan_to_local((q[0], q[1]), frame.origin[2]);
        [l.0, l.1]
    }
}

/// Exact axis in the element's frame, for the 'Axis' representation: the
/// path of a railing, beam or member on a `path:`, or the axis of a curved
/// wall.
fn axis_curve(elem: &ResolvedElement, frame: &Frame) -> Option<Curve2> {
    let wall = match elem.element_type {
        ElementType::Wall => true,
        ElementType::Railing | ElementType::Beam | ElementType::Member => false,
        _ => return None,
    };
    if polygon_prop(&elem.properties, "boundary").is_some()
        || (!wall && find(&elem.properties, "path").is_none())
    {
        return None;
    }
    exact_path(elem)
        .filter(|c| !wall || !c.is_straight())
        .map(|c| c.map(to_local(frame)))
}

/// Profile of a slab, plate, footing, covering or flat roof: the exact
/// `boundary` curve when there is one, else the polygon, with the holes the
/// scene graph cuts.
fn outline_profile(elem: &ResolvedElement) -> Option<Profile> {
    let (outer, holes) = outline_prop(&elem.properties)?;
    Some(region_profile(elem.curve("boundary"), outer, holes, |q| q))
}

/// `boundary` of a slab, plate or roof with the holes the scene graph cuts.
fn outline_prop(props: &[ResolvedProperty]) -> Option<PolygonWithHoles> {
    let (outer, _) = polygon_prop(props, "boundary")?;
    Some((outer, outline_holes(props)))
}

fn point_prop(props: &[ResolvedProperty], key: &str) -> Option<(f64, f64)> {
    match find(props, key)? {
        ResolvedValue::Point(x, y) => Some((*x, *y)),
        _ => None,
    }
}

fn profile_prop<'a>(props: &'a [ResolvedProperty], key: &str) -> Option<(&'a str, &'a [f64])> {
    match find(props, key)? {
        ResolvedValue::Profile {
            profile_type,
            params,
        } => Some((profile_type.as_str(), params.as_slice())),
        _ => None,
    }
}

fn string_prop<'a>(props: &'a [ResolvedProperty], key: &str) -> Option<&'a str> {
    match find(props, key)? {
        ResolvedValue::String(s) => Some(s.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cst_to_ast;
    use crate::semantic;

    #[test]
    fn opening_in_curved_wall_covers_the_wall() {
        let src = "# Site: S\n## Building: B\n### Storey: GF\n  elevation: 0 m\n  Wall: W\n    from: (0, 0) via: (5, 5) to: (10, 0)\n    height: 3 m\n    thickness: 0.3 m\n    Door: D\n      offset: 3 m\n      width: 1 m\n      height: 2.1 m\n";
        let (model, diags) = semantic::analyze(&cst_to_ast::parse(src).unwrap());
        assert!(diags.is_empty(), "{diags:?}");
        let wall = model.elements().find(|e| e.id == "W").unwrap();
        let door = &wall.children[0];
        let axis = WallAxis::of(wall, default_curve_tolerance()).unwrap();
        let ((x, y), angle) = axis.frame(3.5);
        let frame = Frame::upright([x, y, 0.0], angle);
        let shape = through_opening(door, Some(&axis), &frame, 0.3);
        let Profile::Poly { outer, .. } = &shape.solids[0].profile else {
            panic!("{:?}", shape.solids[0].profile);
        };
        // The wall over the door's stretch, shrunk a hair along the axis, lies inside.
        for q in geometry::wall_outline(&axis.sub(3.001, 3.999), 0.3) {
            let l = frame.plan_to_local(q, 0.0);
            assert!(geometry::point_in_polygon(l.0, l.1, outer), "{q:?}");
        }
    }
}
