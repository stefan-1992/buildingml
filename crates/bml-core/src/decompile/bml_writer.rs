use crate::geometry::curve::{Curve, Point, clamped_uniform};
use crate::semantic::model::*;

/// Generate BuildingML source text from a semantic model.
pub fn generate_bml(model: &SemanticModel) -> String {
    let mut output = String::new();

    // Write materials first
    for mat in &model.materials {
        write_material(&mut output, mat);
        output.push('\n');
    }

    // Write spatial hierarchy
    for site in &model.sites {
        write_site(&mut output, site);
    }

    output
}

fn write_material(output: &mut String, mat: &ResolvedMaterial) {
    output.push_str(&format!("Material: {}\n", mat.id));
    match &mat.material_type {
        // `layers:` needs at least one item to parse
        MaterialType::Layered { layers } if layers.is_empty() => {
            output.push_str("  type: layered\n");
        }
        MaterialType::Layered { layers } => {
            output.push_str("  layers:\n");
            for layer in layers {
                output.push_str(&format!(
                    "    - {}: {} m\n",
                    layer.name,
                    format_number(layer.thickness)
                ));
            }
        }
        MaterialType::Homogeneous {
            category,
            grade,
            density,
        } => {
            if let Some(cat) = category {
                output.push_str(&format!("  category: {}\n", cat));
            }
            if let Some(g) = grade {
                output.push_str(&format!("  grade: {}\n", g));
            }
            if let Some(d) = density {
                output.push_str(&format!("  density: {} kg/m3\n", d));
            }
        }
    }
    for prop in &mat.properties {
        if matches!(
            prop.key.as_str(),
            "type" | "category" | "grade" | "density" | "layers"
        ) {
            continue; // Already written above
        }
        write_property(output, &prop.key, &prop.value, 2);
    }
}

fn write_site(output: &mut String, site: &ResolvedSite) {
    output.push_str(&format!("# Site: {}\n", site.name));
    for prop in &site.properties {
        write_property(output, &prop.key, &prop.value, 2);
    }
    output.push('\n');
    write_contents(output, &site.spaces, &site.elements);

    for building in &site.buildings {
        write_building(output, building);
    }
}

fn write_building(output: &mut String, building: &ResolvedBuilding) {
    output.push_str(&format!("## Building: {}\n", building.name));
    for prop in &building.properties {
        write_property(output, &prop.key, &prop.value, 2);
    }
    output.push('\n');
    write_contents(output, &building.spaces, &building.elements);

    for storey in &building.storeys {
        write_storey(output, storey);
    }
}

fn write_storey(output: &mut String, storey: &ResolvedStorey) {
    output.push_str(&format!("### Storey: {}\n", storey.name));
    for prop in &storey.properties {
        write_property(output, &prop.key, &prop.value, 2);
    }
    output.push('\n');
    write_contents(output, &storey.spaces, &storey.elements);
}

/// Elements of a site, building or storey, then its spaces. They come before
/// any lower heading, and elements before spaces: anything after a `####`
/// heading belongs to that space.
fn write_contents(output: &mut String, spaces: &[ResolvedSpace], elements: &[ResolvedElement]) {
    for elem in elements {
        write_element(output, elem, 2);
    }
    for space in spaces {
        write_space(output, space);
    }
}

fn write_space(output: &mut String, space: &ResolvedSpace) {
    output.push_str(&format!("#### Space: {}\n", space.name));
    for prop in &space.properties {
        match space.curve(&prop.key) {
            Some(c) => write_curve(output, &prop.key, c, 2),
            None => write_property(output, &prop.key, &prop.value, 2),
        }
    }
    output.push('\n');

    for elem in &space.elements {
        write_element(output, elem, 2);
    }
}

fn write_element(output: &mut String, elem: &ResolvedElement, indent: usize) {
    let prefix = " ".repeat(indent);
    output.push_str(&format!(
        "{}{}: {}\n",
        prefix,
        elem.element_type.as_str(),
        elem.id
    ));

    for prop in &elem.properties {
        match elem.curve(&prop.key) {
            Some(c) => write_curve(output, &prop.key, c, indent + 2),
            None => write_property(output, &prop.key, &prop.value, indent + 2),
        }
    }

    for child in &elem.children {
        write_element(output, child, indent + 2);
    }
    output.push('\n');
}

