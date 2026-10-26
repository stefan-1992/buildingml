use std::fmt::Write;

use crate::codegen::step_writer::{add, dot, normalize, sub};
use crate::codegen::{geometry, mep};
use crate::geometry::axis::WallAxis;
use crate::scene::graph::{get_number, get_point, get_polygon, get_profile, wall_outline};
use crate::semantic::model::{
    ElementType, ResolvedElement, ResolvedSpace, SemanticModel, contents,
};

/// Options for floorplan SVG rendering.
pub struct FloorplanOptions {
    /// Section cut elevation relative to storey floor (meters). Default 1.0.
    pub cut_height: f64,
    /// Pixels per meter. Default 100.0.
    pub scale: f64,
    /// Whether to show dimension annotations on walls.
    pub show_dimensions: bool,
    /// Whether to show a reference grid.
    pub show_grid: bool,
    /// SVG margin in pixels.
    pub margin: f64,
}

impl Default for FloorplanOptions {
    fn default() -> Self {
        Self {
            cut_height: 1.0,
            scale: 100.0,
            show_dimensions: true,
            show_grid: true,
            margin: 40.0,
        }
    }
}

/// Render an SVG floorplan for the plan at `storey_index` in [`storey_names`].
///
/// Returns the SVG string, or `None` if the index is out of range.
pub fn render_floorplan(
    model: &SemanticModel,
    storey_index: usize,
    options: &FloorplanOptions,
) -> Option<String> {
    let (storey_name, spaces, elements) = plans(model).nth(storey_index)?;

    // Collect all elements for this plan (direct + in spaces)
    let all_elements: Vec<&ResolvedElement> = contents(spaces, elements).collect();

    // Compute bounding box in world coordinates
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for elem in &all_elements {
        extend_bounds(elem, &mut min_x, &mut max_x, &mut min_y, &mut max_y);
    }

    if min_x > max_x {
        // No geometry
        min_x = 0.0;
        max_x = 10.0;
        min_y = 0.0;
        max_y = 10.0;
    }

    // Add padding
    let pad = 1.0; // 1m padding in world coords
    min_x -= pad;
    max_x += pad;
    min_y -= pad;
    max_y += pad;

    let scale = options.scale;
    let margin = options.margin;
    let width = (max_x - min_x) * scale + 2.0 * margin;
    let height = (max_y - min_y) * scale + 2.0 * margin;

    let mut svg = String::with_capacity(8192);

    // SVG header with metadata for LLM parsing
    write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
<title>Floorplan: {storey}</title>
<desc>BuildingML floorplan for storey "{storey}". Scale: {scale}px/m. Origin: ({ox:.2}, {oy:.2})m.</desc>
<defs>
  <style>
    .wall {{ fill: #d0ccc6; stroke: #2a2a2a; stroke-width: 1.5; }}
    .wall-cut {{ fill: #888; stroke: #1a1a1a; stroke-width: 2; }}
    .window {{ fill: none; stroke: #4488cc; stroke-width: 1.5; }}
    .window-glass {{ fill: #a8d4f0; fill-opacity: 0.3; stroke: #4488cc; stroke-width: 0.8; }}
    .door {{ fill: none; stroke: #8b5e3c; stroke-width: 1.2; }}
    .door-swing {{ fill: none; stroke: #8b5e3c; stroke-width: 0.6; stroke-dasharray: 3,2; }}
    .column {{ fill: #666; stroke: #333; stroke-width: 1; }}
    .slab {{ fill: #e8e6e2; fill-opacity: 0.3; stroke: #bbb; stroke-width: 0.5; stroke-dasharray: 4,2; }}
    .beam {{ fill: none; stroke: #999; stroke-width: 1; stroke-dasharray: 6,3; }}
    .dim-line {{ stroke: #666; stroke-width: 0.5; }}
    .dim-text {{ font-family: monospace; font-size: 9px; fill: #666; text-anchor: middle; }}
    .label {{ font-family: sans-serif; font-size: 11px; fill: #333; text-anchor: middle; }}
    .title {{ font-family: sans-serif; font-size: 14px; fill: #222; font-weight: bold; }}
    .grid {{ stroke: #e0e0e0; stroke-width: 0.3; }}
    .grid-major {{ stroke: #ccc; stroke-width: 0.5; }}
    .grid-label {{ font-family: monospace; font-size: 8px; fill: #aaa; }}
    .stair {{ fill: #e0ddd8; stroke: #999; stroke-width: 0.8; }}
    .furniture {{ fill: #c8a882; fill-opacity: 0.5; stroke: #a08060; stroke-width: 0.8; }}
    .proxy {{ fill: #c4b69a; fill-opacity: 0.4; stroke: #a09070; stroke-width: 0.8; stroke-dasharray: 3,2; }}
    .member {{ fill: none; stroke: #888; stroke-width: 1.2; stroke-dasharray: 4,2; }}
    .plate {{ fill: #b8bab4; fill-opacity: 0.3; stroke: #999; stroke-width: 0.8; }}
    .duct {{ fill: none; stroke: #5a8fb8; stroke-opacity: 0.5; stroke-linejoin: round; stroke-linecap: square; }}
    .pipe {{ fill: none; stroke: #3a7a4a; stroke-opacity: 0.6; stroke-linejoin: round; stroke-linecap: square; }}
    .cable-tray {{ fill: none; stroke: #c08a2a; stroke-opacity: 0.5; stroke-linejoin: round; stroke-linecap: square; }}
    .terminal, .equipment, .accessory {{ fill: #d8e4ee; stroke: #456; stroke-width: 0.8; }}
  </style>
</defs>
"#,
        w = width,
        h = height,
        storey = storey_name,
        scale = scale,
        ox = min_x,
        oy = min_y,
    )
    .unwrap();

    // Transform: flip Y (world Y up → SVG Y down) and apply offset + scale
    // SVG_x = (world_x - min_x) * scale + margin
    // SVG_y = (max_y - world_y) * scale + margin  (flip Y)
    let tx = |x: f64| -> f64 { (x - min_x) * scale + margin };
    let ty = |y: f64| -> f64 { (max_y - y) * scale + margin };

    // Background
    write!(
        svg,
        r#"<rect width="{w}" height="{h}" fill="white"/>"#,
        w = width,
        h = height
    )
    .unwrap();
    svg.push('\n');

    // Grid
    if options.show_grid {
        write_grid(&mut svg, min_x, max_x, min_y, max_y, &tx, &ty);
    }

    // Draw order: slabs (background) → walls → openings → columns → beams → labels

    // 1. Slabs
    svg.push_str("<g id=\"slabs\">\n");
    for elem in &all_elements {
        if elem.element_type == ElementType::Slab {
            draw_slab(&mut svg, elem, &tx, &ty);
        }
    }
    svg.push_str("</g>\n");

    // 2. Walls
    svg.push_str("<g id=\"walls\">\n");
    for elem in &all_elements {
        if elem.element_type == ElementType::Wall {
            draw_wall(&mut svg, elem, options, &tx, &ty);
        }
    }
    svg.push_str("</g>\n");

    // 3. Openings (windows + doors) — rendered on top of walls
    svg.push_str("<g id=\"openings\">\n");
    for elem in &all_elements {
        if elem.element_type == ElementType::Wall {
            for child in &elem.children {
                match child.element_type {
                    ElementType::Window => draw_window(&mut svg, child, elem, &tx, &ty),
                    ElementType::Door => draw_door(&mut svg, child, elem, &tx, &ty),
                    _ => {}
                }
            }
        }
    }
    svg.push_str("</g>\n");

    // 4. Columns
    svg.push_str("<g id=\"columns\">\n");
    for elem in &all_elements {
        if elem.element_type == ElementType::Column {
            draw_column(&mut svg, elem, &tx, &ty);
        }
    }
    svg.push_str("</g>\n");

    // 5. Beams (dashed overhead)
    svg.push_str("<g id=\"beams\">\n");
    for elem in &all_elements {
        if elem.element_type == ElementType::Beam {
            draw_beam(&mut svg, elem, &tx, &ty);
        }
    }
    svg.push_str("</g>\n");

    // 6. Stairs
    svg.push_str("<g id=\"stairs\">\n");
    for elem in &all_elements {
        if elem.element_type == ElementType::Stair {
            draw_stair(&mut svg, elem, &tx, &ty);
        }
    }
    svg.push_str("</g>\n");

    // 7. Furniture
    svg.push_str("<g id=\"furniture\">\n");
    for elem in &all_elements {
        if elem.element_type == ElementType::Furniture {
            draw_furniture(&mut svg, elem, &tx, &ty);
        }
    }
    svg.push_str("</g>\n");

    // 8. Proxies
    svg.push_str("<g id=\"proxies\">\n");
    for elem in &all_elements {
        if elem.element_type == ElementType::Proxy {
            draw_proxy(&mut svg, elem, &tx, &ty);
        }
    }
    svg.push_str("</g>\n");

    // 9. Members
    svg.push_str("<g id=\"members\">\n");
    for elem in &all_elements {
        if elem.element_type == ElementType::Member {
            draw_member(&mut svg, elem, &tx, &ty);
        }
    }
    svg.push_str("</g>\n");

    // 10. Plates
    svg.push_str("<g id=\"plates\">\n");
    for elem in &all_elements {
        if elem.element_type == ElementType::Plate {
            draw_plate(&mut svg, elem, &tx, &ty);
        }
    }
    svg.push_str("</g>\n");

    // 10b. Ducts, pipes, cable trays, terminals and equipment
    svg.push_str("<g id=\"mep\">\n");
    for elem in &all_elements {
        draw_mep(&mut svg, elem, &tx, &ty);
    }
    svg.push_str("</g>\n");

    // 11. Structural grids
    svg.push_str("<g id=\"structural-grids\">\n");
    for elem in &all_elements {
        if elem.element_type == ElementType::Grid {
            draw_structural_grid(&mut svg, elem, &tx, &ty);
        }
    }
    svg.push_str("</g>\n");

    // 12. Dimension annotations
    if options.show_dimensions {
        svg.push_str("<g id=\"dimensions\">\n");
        for elem in &all_elements {
            if elem.element_type == ElementType::Wall {
                draw_wall_dimension(&mut svg, elem, &tx, &ty);
            }
        }
        svg.push_str("</g>\n");
    }

    // 9. Space labels
    svg.push_str("<g id=\"labels\">\n");
    for space in spaces {
        draw_space_label(&mut svg, space, &all_elements, &tx, &ty);
    }
    svg.push_str("</g>\n");

    // Title block
    write!(
        svg,
        r#"<text x="{x}" y="{y}" class="title">{storey}</text>"#,
        x = margin,
        y = height - 10.0,
        storey = storey_name,
    )
    .unwrap();
    svg.push('\n');

    // Scale bar
    let bar_x = width - margin - 100.0;
    let bar_y = height - 15.0;
    let bar_len_m = (1.0_f64).max((max_x - min_x) / 10.0).floor();
    let bar_len_px = bar_len_m * scale;
    write!(
        svg,
        "<line x1=\"{x1}\" y1=\"{y}\" x2=\"{x2}\" y2=\"{y}\" stroke=\"#333\" stroke-width=\"2\"/>\n\
         <text x=\"{xm}\" y=\"{yt}\" class=\"dim-text\">{len}m</text>",
        x1 = bar_x,
        x2 = bar_x + bar_len_px,
        y = bar_y,
        xm = bar_x + bar_len_px / 2.0,
        yt = bar_y - 4.0,
        len = bar_len_m,
    )
    .unwrap();
    svg.push('\n');

    svg.push_str("</svg>\n");
    Some(svg)
}

/// Return the names of the plans [`render_floorplan`] draws: every storey, then
/// each building and site that holds elements or spaces outside a storey.
pub fn storey_names(model: &SemanticModel) -> Vec<String> {
    plans(model).map(|(name, ..)| name.to_string()).collect()
}

/// Plans as (name, spaces, elements), in [`SemanticModel::containers`] order;
/// buildings and sites only when they hold contents. Same rule as `getStoreys()`
/// in `web/src/shared/floorplan/model-importer.ts`.
fn plans(
    model: &SemanticModel,
) -> impl Iterator<Item = (&str, &[ResolvedSpace], &[ResolvedElement])> {
    model.containers().filter_map(|c| {
        let name = match (c.storey, c.building) {
            (Some(storey), _) => &storey.name,
            _ if c.spaces.is_empty() && c.elements.is_empty() => return None,
            (None, Some(building)) => &building.name,
            (None, None) => &c.site.name,
        };
        Some((name.as_str(), c.spaces, c.elements))
    })
}

// ---------------------------------------------------------------------------
// Bounds calculation
// ---------------------------------------------------------------------------

fn extend_bounds(
    elem: &ResolvedElement,
    min_x: &mut f64,
    max_x: &mut f64,
    min_y: &mut f64,
    max_y: &mut f64,
) {
    match elem.element_type {
        ElementType::Wall => {
            for (x, y) in wall_outline(elem).unwrap_or_default() {
                *min_x = min_x.min(x);
                *max_x = max_x.max(x);
                *min_y = min_y.min(y);
                *max_y = max_y.max(y);
            }
        }
        ElementType::Column => {
            if let Some((x, y)) = get_point(&elem.properties, "position") {
                let r = 0.3;
                *min_x = min_x.min(x - r);
                *max_x = max_x.max(x + r);
                *min_y = min_y.min(y - r);
                *max_y = max_y.max(y + r);
            }
        }
        ElementType::Slab => {
            if let Some(pts) = get_polygon(&elem.properties, "boundary") {
                for (x, y) in &pts {
                    *min_x = min_x.min(*x);
                    *max_x = max_x.max(*x);
                    *min_y = min_y.min(*y);
                    *max_y = max_y.max(*y);
                }
            }
        }
        ElementType::Beam => {
            for (x, y) in bar_outline(elem, 0.15).unwrap_or_default() {
                *min_x = min_x.min(x);
                *max_x = max_x.max(x);
                *min_y = min_y.min(y);
                *max_y = max_y.max(y);
            }
        }
        ElementType::Stair => {
            if get_point(&elem.properties, "position").is_some()
                && !crate::codegen::ifc_writer::no_body(elem)
            {
                let (flight, position, rotation) = stair_plan(elem);
                for (x, y) in geometry::placed(flight_outline(&flight), position, rotation) {
                    *min_x = min_x.min(x);
                    *max_x = max_x.max(x);
                    *min_y = min_y.min(y);
                    *max_y = max_y.max(y);
                }
            }
        }
        ElementType::Furniture | ElementType::Proxy => {
            if let Some((x, y)) = get_point(&elem.properties, "position") {
                let hw = get_number(&elem.properties, "width").unwrap_or(1.0) / 2.0;
                let hd = get_number(&elem.properties, "depth").unwrap_or(1.0) / 2.0;
                *min_x = min_x.min(x - hw);
                *max_x = max_x.max(x + hw);
                *min_y = min_y.min(y - hd);
                *max_y = max_y.max(y + hd);
            }
        }
        ElementType::Member => {
            for (x, y) in bar_outline(elem, 0.05).unwrap_or_default() {
                *min_x = min_x.min(x);
                *max_x = max_x.max(x);
                *min_y = min_y.min(y);
                *max_y = max_y.max(y);
            }
        }
        ElementType::Plate => {
            if let Some(pts) = get_polygon(&elem.properties, "boundary") {
                for (x, y) in &pts {
                    *min_x = min_x.min(*x);
                    *max_x = max_x.max(*x);
                    *min_y = min_y.min(*y);
                    *max_y = max_y.max(*y);
                }
            }
        }
        ElementType::Grid => {
            let (px, py) = get_point(&elem.properties, "position").unwrap_or((0.0, 0.0));
            let x_spacing = get_number(&elem.properties, "x_spacing").unwrap_or(1.0);
            let y_spacing = get_number(&elem.properties, "y_spacing").unwrap_or(1.0);
            let x_count = get_number(&elem.properties, "x_count").unwrap_or(2.0) as usize;
            let y_count = get_number(&elem.properties, "y_count").unwrap_or(2.0) as usize;
            let x_total = (x_count.saturating_sub(1)) as f64 * x_spacing;
            let y_total = (y_count.saturating_sub(1)) as f64 * y_spacing;
            *min_x = min_x.min(px);
            *max_x = max_x.max(px + x_total);
            *min_y = min_y.min(py);
            *max_y = max_y.max(py + y_total);
        }
        _ => {
            for (x, y) in mep_plan(elem).map(|(l, _)| l.concat()).unwrap_or_default() {
                *min_x = min_x.min(x);
                *max_x = max_x.max(x);
                *min_y = min_y.min(y);
                *max_y = max_y.max(y);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Drawing functions
// ---------------------------------------------------------------------------

fn write_grid(
    svg: &mut String,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    tx: &dyn Fn(f64) -> f64,
    ty: &dyn Fn(f64) -> f64,
) {
    svg.push_str("<g id=\"grid\">\n");
    let step = 1.0; // 1m grid
    let x_start = (min_x).ceil();
    let x_end = (max_x).floor();
    let y_start = (min_y).ceil();
    let y_end = (max_y).floor();

    let mut x = x_start;
    while x <= x_end {
        let sx = tx(x);
        let sy1 = ty(min_y);
        let sy2 = ty(max_y);
        let class = if (x as i64) % 5 == 0 {
            "grid-major"
        } else {
            "grid"
        };
        write!(
            svg,
            r#"<line x1="{sx:.1}" y1="{sy1:.1}" x2="{sx:.1}" y2="{sy2:.1}" class="{class}"/>"#,
        )
        .unwrap();
        svg.push('\n');
        if (x as i64) % 5 == 0 {
            write!(
                svg,
                r#"<text x="{sx:.1}" y="{yt:.1}" class="grid-label">{x:.0}</text>"#,
                yt = ty(min_y) + 10.0,
            )
            .unwrap();
            svg.push('\n');
        }
        x += step;
    }

    let mut y = y_start;
    while y <= y_end {
        let sy = ty(y);
        let sx1 = tx(min_x);
        let sx2 = tx(max_x);
        let class = if (y as i64) % 5 == 0 {
            "grid-major"
        } else {
            "grid"
        };
        write!(
            svg,
            r#"<line x1="{sx1:.1}" y1="{sy:.1}" x2="{sx2:.1}" y2="{sy:.1}" class="{class}"/>"#,
        )
        .unwrap();
        svg.push('\n');
        if (y as i64) % 5 == 0 {
            write!(
                svg,
                r#"<text x="{xt:.1}" y="{sy:.1}" class="grid-label">{y:.0}</text>"#,
                xt = tx(min_x) - 5.0,
            )
            .unwrap();
            svg.push('\n');
        }
        y += step;
    }
    svg.push_str("</g>\n");
}

fn draw_wall(
    svg: &mut String,
    elem: &ResolvedElement,
    options: &FloorplanOptions,
    tx: &dyn Fn(f64) -> f64,
    ty: &dyn Fn(f64) -> f64,
) {
    let Some(corners) = wall_outline(elem) else {
        return;
    };
    let wall_height = get_number(&elem.properties, "height").unwrap_or(2.8);

    let class = if wall_height > options.cut_height {
        "wall-cut"
    } else {
        "wall"
    };

    write!(
        svg,
        r#"<polygon points="{}" class="{class}" data-id="{id}" data-type="wall"/>"#,
        corners
            .iter()
            .map(|(x, y)| format!("{:.1},{:.1}", tx(*x), ty(*y)))
            .collect::<Vec<_>>()
            .join(" "),
        id = elem.id,
    )
    .unwrap();
    svg.push('\n');
}

/// Start and end of a door or window on its host wall's axis, following the
/// curve on curved walls.
fn opening_ends(
    parent: &ResolvedElement,
    offset: f64,
    width: f64,
) -> Option<((f64, f64), (f64, f64))> {
    let axis = WallAxis::of(parent, crate::config::default_curve_tolerance())?;
    let at = |s: f64| {
        let [x, y] = axis.point_at(s);
        (x, y)
    };
    Some((at(offset), at(offset + width)))
}

fn draw_window(
    svg: &mut String,
    child: &ResolvedElement,
    parent: &ResolvedElement,
    tx: &dyn Fn(f64) -> f64,
    ty: &dyn Fn(f64) -> f64,
) {
    let wall_thickness = get_number(&parent.properties, "thickness").unwrap_or(0.2);
    let offset = get_number(&child.properties, "offset").unwrap_or(0.0);
    let win_width = get_number(&child.properties, "width").unwrap_or(1.0);

    // Window ends on the wall center line
    let Some(((cx1, cy1), (cx2, cy2))) = opening_ends(parent, offset, win_width) else {
        return;
    };
    let angle = geometry::wall_angle((cx1, cy1), (cx2, cy2));
    let cos_a = angle.cos();
    let sin_a = angle.sin();
    let half_t = wall_thickness / 2.0;

    // Normal direction
    let nx = -sin_a * half_t;
    let ny = cos_a * half_t;

    // Draw window as a gap with glass lines
    // Outer rectangle (white to cut the wall)
    let gap_corners = [
        (cx1 - nx * 1.1, cy1 - ny * 1.1),
        (cx2 - nx * 1.1, cy2 - ny * 1.1),
        (cx2 + nx * 1.1, cy2 + ny * 1.1),
        (cx1 + nx * 1.1, cy1 + ny * 1.1),
    ];
    write!(
        svg,
        r#"<polygon points="{}" fill="white" stroke="none"/>"#,
        gap_corners
            .iter()
            .map(|(x, y)| format!("{:.1},{:.1}", tx(*x), ty(*y)))
            .collect::<Vec<_>>()
            .join(" "),
    )
    .unwrap();
    svg.push('\n');

    // Glass pane fill
    let glass_corners = [
        (cx1 - nx * 0.6, cy1 - ny * 0.6),
        (cx2 - nx * 0.6, cy2 - ny * 0.6),
        (cx2 + nx * 0.6, cy2 + ny * 0.6),
        (cx1 + nx * 0.6, cy1 + ny * 0.6),
    ];
    write!(
        svg,
        r#"<polygon points="{}" class="window-glass" data-id="{id}" data-type="window"/>"#,
        glass_corners
            .iter()
            .map(|(x, y)| format!("{:.1},{:.1}", tx(*x), ty(*y)))
            .collect::<Vec<_>>()
            .join(" "),
        id = child.id,
    )
    .unwrap();
    svg.push('\n');

    // Double glass lines (standard architectural symbol)
    let line_offset = half_t * 0.15;
    for sign in [-1.0_f64, 1.0] {
        let lnx = -sin_a * line_offset * sign;
        let lny = cos_a * line_offset * sign;
        write!(
            svg,
            r#"<line x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" class="window"/>"#,
            x1 = tx(cx1 + lnx),
            y1 = ty(cy1 + lny),
            x2 = tx(cx2 + lnx),
            y2 = ty(cy2 + lny),
        )
        .unwrap();
        svg.push('\n');
    }
}

fn draw_door(
    svg: &mut String,
    child: &ResolvedElement,
    parent: &ResolvedElement,
    tx: &dyn Fn(f64) -> f64,
    ty: &dyn Fn(f64) -> f64,
) {
    let wall_thickness = get_number(&parent.properties, "thickness").unwrap_or(0.2);
    let offset = get_number(&child.properties, "offset").unwrap_or(0.0);
    let door_width = get_number(&child.properties, "width").unwrap_or(0.9);

    let Some(((cx1, cy1), (cx2, cy2))) = opening_ends(parent, offset, door_width) else {
        return;
    };
    let angle = geometry::wall_angle((cx1, cy1), (cx2, cy2));
    let cos_a = angle.cos();
    let sin_a = angle.sin();
    let half_t = wall_thickness / 2.0;

    let nx = -sin_a * half_t;
    let ny = cos_a * half_t;

    // White gap to cut wall
    let gap_corners = [
        (cx1 - nx * 1.1, cy1 - ny * 1.1),
        (cx2 - nx * 1.1, cy2 - ny * 1.1),
        (cx2 + nx * 1.1, cy2 + ny * 1.1),
        (cx1 + nx * 1.1, cy1 + ny * 1.1),
    ];
    write!(
        svg,
        r#"<polygon points="{}" fill="white" stroke="none"/>"#,
        gap_corners
            .iter()
            .map(|(x, y)| format!("{:.1},{:.1}", tx(*x), ty(*y)))
            .collect::<Vec<_>>()
            .join(" "),
    )
    .unwrap();
    svg.push('\n');

    // Door leaf (line from hinge to open position)
    let hinge_x = cx1;
    let hinge_y = cy1;
    write!(
        svg,
        r#"<line x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" class="door" data-id="{id}" data-type="door"/>"#,
        x1 = tx(hinge_x),
        y1 = ty(hinge_y),
        x2 = tx(hinge_x - sin_a * door_width),
        y2 = ty(hinge_y + cos_a * door_width),
        id = child.id,
    )
    .unwrap();
    svg.push('\n');

    // Door swing arc (quarter circle)
    let arc_end_x = tx(cx2);
    let arc_end_y = ty(cy2);
    let arc_swing_x = tx(hinge_x - sin_a * door_width);
    let arc_swing_y = ty(hinge_y + cos_a * door_width);
    let r = door_width * (tx(1.0) - tx(0.0)); // radius in SVG units
    write!(
        svg,
        r#"<path d="M {sx:.1},{sy:.1} A {r:.1},{r:.1} 0 0 1 {ex:.1},{ey:.1}" class="door-swing"/>"#,
        sx = arc_end_x,
        sy = arc_end_y,
        r = r,
        ex = arc_swing_x,
        ey = arc_swing_y,
    )
    .unwrap();
    svg.push('\n');
}

fn draw_column(
    svg: &mut String,
    elem: &ResolvedElement,
    tx: &dyn Fn(f64) -> f64,
    ty: &dyn Fn(f64) -> f64,
) {
    let (x, y) = match get_point(&elem.properties, "position") {
        Some(v) => v,
        None => return,
    };

    let (hw, hd) = if let Some((profile_type, params)) = get_profile(&elem.properties, "profile") {
        match profile_type.as_str() {
            "rectangle" if params.len() >= 2 => (params[0] / 2.0, params[1] / 2.0),
            "circle" if !params.is_empty() => (params[0] / 2.0, params[0] / 2.0),
            _ => (0.15, 0.15),
        }
    } else {
        (0.15, 0.15)
    };

    write!(
        svg,
        r#"<rect x="{rx:.1}" y="{ry:.1}" width="{w:.1}" height="{h:.1}" class="column" data-id="{id}" data-type="column"/>"#,
        rx = tx(x - hw),
        ry = ty(y + hd),
        w = (tx(x + hw) - tx(x - hw)).abs(),
        h = (ty(y - hd) - ty(y + hd)).abs(),
        id = elem.id,
    )
    .unwrap();
    svg.push('\n');
}

fn draw_slab(
    svg: &mut String,
    elem: &ResolvedElement,
    tx: &dyn Fn(f64) -> f64,
    ty: &dyn Fn(f64) -> f64,
) {
    let boundary = match get_polygon(&elem.properties, "boundary") {
        Some(v) => v,
        None => return,
    };

    write!(
        svg,
        r#"<polygon points="{}" class="slab" data-id="{id}" data-type="slab"/>"#,
        boundary
            .iter()
            .map(|(x, y)| format!("{:.1},{:.1}", tx(*x), ty(*y)))
            .collect::<Vec<_>>()
            .join(" "),
        id = elem.id,
    )
    .unwrap();
    svg.push('\n');
}

/// Plan outline of a beam or member along its axis (`path:` or `from` / `to`),
/// as wide as its rectangle profile, else `2 * default_hw`.
fn bar_outline(elem: &ResolvedElement, default_hw: f64) -> Option<Vec<(f64, f64)>> {
    let axis = WallAxis::of(elem, crate::config::default_curve_tolerance())?;
    let hw = match get_profile(&elem.properties, "profile") {
        Some((t, params)) if t == "rectangle" && params.len() >= 2 => params[0] / 2.0,
        _ => default_hw,
    };
    Some(geometry::wall_outline(&axis, 2.0 * hw))
}

fn draw_beam(
    svg: &mut String,
    elem: &ResolvedElement,
    tx: &dyn Fn(f64) -> f64,
    ty: &dyn Fn(f64) -> f64,
) {
    let Some(corners) = bar_outline(elem, 0.15) else {
        return;
    };

    write!(
        svg,
        r#"<polygon points="{}" class="beam" data-id="{id}" data-type="beam"/>"#,
        corners
            .iter()
            .map(|(x, y)| format!("{:.1},{:.1}", tx(*x), ty(*y)))
            .collect::<Vec<_>>()
            .join(" "),
        id = elem.id,
    )
    .unwrap();
    svg.push('\n');
}

fn draw_stair(
    svg: &mut String,
    elem: &ResolvedElement,
    tx: &dyn Fn(f64) -> f64,
    ty: &dyn Fn(f64) -> f64,
) {
    // The whole of a stair with landings: its flights and landing are drawn.
    if crate::codegen::ifc_writer::no_body(elem) {
        return;
    }
    let (flight, position, rotation) = stair_plan(elem);
    let (w, run) = (flight.width, flight.risers as f64 * flight.tread);
    let points = |local: Vec<(f64, f64)>| {
        geometry::placed(local, position, rotation)
            .into_iter()
            .map(|(x, y)| format!("{:.1},{:.1}", tx(x), ty(y)))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let line = |svg: &mut String, a: (f64, f64), b: (f64, f64), color: &str, width: f64| {
        writeln!(
            svg,
            r#"<polyline points="{}" stroke="{color}" stroke-width="{width}" fill="none"/>"#,
            points(vec![a, b])
        )
        .unwrap();
    };

    writeln!(
        svg,
        r#"<polygon points="{}" class="stair" data-id="{id}" data-type="stair"/>"#,
        points(flight_outline(&flight)),
        id = elem.id,
    )
    .unwrap();
    for i in 1..flight.risers {
        let along = i as f64 * flight.tread;
        line(svg, (0.0, along), (w, along), "#aaa", 0.5);
    }
    // Direction arrow
    if run > 0.6 {
        line(svg, (w / 2.0, 0.3), (w / 2.0, run - 0.3), "#888", 1.0);
    }
}

/// A stair flight with its position and rotation. The plan does not use the
/// riser height.
fn stair_plan(elem: &ResolvedElement) -> (geometry::Flight, (f64, f64), f64) {
    let p = &elem.properties;
    (
        geometry::Flight::of(p, 3.0),
        get_point(p, "position").unwrap_or((0.0, 0.0)),
        get_number(p, "rotation").unwrap_or(0.0),
    )
}

/// The flight's outline in its local frame: x across the width, y along the run.
fn flight_outline(f: &geometry::Flight) -> Vec<(f64, f64)> {
    let run = f.risers as f64 * f.tread;
    vec![(0.0, 0.0), (f.width, 0.0), (f.width, run), (0.0, run)]
}

fn draw_furniture(
    svg: &mut String,
    elem: &ResolvedElement,
    tx: &dyn Fn(f64) -> f64,
    ty: &dyn Fn(f64) -> f64,
) {
    let (x, y) = get_point(&elem.properties, "position").unwrap_or((0.0, 0.0));
    let size = 0.5;

    write!(
        svg,
        r#"<rect x="{rx:.1}" y="{ry:.1}" width="{w:.1}" height="{h:.1}" class="furniture" data-id="{id}" data-type="furniture"/>"#,
        rx = tx(x - size),
        ry = ty(y + size),
        w = (tx(x + size) - tx(x - size)).abs(),
        h = (ty(y - size) - ty(y + size)).abs(),
        id = elem.id,
    )
    .unwrap();
    svg.push('\n');
}

fn draw_proxy(
    svg: &mut String,
    elem: &ResolvedElement,
    tx: &dyn Fn(f64) -> f64,
    ty: &dyn Fn(f64) -> f64,
) {
    let (x, y) = get_point(&elem.properties, "position").unwrap_or((0.0, 0.0));
    let w = get_number(&elem.properties, "width").unwrap_or(0.5);
    let d = get_number(&elem.properties, "depth").unwrap_or(0.5);
    let hw = w / 2.0;
    let hd = d / 2.0;

    write!(
        svg,
        r#"<rect x="{rx:.1}" y="{ry:.1}" width="{rw:.1}" height="{rh:.1}" class="proxy" data-id="{id}" data-type="proxy"/>"#,
        rx = tx(x - hw),
        ry = ty(y + hd),
        rw = (tx(x + hw) - tx(x - hw)).abs(),
        rh = (ty(y - hd) - ty(y + hd)).abs(),
        id = elem.id,
    )
    .unwrap();
    svg.push('\n');
}

type Polyline = Vec<(f64, f64)>;

/// Plan of a duct, pipe or cable tray (centreline polylines and section
/// width) or of a terminal, equipment or accessory (body outline, width 0).
fn mep_plan(elem: &ResolvedElement) -> Option<(Vec<Polyline>, f64)> {
    let p = &elem.properties;
    if matches!(
        elem.element_type,
        ElementType::Terminal | ElementType::Equipment | ElementType::Accessory
    ) {
        let (lo, hi) = mep::body_box(elem)?;
        let outline = vec![
            (lo[0], lo[1]),
            (hi[0], lo[1]),
            (hi[0], hi[1]),
            (lo[0], hi[1]),
        ];
        return Some((vec![outline], 0.0));
    }
    if !mep::is_run(elem.element_type) {
        return None;
    }
    let width = match mep::Section::of(elem.element_type, p) {
        mep::Section::Round(d) => d,
        mep::Section::Rect(w, _) | mep::Section::Tray(w, _) => w,
    };
    let lines = match (mep::path(p), mep::axis3(p)) {
        (Some(path), _) => vec![path],
        (None, Some((a, b))) => match mep::branch_point(p) {
            // Main run, and the branch from its foot on the main axis.
            Some(c) => {
                let d = normalize(sub(b, a));
                vec![vec![a, b], vec![add(a, d, dot(sub(c, a), d)), c]]
            }
            None => vec![vec![a, b]],
        },
        _ => return None,
    };
    let plan = lines
        .iter()
        .map(|l| l.iter().map(|q| (q[0], q[1])).collect())
        .collect();
    Some((plan, width))
}

fn draw_mep(
    svg: &mut String,
    elem: &ResolvedElement,
    tx: &dyn Fn(f64) -> f64,
    ty: &dyn Fn(f64) -> f64,
) {
    let Some((lines, width)) = mep_plan(elem) else {
        return;
    };
    let class = crate::semantic::registry::get_descriptor(elem.element_type).svg_css_class;
    let close = if width > 0.0 { "" } else { " Z" };
    let d = lines
        .iter()
        .map(|l| {
            let pts: Vec<String> = l
                .iter()
                .map(|(x, y)| format!("{:.1},{:.1}", tx(*x), ty(*y)))
                .collect();
            format!("M {}{close}", pts.join(" L "))
        })
        .collect::<Vec<_>>()
        .join(" ");
    let stroke = if width > 0.0 {
        format!(r#" stroke-width="{:.1}""#, (tx(width) - tx(0.0)).max(1.0))
    } else {
        String::new()
    };
    writeln!(
        svg,
        r#"<path d="{d}" class="{class}"{stroke} data-id="{id}" data-type="{class}"/>"#,
        id = elem.id,
    )
    .unwrap();
}

fn draw_member(
    svg: &mut String,
    elem: &ResolvedElement,
    tx: &dyn Fn(f64) -> f64,
    ty: &dyn Fn(f64) -> f64,
) {
    let Some(corners) = bar_outline(elem, 0.05) else {
        return;
    };

    write!(
        svg,
        r#"<polygon points="{}" class="member" data-id="{id}" data-type="member"/>"#,
        corners
            .iter()
            .map(|(x, y)| format!("{:.1},{:.1}", tx(*x), ty(*y)))
            .collect::<Vec<_>>()
            .join(" "),
        id = elem.id,
    )
    .unwrap();
    svg.push('\n');
}

fn draw_plate(
    svg: &mut String,
    elem: &ResolvedElement,
    tx: &dyn Fn(f64) -> f64,
    ty: &dyn Fn(f64) -> f64,
) {
    let boundary = match get_polygon(&elem.properties, "boundary") {
        Some(v) => v,
        None => return,
    };

    write!(
        svg,
        r#"<polygon points="{}" class="plate" data-id="{id}" data-type="plate"/>"#,
        boundary
            .iter()
            .map(|(x, y)| format!("{:.1},{:.1}", tx(*x), ty(*y)))
            .collect::<Vec<_>>()
            .join(" "),
        id = elem.id,
    )
    .unwrap();
    svg.push('\n');
}

fn draw_structural_grid(
    svg: &mut String,
    elem: &ResolvedElement,
    tx: &dyn Fn(f64) -> f64,
    ty: &dyn Fn(f64) -> f64,
) {
    let (px, py) = get_point(&elem.properties, "position").unwrap_or((0.0, 0.0));
    let x_spacing = get_number(&elem.properties, "x_spacing").unwrap_or(1.0);
    let y_spacing = get_number(&elem.properties, "y_spacing").unwrap_or(1.0);
    let x_count = get_number(&elem.properties, "x_count").unwrap_or(2.0) as usize;
    let y_count = get_number(&elem.properties, "y_count").unwrap_or(2.0) as usize;

    let x_total = (x_count.saturating_sub(1)) as f64 * x_spacing;
    let y_total = (y_count.saturating_sub(1)) as f64 * y_spacing;

    // Draw X-direction axes (vertical lines) with labels
    for i in 0..x_count {
        let ax = px + i as f64 * x_spacing;
        let label = ((b'A' + (i as u8) % 26) as char).to_string();
        let sx = tx(ax);
        let sy1 = ty(py);
        let sy2 = ty(py + y_total);
        write!(
            svg,
            r#"<line x1="{sx:.1}" y1="{sy1:.1}" x2="{sx:.1}" y2="{sy2:.1}" class="structural-grid" data-id="{id}" data-type="grid"/>"#,
            id = elem.id,
        )
        .unwrap();
        svg.push('\n');
        // Label at top
        write!(
            svg,
            r#"<text x="{sx:.1}" y="{y:.1}" class="grid-axis-label">{label}</text>"#,
            y = sy1 - 8.0,
        )
        .unwrap();
        svg.push('\n');
    }

    // Draw Y-direction axes (horizontal lines) with labels
    for i in 0..y_count {
        let ay = py + i as f64 * y_spacing;
        let label = (i + 1).to_string();
        let sy = ty(ay);
        let sx1 = tx(px);
        let sx2 = tx(px + x_total);
        write!(
            svg,
            r#"<line x1="{sx1:.1}" y1="{sy:.1}" x2="{sx2:.1}" y2="{sy:.1}" class="structural-grid" data-id="{id}" data-type="grid"/>"#,
            id = elem.id,
        )
        .unwrap();
        svg.push('\n');
        // Label at left
        write!(
            svg,
            r#"<text x="{x:.1}" y="{sy:.1}" class="grid-axis-label">{label}</text>"#,
            x = sx1 - 8.0,
        )
        .unwrap();
        svg.push('\n');
    }
}

fn draw_wall_dimension(
    svg: &mut String,
    elem: &ResolvedElement,
    tx: &dyn Fn(f64) -> f64,
    ty: &dyn Fn(f64) -> f64,
) {
    let Some(axis) = WallAxis::of(elem, crate::config::default_curve_tolerance()) else {
        return;
    };

    let length = axis.length();
    if length < 0.1 {
        return;
    }

    let [mid_x, mid_y] = axis.point_at(length / 2.0);
    let [dx, dy] = axis.tangent_at(length / 2.0);

    // Offset the dimension text perpendicular to the wall
    let offset = 0.5;
    let dim_x = mid_x - dy * offset;
    let dim_y = mid_y + dx * offset;

    let label = if (length - length.round()).abs() < 0.01 {
        format!("{:.0}", length)
    } else {
        format!("{:.2}", length)
    };

    write!(
        svg,
        r#"<text x="{x:.1}" y="{y:.1}" class="dim-text" data-wall="{id}">{label}m</text>"#,
        x = tx(dim_x),
        y = ty(dim_y),
        id = elem.id,
    )
    .unwrap();
    svg.push('\n');
}

/// First and last point of a wall's axis.
fn axis_ends(elem: &ResolvedElement) -> Option<((f64, f64), (f64, f64))> {
    let axis = WallAxis::of(elem, crate::config::default_curve_tolerance())?;
    let (a, b) = (axis.point_at(0.0), axis.point_at(axis.length()));
    Some(((a[0], a[1]), (b[0], b[1])))
}

fn draw_space_label(
    svg: &mut String,
    space: &crate::semantic::model::ResolvedSpace,
    all_elements: &[&ResolvedElement],
    tx: &dyn Fn(f64) -> f64,
    ty: &dyn Fn(f64) -> f64,
) {
    // Find centroid of walls belonging to this space
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut count = 0;

    for elem in &space.elements {
        if elem.element_type == ElementType::Wall
            && let Some((from, to)) = axis_ends(elem)
        {
            sum_x += from.0 + to.0;
            sum_y += from.1 + to.1;
            count += 2;
        }
    }

    // Also check elements at storey level that might belong to this space
    if count == 0 {
        for elem in all_elements.iter() {
            if elem.element_type == ElementType::Wall
                && let Some((from, to)) = axis_ends(elem)
            {
                sum_x += from.0 + to.0;
                sum_y += from.1 + to.1;
                count += 2;
            }
        }
    }

    if count == 0 {
        return;
    }

    let cx = sum_x / count as f64;
    let cy = sum_y / count as f64;

    let area = get_number(&space.properties, "area");
    let area_text = area
        .map(|a| format!(" ({:.1}m\u{00b2})", a))
        .unwrap_or_default();

    write!(
        svg,
        r#"<text x="{x:.1}" y="{y:.1}" class="label" data-space="{name}">{name}{area}</text>"#,
        x = tx(cx),
        y = ty(cy),
        name = space.name,
        area = area_text,
    )
    .unwrap();
    svg.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cst_to_ast;
    use crate::semantic;

    #[test]
    fn test_floorplan_basic() {
        let source = "\
# Site: S
## Building: B
### Storey: Ground Floor
  elevation: 0 m
  height: 2.8 m
  Wall: W-01
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.24 m
  Wall: W-02
    from: (6, 0) to: (6, 5)
    height: 2.8 m
    thickness: 0.24 m
  Wall: W-03
    from: (6, 5) to: (0, 5)
    height: 2.8 m
    thickness: 0.24 m
  Wall: W-04
    from: (0, 5) to: (0, 0)
    height: 2.8 m
    thickness: 0.24 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let svg = render_floorplan(&model, 0, &FloorplanOptions::default()).unwrap();

        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
        assert!(svg.contains("Ground Floor"));
        assert!(svg.contains("data-id=\"W-01\""));
        assert!(svg.contains("data-id=\"W-02\""));
        assert!(svg.contains("data-type=\"wall\""));
    }

    /// An arc wall (`via:`) is drawn as a ring segment, not as its chord.
    #[test]
    fn test_floorplan_arc_wall() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W-ARC
    from: (0, 0) via: (5, 5) to: (10, 0)
    height: 3 m
    thickness: 0.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let svg = render_floorplan(&model, 0, &FloorplanOptions::default()).unwrap();
        let points = svg
            .lines()
            .find(|l| l.contains("data-id=\"W-ARC\""))
            .and_then(|l| l.split('"').nth(1))
            .unwrap();
        // The arc is tessellated to 5 mm chord error: two rings of 37 points.
        assert_eq!(points.split(' ').count(), 74);
    }

    /// A beam on a curved `path:` and a wall on one are drawn, the wall dimensioned
    /// by its length along the curve.
    #[test]
    fn test_floorplan_path_beam_and_wall() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Beam: B-1
    path: [(0, 0), smooth (5, 3), (10, 0)]
    z_offset: 3 m
    profile: rectangle(0.2 m, 0.4 m)
  Wall: W-1
    path: [(0, -5), smooth (5, -2), (10, -5)]
    height: 3 m
    thickness: 0.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, diags) = semantic::analyze(&ast);
        assert!(
            diags
                .iter()
                .all(|d| d.severity != crate::semantic::diagnostics::Severity::Error),
            "{diags:?}"
        );
        let svg = render_floorplan(&model, 0, &FloorplanOptions::default()).unwrap();
        let beam = svg.lines().find(|l| l.contains("data-id=\"B-1\"")).unwrap();
        assert!(
            beam.split('"').nth(1).unwrap().split(' ').count() > 10,
            "{beam}"
        );
        let dim = svg
            .lines()
            .find(|l| l.contains("data-wall=\"W-1\""))
            .unwrap();
        let len: f64 = dim
            .split('>')
            .nth(1)
            .unwrap()
            .trim_end_matches("m</text")
            .parse()
            .unwrap();
        assert!(len > 10.5, "{dim}");
    }

    #[test]
    fn test_floorplan_door_on_arc_wall() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W-ARC
    from: (0, 0) via: (5, 5) to: (10, 0)
    height: 3 m
    thickness: 0.2 m
    Door: D1
      offset: 7.0 m
      width: 1.0 m
      height: 2.1 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let svg = render_floorplan(&model, 0, &FloorplanOptions::default()).unwrap();
        let attr = |line: &str, name: &str| -> f64 {
            let value = line.split(&format!("{name}=\"")).nth(1).unwrap();
            value.split('"').next().unwrap().parse().unwrap()
        };
        let height = attr(
            svg.lines().find(|l| l.starts_with("<svg")).unwrap(),
            "height",
        );
        let door = svg.lines().find(|l| l.contains("data-id=\"D1\"")).unwrap();
        // 7 m along the arc is near its apex (world y 4.93); the straight chord
        // would put the hinge on the baseline (world y 0) at the bottom of the plan.
        assert!(attr(door, "y1") < 0.3 * height, "{door}");
    }

    #[test]
    fn test_floorplan_with_openings() {
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
    Door: D1
      offset: 4.0 m
      width: 0.9 m
      height: 2.1 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let svg = render_floorplan(&model, 0, &FloorplanOptions::default()).unwrap();

        assert!(svg.contains("data-id=\"WIN1\""));
        assert!(svg.contains("data-type=\"window\""));
        assert!(svg.contains("data-id=\"D1\""));
        assert!(svg.contains("data-type=\"door\""));
    }

    #[test]
    fn test_floorplan_column_slab() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Column: COL1
    position: (3, 2)
    height: 3.0 m
    profile: rectangle(0.3, 0.3)
  Slab: S1
    boundary: [(0,0), (6,0), (6,5), (0,5)]
    thickness: 0.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let svg = render_floorplan(&model, 0, &FloorplanOptions::default()).unwrap();

        assert!(svg.contains("data-id=\"COL1\""));
        assert!(svg.contains("data-type=\"column\""));
        assert!(svg.contains("data-id=\"S1\""));
        assert!(svg.contains("data-type=\"slab\""));
    }

    #[test]
    fn test_storey_names() {
        let source = "\
# Site: S
## Building: B
### Storey: Ground Floor
  elevation: 0 m
### Storey: First Floor
  elevation: 3.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let names = storey_names(&model);
        assert_eq!(names, vec!["Ground Floor", "First Floor"]);
    }

    #[test]
    fn test_floorplan_building_and_site_level() {
        let source = "\
# Site: Campus
  Wall: W-Fence
    from: (0, 0) to: (20, 0)
    height: 1.5 m
    thickness: 0.1 m
## Building: B
#### Space: Atrium
  Column: C-Atrium
    position: (3, 3)
    height: 9 m
    profile: rectangle(0.4, 0.4)
### Storey: GF
  elevation: 0 m
## Building: Shed
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        // The empty building gets no plan
        assert_eq!(storey_names(&model), vec!["GF", "B", "Campus"]);
        let options = FloorplanOptions::default();
        let building = render_floorplan(&model, 1, &options).unwrap();
        assert!(building.contains("C-Atrium") && building.contains("Atrium"));
        let site = render_floorplan(&model, 2, &options).unwrap();
        assert!(site.contains("W-Fence"));
    }

    #[test]
    fn test_floorplan_invalid_storey() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        assert!(render_floorplan(&model, 5, &FloorplanOptions::default()).is_none());
    }

    #[test]
    fn test_floorplan_all_elements() {
        let source = std::fs::read_to_string("../../tests/fixtures/all_elements.bml").unwrap();
        let ast = cst_to_ast::parse(&source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let svg = render_floorplan(&model, 0, &FloorplanOptions::default()).unwrap();

        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
        // Should contain walls, columns, slabs at minimum
        assert!(svg.contains("data-type=\"wall\""));
        // Should contain the new element types from all_elements.bml
        assert!(svg.contains("data-type=\"proxy\""));
        assert!(svg.contains("data-type=\"member\""));
        assert!(svg.contains("data-type=\"plate\""));
    }

    #[test]
    fn test_floorplan_proxy_member_plate() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Proxy: PX-01
    position: (2, 3)
    width: 0.6 m
    depth: 0.4 m
    height: 1.0 m
  Member: MEM-01
    from: (0, 5) to: (6, 5)
    profile: rectangle(0.1, 0.2)
  Plate: PL-01
    boundary: [(1, 1), (3, 1), (3, 2), (1, 2)]
    thickness: 0.012 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let svg = render_floorplan(&model, 0, &FloorplanOptions::default()).unwrap();

        assert!(svg.contains("data-id=\"PX-01\""));
        assert!(svg.contains("data-type=\"proxy\""));
        assert!(svg.contains("class=\"proxy\""));

        assert!(svg.contains("data-id=\"MEM-01\""));
        assert!(svg.contains("data-type=\"member\""));
        assert!(svg.contains("class=\"member\""));

        assert!(svg.contains("data-id=\"PL-01\""));
        assert!(svg.contains("data-type=\"plate\""));
        assert!(svg.contains("class=\"plate\""));
    }
}
