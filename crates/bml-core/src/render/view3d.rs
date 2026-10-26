use std::fmt::Write;

use crate::scene::graph::SceneGraph;

/// Camera angle presets for 3D views.
#[derive(Debug, Clone, Copy)]
pub enum ViewAngle {
    /// Looking from +Y toward -Y (front elevation)
    Front,
    /// Looking from -Y toward +Y (rear elevation)
    Back,
    /// Looking from -X toward +X (left elevation)
    Left,
    /// Looking from +X toward -X (right elevation)
    Right,
    /// Looking from +Z toward -Z (plan/top view)
    Top,
    /// Standard isometric angle (45° azimuth, 35.264° elevation)
    Isometric,
}

impl ViewAngle {
    /// Parse from string (for CLI).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "front" => Some(Self::Front),
            "back" => Some(Self::Back),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "top" => Some(Self::Top),
            "iso" | "isometric" => Some(Self::Isometric),
            _ => None,
        }
    }

    /// File name suffix for this view angle.
    pub fn file_suffix(&self) -> &'static str {
        match self {
            Self::Front => "front",
            Self::Back => "back",
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Isometric => "isometric",
        }
    }

    /// Camera direction vectors: (forward, up, right).
    fn vectors(&self) -> ([f64; 3], [f64; 3], [f64; 3]) {
        match self {
            // Front: camera at +Y, looking toward -Y
            Self::Front => ([0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
            // Back: camera at -Y, looking toward +Y
            Self::Back => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0]),
            // Left: camera at -X, looking toward +X
            Self::Left => ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]),
            // Right: camera at +X, looking toward -X
            Self::Right => ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            // Top: camera at +Z, looking toward -Z
            Self::Top => ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
            // Isometric: 45° azimuth, 35.264° elevation (true isometric)
            Self::Isometric => {
                // Isometric projection: rotate 45° around Z, then tilt ~35.264° around X
                // Camera looks from upper-right-back toward origin
                let az = std::f64::consts::FRAC_PI_4; // 45°
                let el = (1.0_f64 / 2.0_f64.sqrt()).atan(); // ~35.264°
                let fwd = [-az.cos() * el.cos(), -az.sin() * el.cos(), -el.sin()];
                // Up must have positive Z so world Z-up maps to screen-up
                let up = [-az.cos() * el.sin(), -az.sin() * el.sin(), el.cos()];
                let right = [az.sin(), -az.cos(), 0.0];
                (fwd, up, right)
            }
        }
    }
}

/// Rendering style for 3D views.
#[derive(Debug, Clone, Copy)]
pub enum RenderStyle {
    /// Filled polygons with flat shading.
    Shaded,
    /// Outline edges only.
    Wireframe,
}

impl RenderStyle {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "shaded" => Some(Self::Shaded),
            "wireframe" => Some(Self::Wireframe),
            _ => None,
        }
    }
}

/// Options for 3D SVG rendering.
pub struct ViewOptions {
    /// Pixels per meter for the orthographic projection.
    pub scale: f64,
    /// SVG margin in pixels.
    pub margin: f64,
    /// Rendering style.
    pub style: RenderStyle,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            scale: 80.0,
            margin: 40.0,
            style: RenderStyle::Shaded,
        }
    }
}

/// A projected triangle for depth-sorting and rendering.
struct ProjectedTri {
    /// 2D projected vertices: [(x, y); 3]
    pts: [(f64, f64); 3],
    /// Average depth (for painter's algorithm sorting)
    depth: f64,
    /// Face color RGBA
    color: [f32; 4],
    /// Shading factor (0..1)
    shade: f64,
}