fn write_property(output: &mut String, key: &str, value: &ResolvedValue, indent: usize) {
    let prefix = " ".repeat(indent);
    match value {
        ResolvedValue::List(items) => {
            output.push_str(&format!("{}{}:\n", prefix, key));
            for item in items {
                let mut line = String::new();
                write_property(&mut line, &item.key, &item.value, 0);
                output.push_str(&format!("{}  - {}", prefix, line));
            }
        }
        ResolvedValue::Number(n) => {
            output.push_str(&format!("{}{}: {}\n", prefix, key, format_number(*n)));
        }
        ResolvedValue::String(s) => {
            output.push_str(&format!("{}{}: \"{}\"\n", prefix, key, escape(s)));
        }
        ResolvedValue::Boolean(b) => {
            output.push_str(&format!("{}{}: {}\n", prefix, key, b));
        }
        ResolvedValue::Identifier(id) => {
            output.push_str(&format!("{}{}: {}\n", prefix, key, id));
        }
        ResolvedValue::Point(x, y) => {
            output.push_str(&format!(
                "{}{}: ({}, {})\n",
                prefix,
                key,
                format_number(*x),
                format_number(*y)
            ));
        }
        ResolvedValue::Polygon(pts) => {
            let pts_str: Vec<String> = pts
                .iter()
                .map(|(x, y)| format!("({},{})", format_number(*x), format_number(*y)))
                .collect();
            output.push_str(&format!("{}{}: [{}]\n", prefix, key, pts_str.join(", ")));
        }
        ResolvedValue::Profile {
            profile_type,
            params,
        } => {
            let params_str: Vec<String> = params.iter().map(|p| format_number(*p)).collect();
            output.push_str(&format!(
                "{}{}: {}({})\n",
                prefix,
                key,
                profile_type,
                params_str.join(", ")
            ));
        }
        ResolvedValue::Axis { from, to, via, z } => {
            let pt = |(x, y): (f64, f64), z: Option<f64>| match z {
                Some(z) => format!(
                    "({}, {}, {})",
                    format_number(x),
                    format_number(y),
                    format_number(z)
                ),
                None => format!("({}, {})", format_number(x), format_number(y)),
            };
            let via = via
                .map(|v| format!(" via: {}", pt(v, None)))
                .unwrap_or_default();
            output.push_str(&format!(
                "{}{}: {}{} to: {}\n",
                prefix,
                key,
                pt(*from, z.map(|z| z.0)),
                via,
                pt(*to, z.map(|z| z.1))
            ));
        }
        ResolvedValue::Path3D(pts) => {
            let pts_str: Vec<String> = pts
                .iter()
                .map(|(x, y, z)| {
                    format!(
                        "({}, {}, {})",
                        format_number(*x),
                        format_number(*y),
                        format_number(*z)
                    )
                })
                .collect();
            output.push_str(&format!("{}{}: [{}]\n", prefix, key, pts_str.join(", ")));
        }
        ResolvedValue::PolygonWithHoles { outer, holes } => {
            let outer_str: Vec<String> = outer
                .iter()
                .map(|(x, y)| format!("({}, {})", format_number(*x), format_number(*y)))
                .collect();
            output.push_str(&format!("{}boundary: [{}]\n", prefix, outer_str.join(", ")));
            if !holes.is_empty() {
                let holes_str: Vec<String> = holes
                    .iter()
                    .map(|hole| {
                        let pts: Vec<String> = hole
                            .iter()
                            .map(|(x, y)| format!("({}, {})", format_number(*x), format_number(*y)))
                            .collect();
                        format!("[{}]", pts.join(", "))
                    })
                    .collect();
                output.push_str(&format!("{}holes: [{}]\n", prefix, holes_str.join(", ")));
            }
        }
        ResolvedValue::EnumList(vals) => {
            output.push_str(&format!("{}{}: [{}]\n", prefix, key, vals.join(", ")));
        }
        ResolvedValue::NumberList(vals) => {
            let vals: Vec<String> = vals.iter().map(|v| format_number(*v)).collect();
            output.push_str(&format!("{}{}: [{}]\n", prefix, key, vals.join(", ")));
        }
        ResolvedValue::Point3D(x, y, z) => {
            output.push_str(&format!(
                "{}{}: ({}, {}, {})\n",
                prefix,
                key,
                format_number(*x),
                format_number(*y),
                format_number(*z)
            ));
        }
        ResolvedValue::GeometryPrimitive(geom) => {
            use crate::semantic::model::GeometryPrimitive;
            let s = match geom {
                GeometryPrimitive::Box(w, d, h) => format!(
                    "box({}, {}, {})",
                    format_number(*w),
                    format_number(*d),
                    format_number(*h)
                ),
                GeometryPrimitive::Cylinder(r, h) => {
                    format!("cylinder({}, {})", format_number(*r), format_number(*h))
                }
                GeometryPrimitive::Extrude(pts, h) => {
                    let pts_str: Vec<String> = pts
                        .iter()
                        .map(|(x, y)| format!("({}, {})", format_number(*x), format_number(*y)))
                        .collect();
                    format!("extrude([{}], {})", pts_str.join(", "), format_number(*h))
                }
                GeometryPrimitive::Import(path) => format!("import(\"{}\")", path),
            };
            output.push_str(&format!("{}{}: {}\n", prefix, key, s));
        }
    }
}

