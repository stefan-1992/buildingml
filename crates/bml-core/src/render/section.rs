use crate::scene::graph::{SceneGraph, SceneMesh, SceneNode, Vertex};

/// A clipping plane for section cuts.
#[derive(Debug, Clone)]
pub enum ClipPlane {
    /// Horizontal cut: remove everything above `z`.
    HorizontalAbove(f32),
    /// Horizontal cut: remove everything below `z`.
    HorizontalBelow(f32),
    /// Vertical cut: remove everything on the positive side of a plane at x=`value`.
    VerticalX(f32),
    /// Vertical cut: remove everything on the positive side of a plane at y=`value`.
    VerticalY(f32),
}

impl ClipPlane {
    /// Returns true if the point is on the "keep" side (inside).
    fn is_inside(&self, p: &[f32; 3]) -> bool {
        match self {
            ClipPlane::HorizontalAbove(z) => p[2] <= *z,
            ClipPlane::HorizontalBelow(z) => p[2] >= *z,
            ClipPlane::VerticalX(x) => p[0] <= *x,
            ClipPlane::VerticalY(y) => p[1] <= *y,
        }
    }

    /// Interpolate between two points at the plane boundary.
    fn intersect(&self, a: &Vertex, b: &Vertex) -> Vertex {
        let t = match self {
            ClipPlane::HorizontalAbove(z) | ClipPlane::HorizontalBelow(z) => {
                (z - a.position[2]) / (b.position[2] - a.position[2])
            }
            ClipPlane::VerticalX(x) => (x - a.position[0]) / (b.position[0] - a.position[0]),
            ClipPlane::VerticalY(y) => (y - a.position[1]) / (b.position[1] - a.position[1]),
        };
        let t = t.clamp(0.0, 1.0);
        Vertex {
            position: [
                a.position[0] + t * (b.position[0] - a.position[0]),
                a.position[1] + t * (b.position[1] - a.position[1]),
                a.position[2] + t * (b.position[2] - a.position[2]),
            ],
            normal: [
                a.normal[0] + t * (b.normal[0] - a.normal[0]),
                a.normal[1] + t * (b.normal[1] - a.normal[1]),
                a.normal[2] + t * (b.normal[2] - a.normal[2]),
            ],
        }
    }
}

/// Clip a scene graph at the given plane, returning a new scene with clipped geometry.
pub fn clip_scene(scene: &SceneGraph, plane: &ClipPlane) -> SceneGraph {
    let nodes = scene
        .nodes
        .iter()
        .filter_map(|node| clip_node(node, plane))
        .collect();
    SceneGraph {
        nodes,
        materials: scene.materials.clone(),
    }
}

fn clip_node(node: &SceneNode, plane: &ClipPlane) -> Option<SceneNode> {
    let clipped_mesh = clip_mesh(&node.mesh, plane);

    let children = node
        .children
        .iter()
        .filter_map(|child| clip_node(child, plane))
        .collect();

    // Keep node if it has any geometry or children
    if clipped_mesh.vertices.is_empty() && clipped_mesh.indices.is_empty() {
        // Check if any children survived
        let children: Vec<SceneNode> = children;
        if children.is_empty() {
            return None;
        }
        return Some(SceneNode {
            id: node.id.clone(),
            element_type: node.element_type.clone(),
            label: node.label.clone(),
            mesh: clipped_mesh,
            children,
            properties: node.properties.clone(),
            material: node.material,
        });
    }

    Some(SceneNode {
        id: node.id.clone(),
        element_type: node.element_type.clone(),
        label: node.label.clone(),
        mesh: clipped_mesh,
        children,
        properties: node.properties.clone(),
        material: node.material,
    })
}