/// Render a 3D view of the scene as SVG.
pub fn render_3d_view(scene: &SceneGraph, angle: ViewAngle, options: &ViewOptions) -> String {
    let (fwd, up, right) = angle.vectors();

    // Collect all triangles, project them
    let mut tris = Vec::new();

    for node in &scene.nodes {
        collect_triangles(node, &fwd, &up, &right, &mut tris);
        for child in &node.children {
            collect_triangles(child, &fwd, &up, &right, &mut tris);
        }
    }

    // Sort by depth (back to front for painter's algorithm)
    tris.sort_by(|a, b| {
        b.depth
            .partial_cmp(&a.depth)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Compute 2D bounding box
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for tri in &tris {
        for (x, y) in &tri.pts {
            min_x = min_x.min(*x);
            max_x = max_x.max(*x);
            min_y = min_y.min(*y);
            max_y = max_y.max(*y);
        }
    }

    if min_x > max_x {
        min_x = 0.0;
        max_x = 10.0;
        min_y = 0.0;
        max_y = 10.0;
    }

    let pad = 0.5;
    min_x -= pad;
    max_x += pad;
    min_y -= pad;
    max_y += pad;

    let scale = options.scale;
    let margin = options.margin;
    let width = (max_x - min_x) * scale + 2.0 * margin;
    let height = (max_y - min_y) * scale + 2.0 * margin;

    let tx = |x: f64| -> f64 { (x - min_x) * scale + margin };
    // Flip Y so "up" in the scene maps to "up" in SVG (smaller Y)
    let ty = |y: f64| -> f64 { (max_y - y) * scale + margin };

    let mut svg = String::with_capacity(16384);

    write!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\n\
         <title>3D View: {view}</title>\n\
         <desc>BuildingML 3D {view} view. Projection: orthographic. Scale: {scale}px/m.</desc>\n\
         <rect width=\"{w}\" height=\"{h}\" fill=\"#f8f8fc\"/>\n",
        w = width,
        h = height,
        view = angle.file_suffix(),
        scale = scale,
    )
    .unwrap();

    match options.style {
        RenderStyle::Shaded => {
            svg.push_str("<g id=\"faces\">\n");
            for tri in &tris {
                let r = (tri.color[0] * tri.shade as f32 * 255.0) as u8;
                let g = (tri.color[1] * tri.shade as f32 * 255.0) as u8;
                let b = (tri.color[2] * tri.shade as f32 * 255.0) as u8;
                let a = tri.color[3];

                write!(
                    svg,
                    r#"<polygon points="{x0:.1},{y0:.1} {x1:.1},{y1:.1} {x2:.1},{y2:.1}" fill="rgb({r},{g},{b})" fill-opacity="{a}" stroke="rgb({sr},{sg},{sb})" stroke-width="0.3"/>"#,
                    x0 = tx(tri.pts[0].0), y0 = ty(tri.pts[0].1),
                    x1 = tx(tri.pts[1].0), y1 = ty(tri.pts[1].1),
                    x2 = tx(tri.pts[2].0), y2 = ty(tri.pts[2].1),
                    sr = (r as f32 * 0.7) as u8,
                    sg = (g as f32 * 0.7) as u8,
                    sb = (b as f32 * 0.7) as u8,
                )
                .unwrap();
                svg.push('\n');
            }
            svg.push_str("</g>\n");
        }
        RenderStyle::Wireframe => {
            svg.push_str("<g id=\"edges\" fill=\"none\" stroke=\"#444\" stroke-width=\"0.8\">\n");

            // Collect unique edges to avoid duplicates
            let mut edges = std::collections::HashSet::new();
            for tri in &tris {
                for i in 0..3 {
                    let j = (i + 1) % 3;
                    let (p0, p1) = (tri.pts[i], tri.pts[j]);
                    // Normalize edge direction for dedup
                    let edge = if (p0.0, p0.1) < (p1.0, p1.1) {
                        (
                            (p0.0 * 10.0) as i64,
                            (p0.1 * 10.0) as i64,
                            (p1.0 * 10.0) as i64,
                            (p1.1 * 10.0) as i64,
                        )
                    } else {
                        (
                            (p1.0 * 10.0) as i64,
                            (p1.1 * 10.0) as i64,
                            (p0.0 * 10.0) as i64,
                            (p0.1 * 10.0) as i64,
                        )
                    };
                    if edges.insert(edge) {
                        write!(
                            svg,
                            r#"<line x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}"/>"#,
                            x1 = tx(p0.0),
                            y1 = ty(p0.1),
                            x2 = tx(p1.0),
                            y2 = ty(p1.1),
                        )
                        .unwrap();
                        svg.push('\n');
                    }
                }
            }
            svg.push_str("</g>\n");
        }
    }

    // View label
    write!(
        svg,
        "<text x=\"{x}\" y=\"{y}\" font-family=\"sans-serif\" font-size=\"12\" fill=\"#666\">{label} view</text>",
        x = margin,
        y = height - 10.0,
        label = angle.file_suffix(),
    )
    .unwrap();
    svg.push('\n');

    svg.push_str("</svg>\n");
    svg
}

fn collect_triangles(
    node: &crate::scene::graph::SceneNode,
    fwd: &[f64; 3],
    up: &[f64; 3],
    right: &[f64; 3],
    tris: &mut Vec<ProjectedTri>,
) {
    let verts = &node.mesh.vertices;
    let indices = &node.mesh.indices;
    let color = node.mesh.color;

    // Light direction for shading (fixed: from upper-right-front)
    let light = [0.5_f64, 0.3, 0.8];
    let light_len = (light[0] * light[0] + light[1] * light[1] + light[2] * light[2]).sqrt();
    let light = [
        light[0] / light_len,
        light[1] / light_len,
        light[2] / light_len,
    ];

    let mut i = 0;
    while i + 2 < indices.len() {
        let i0 = indices[i] as usize;
        let i1 = indices[i + 1] as usize;
        let i2 = indices[i + 2] as usize;

        if i0 >= verts.len() || i1 >= verts.len() || i2 >= verts.len() {
            i += 3;
            continue;
        }

        let p0 = verts[i0].position;
        let p1 = verts[i1].position;
        let p2 = verts[i2].position;

        // Project each vertex using orthographic projection
        // x_proj = dot(position, right)
        // y_proj = dot(position, up)
        // depth  = dot(position, forward)
        let proj = |p: [f32; 3]| -> (f64, f64, f64) {
            let px = p[0] as f64;
            let py = p[1] as f64;
            let pz = p[2] as f64;
            let x = right[0] * px + right[1] * py + right[2] * pz;
            let y = up[0] * px + up[1] * py + up[2] * pz;
            let d = fwd[0] * px + fwd[1] * py + fwd[2] * pz;
            (x, y, d)
        };

        let (x0, y0, d0) = proj(p0);
        let (x1, y1, d1) = proj(p1);
        let (x2, y2, d2) = proj(p2);

        // Back-face culling: check if triangle faces the camera
        let e1x = x1 - x0;
        let e1y = y1 - y0;
        let e2x = x2 - x0;
        let e2y = y2 - y0;
        let cross = e1x * e2y - e1y * e2x;
        if cross < 0.0 {
            i += 3;
            continue; // back-facing
        }

        // Compute face normal for shading
        let n = verts[i0].normal;
        let ndot =
            (n[0] as f64 * light[0] + n[1] as f64 * light[1] + n[2] as f64 * light[2]).max(0.0);
        let shade = 0.3 + 0.7 * ndot;

        tris.push(ProjectedTri {
            pts: [(x0, y0), (x1, y1), (x2, y2)],
            depth: (d0 + d1 + d2) / 3.0,
            color,
            shade,
        });

        i += 3;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cst_to_ast;
    use crate::scene;
    use crate::semantic;

    #[test]
    fn test_view_angle_parse() {
        assert!(matches!(ViewAngle::parse("front"), Some(ViewAngle::Front)));
        assert!(matches!(
            ViewAngle::parse("iso"),
            Some(ViewAngle::Isometric)
        ));
        assert!(matches!(ViewAngle::parse("top"), Some(ViewAngle::Top)));
        assert!(ViewAngle::parse("invalid").is_none());
    }

    #[test]
    fn test_3d_view_basic() {
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
        let scene_graph = scene::generate_scene(&model);

        let svg = render_3d_view(&scene_graph, ViewAngle::Front, &ViewOptions::default());
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
        assert!(svg.contains("front view"));
    }

    #[test]
    fn test_3d_view_isometric() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.24 m
  Wall: W2
    from: (6, 0) to: (6, 5)
    height: 2.8 m
    thickness: 0.24 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene_graph = scene::generate_scene(&model);

        let svg = render_3d_view(&scene_graph, ViewAngle::Isometric, &ViewOptions::default());
        assert!(svg.contains("<svg"));
        assert!(svg.contains("isometric view"));
    }

    #[test]
    fn test_3d_view_wireframe() {
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
        let scene_graph = scene::generate_scene(&model);

        let opts = ViewOptions {
            style: RenderStyle::Wireframe,
            ..ViewOptions::default()
        };
        let svg = render_3d_view(&scene_graph, ViewAngle::Front, &opts);
        assert!(svg.contains("<svg"));
        assert!(svg.contains("id=\"edges\""));
    }

    #[test]
    fn test_all_view_angles() {
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
        let scene_graph = scene::generate_scene(&model);

        for angle in [
            ViewAngle::Front,
            ViewAngle::Back,
            ViewAngle::Left,
            ViewAngle::Right,
            ViewAngle::Top,
            ViewAngle::Isometric,
        ] {
            let svg = render_3d_view(&scene_graph, angle, &ViewOptions::default());
            assert!(svg.contains("<svg"), "missing svg for {:?}", angle);
            assert!(svg.contains("</svg>"), "incomplete svg for {:?}", angle);
        }
    }
}