/// An exact curve as a point list: lines as corners, the rest as `nurbs`
/// items. Weights are left out when all 1, knots when clamped uniform.
fn write_curve(output: &mut String, key: &str, curve: &ElementCurve, indent: usize) {
    let prefix = " ".repeat(indent);
    let line = match curve {
        ElementCurve::Path2(c) => format!("{key}: {}", curve_items(c)),
        ElementCurve::Path3(c) => format!("{key}: {}", curve_items(c)),
        ElementCurve::Region { outer, holes } if holes.is_empty() => {
            format!("{key}: {}", curve_items(outer))
        }
        ElementCurve::Region { outer, holes } => {
            let holes: Vec<String> = holes.iter().map(curve_items).collect();
            format!(
                "{key}: {{ outer: {}, holes: [{}] }}",
                curve_items(outer),
                holes.join(", ")
            )
        }
    };
    output.push_str(&format!("{prefix}{line}\n"));
}

fn curve_items<P: Point + AsRef<[f64]>>(curve: &Curve<P>) -> String {
    let pt = |p: &P| {
        let c: Vec<String> = p.as_ref().iter().map(|v| format_number(*v)).collect();
        format!("({})", c.join(", "))
    };
    let list = |pts: &mut dyn Iterator<Item = String>| pts.collect::<Vec<_>>().join(", ");
    let mut items = Vec::new();
    let mut end: Option<P> = None;
    let mut corner_end = false;
    for seg in &curve.segments {
        let Some(n) = seg.to_nurbs() else {
            if end != Some(seg.start()) {
                items.push(pt(&seg.start()));
            }
            items.push(pt(&seg.end()));
            end = Some(seg.end());
            corner_end = true;
            continue;
        };
        let knots = n.normalized_knots();
        let uniform = clamped_uniform(n.degree, n.points.len());
        let mut item = format!(
            "nurbs {{ degree: {}, points: [{}]",
            n.degree,
            list(&mut n.points.iter().map(pt))
        );
        if n.weights.iter().any(|w| (w - 1.0).abs() > 1e-9) {
            let w = list(&mut n.weights.iter().map(|w| ratio(*w)));
            item.push_str(&format!(", weights: [{w}]"));
        }
        if knots
            .iter()
            .zip(&uniform)
            .any(|(a, b)| (a - b).abs() > 1e-6)
        {
            let k = list(&mut knots.iter().map(|k| ratio(*k)));
            item.push_str(&format!(", knots: [{k}]"));
        }
        item.push_str(" }");
        // A corner the nurbs starts at is its first control point already.
        if corner_end && end == Some(n.points[0]) {
            items.pop();
        }
        end = n.points.last().copied();
        corner_end = false;
        items.push(item);
    }
    // A closed curve closes itself: no repeated start point.
    let start = curve.segments.first().map(|s| s.start());
    if curve.closed && corner_end && end == start && items.len() > 1 {
        items.pop();
    }
    format!("[{}]", items.join(", "))
}

/// Weights and knots keep 6 decimals: they are ratios, not lengths.
fn ratio(v: f64) -> String {
    ((v * 1e6).round() / 1e6 + 0.0).to_string()
}

/// Numbers are rounded to 4 decimals - a tenth of a millimetre in metres.
fn format_number(n: f64) -> String {
    let n = (n * 1e4).round() / 1e4 + 0.0; // `+ 0.0` turns -0.0 into 0.0
    if n == n.floor() && n.abs() < 1e10 {
        format!("{:.1}", n)
    } else {
        format!("{}", n)
    }
}

/// Escape a string for a BML string literal.
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ")
}