fn clip_mesh(mesh: &SceneMesh, plane: &ClipPlane) -> SceneMesh {
    let mut new_vertices = Vec::new();
    let mut new_indices = Vec::new();

    let verts = &mesh.vertices;
    let indices = &mesh.indices;

    let mut i = 0;
    while i + 2 < indices.len() {
        let i0 = indices[i] as usize;
        let i1 = indices[i + 1] as usize;
        let i2 = indices[i + 2] as usize;
        i += 3;

        if i0 >= verts.len() || i1 >= verts.len() || i2 >= verts.len() {
            continue;
        }

        let v0 = &verts[i0];
        let v1 = &verts[i1];
        let v2 = &verts[i2];

        let in0 = plane.is_inside(&v0.position);
        let in1 = plane.is_inside(&v1.position);
        let in2 = plane.is_inside(&v2.position);

        let inside_count = in0 as u8 + in1 as u8 + in2 as u8;

        match inside_count {
            3 => {
                // All inside: keep triangle
                let base = new_vertices.len() as u32;
                new_vertices.push(*v0);
                new_vertices.push(*v1);
                new_vertices.push(*v2);
                new_indices.extend_from_slice(&[base, base + 1, base + 2]);
            }
            0 => {
                // All outside: discard
            }
            2 => {
                // Two inside, one outside: clip to quad (two triangles)
                let (a, b, c) = if !in0 {
                    (v0, v1, v2)
                } else if !in1 {
                    (v1, v2, v0)
                } else {
                    (v2, v0, v1)
                };
                // a is outside, b and c are inside
                let ab = plane.intersect(a, b);
                let ac = plane.intersect(a, c);

                let base = new_vertices.len() as u32;
                new_vertices.push(ab);
                new_vertices.push(*b);
                new_vertices.push(*c);
                new_vertices.push(ac);
                new_indices.extend_from_slice(&[base, base + 1, base + 2]);
                new_indices.extend_from_slice(&[base, base + 2, base + 3]);
            }
            1 => {
                // One inside, two outside: clip to single triangle
                let (a, b, c) = if in0 {
                    (v0, v1, v2)
                } else if in1 {
                    (v1, v2, v0)
                } else {
                    (v2, v0, v1)
                };
                // a is inside, b and c are outside
                let ab = plane.intersect(a, b);
                let ac = plane.intersect(a, c);

                let base = new_vertices.len() as u32;
                new_vertices.push(*a);
                new_vertices.push(ab);
                new_vertices.push(ac);
                new_indices.extend_from_slice(&[base, base + 1, base + 2]);
            }
            _ => unreachable!(),
        }
    }

    SceneMesh {
        vertices: new_vertices,
        indices: new_indices,
        color: mesh.color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cst_to_ast;
    use crate::scene;
    use crate::semantic;

    #[test]
    fn test_clip_horizontal_above() {
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
        let scene_graph = scene::generate_scene(&model);

        // Cut at 1.5m — should keep lower portion
        let clipped = clip_scene(&scene_graph, &ClipPlane::HorizontalAbove(1.5));
        assert!(!clipped.nodes.is_empty(), "should have clipped nodes");

        // All vertex Z should be <= 1.5
        for node in &clipped.nodes {
            for v in &node.mesh.vertices {
                assert!(
                    v.position[2] <= 1.501, // small epsilon
                    "vertex z={} exceeds cut plane 1.5",
                    v.position[2]
                );
            }
        }
    }

    #[test]
    fn test_clip_removes_all() {
        let source = "\
# Site: S
## Building: B
### Storey: FF
  elevation: 5.0 m
  Wall: W1
    from: (0, 0) to: (4, 0)
    height: 2.8 m
    thickness: 0.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene_graph = scene::generate_scene(&model);

        // Cut below the entire wall
        let clipped = clip_scene(&scene_graph, &ClipPlane::HorizontalAbove(2.0));
        assert!(
            clipped.nodes.is_empty(),
            "wall at elevation 5m should be completely removed by cut at z=2"
        );
    }

    #[test]
    fn test_clip_keeps_all() {
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

        let orig_tri_count: usize = scene_graph
            .nodes
            .iter()
            .map(|n| n.mesh.indices.len() / 3)
            .sum();

        // Cut above the entire wall
        let clipped = clip_scene(&scene_graph, &ClipPlane::HorizontalAbove(10.0));
        let clipped_tri_count: usize = clipped.nodes.iter().map(|n| n.mesh.indices.len() / 3).sum();

        assert_eq!(
            orig_tri_count, clipped_tri_count,
            "nothing should be clipped"
        );
    }

    #[test]
    fn test_clip_vertical() {
        let source = "\
# Site: S
## Building: B
### Storey: GF
  elevation: 0 m
  Wall: W1
    from: (0, 0) to: (10, 0)
    height: 2.8 m
    thickness: 0.2 m
";
        let ast = cst_to_ast::parse(source).unwrap();
        let (model, _) = semantic::analyze(&ast);
        let scene_graph = scene::generate_scene(&model);

        // Cut at x=5
        let clipped = clip_scene(&scene_graph, &ClipPlane::VerticalX(5.0));
        assert!(!clipped.nodes.is_empty());

        for node in &clipped.nodes {
            for v in &node.mesh.vertices {
                assert!(
                    v.position[0] <= 5.001,
                    "vertex x={} exceeds cut plane 5.0",
                    v.position[0]
                );
            }
        }
    }
}
