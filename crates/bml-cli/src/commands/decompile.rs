use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use bml_core::decompile::{self, bml_writer, mesh_fit, styles};
use bml_core::semantic::model::{ResolvedElement, ResolvedProperty, ResolvedValue, SemanticModel};
use ifc_lite_core::{EntityDecoder, EntityIndex, EntityScanner};
use ifc_lite_geometry::GeometryRouter;

use bml_core::scene::SceneMaterial;

use super::ifc_scene::{length_unit_scale, repair, weld};
use super::ifc_styles::StyleIndex;

pub fn run(input: PathBuf, output_path: Option<PathBuf>) -> miette::Result<()> {
    let ifc_content = std::fs::read_to_string(&input)
        .map_err(|e| miette::miette!("failed to read {}: {}", input.display(), e))?;

    let mut model = decompile::decompile_ifc(&ifc_content)
        .map_err(|e| miette::miette!("decompilation failed: {}", e))?;
    // Elements placed via an undefined entity fail to mesh; as in `ifc-scene`,
    // such references become `$`.
    let (content, index) = repair(ifc_content.into_bytes());
    let (meshes, element_styles, style_index) = meshes(&content, index, &guids(&model));
    let styled = styles::apply_styles(&mut model, &element_styles, |name| {
        style_index.for_material_name(name).cloned()
    });
    let fitted = mesh_fit::fit_meshes(&mut model, &meshes);
    let bml_output = bml_writer::generate_bml(&model);

    // Determine output path
    let output_dir = output_path.unwrap_or_else(|| {
        let stem = input.file_stem().unwrap_or_default().to_string_lossy();
        PathBuf::from(format!("{}-bml", stem))
    });

    // Create output directory if it doesn't exist
    std::fs::create_dir_all(&output_dir).map_err(|e| {
        miette::miette!("failed to create directory {}: {}", output_dir.display(), e)
    })?;

    // Write the BML output as a single file
    let output_file = output_dir.join("project.bml");
    std::fs::write(&output_file, &bml_output)
        .map_err(|e| miette::miette!("failed to write {}: {}", output_file.display(), e))?;

    // Write a default buildingml.toml
    let config_content = r#"buildingml_version = "0.1"
ifc_version = "IFC4"
output_format = "step"
entry = "project.bml"

[units]
length = "m"
area = "m2"
angle = "deg"
"#;
    let config_file = output_dir.join("buildingml.toml");
    std::fs::write(&config_file, config_content)
        .map_err(|e| miette::miette!("failed to write {}: {}", config_file.display(), e))?;

    eprintln!(
        "\x1b[32mDecompiled\x1b[0m {} -> {} ({} bytes, {} of {} meshed elements fitted to their mesh, {} of {} materials styled)",
        input.display(),
        output_file.display(),
        bml_output.len(),
        fitted,
        meshes.len(),
        styled,
        model.materials.len()
    );

    Ok(())
}

/// GlobalIds of all elements and spaces in the model.
fn guids(model: &SemanticModel) -> HashSet<String> {
    fn add(props: &[ResolvedProperty], out: &mut HashSet<String>) {
        if let Some(ResolvedValue::String(g)) =
            props.iter().find(|p| p.key == "guid").map(|p| &p.value)
        {
            out.insert(g.clone());
        }
    }
    fn walk(e: &ResolvedElement, out: &mut HashSet<String>) {
        add(&e.properties, out);
        e.children.iter().for_each(|c| walk(c, out));
    }
    let mut out = HashSet::new();
    model.elements().for_each(|e| walk(e, &mut out));
    model.spaces().for_each(|s| add(&s.properties, &mut out));
    out
}

/// World meshes (metres) of the IFC entities whose GlobalId is in `guids`, the
/// surface style covering most of each one's area, and the file's style index.
fn meshes(
    content: &[u8],
    index: EntityIndex,
    guids: &HashSet<String>,
) -> (
    HashMap<String, mesh_fit::Mesh>,
    HashMap<String, SceneMaterial>,
    StyleIndex,
) {
    // GlobalId is the first attribute: `#12=IFCWALL('<22 chars>',...`.
    let mut wanted: Vec<(u32, String)> = Vec::new();
    let mut project = None;
    let mut scanner = EntityScanner::new(content);
    while let Some((id, type_name, start, end)) = scanner.next_entity() {
        if type_name == "IFCPROJECT" {
            project.get_or_insert(id);
        }
        let text = &content[start..end];
        if let Some(open) = text.iter().position(|&b| b == b'(')
            && text.get(open + 1) == Some(&b'\'')
            && text.get(open + 24) == Some(&b'\'')
            && let Ok(g) = std::str::from_utf8(&text[open + 2..open + 24])
            && guids.contains(g)
        {
            wanted.push((id, g.to_string()));
        }
    }
    let mut decoder = EntityDecoder::with_index(content, index);
    let styles = StyleIndex::build(content, &mut decoder);
    let scale = length_unit_scale(project, &mut decoder) as f32;
    let router = GeometryRouter::new();
    let mut out = HashMap::new();
    let mut element_styles = HashMap::new();
    for (id, guid) in wanted {
        let Ok(element) = decoder.decode_by_id(id) else {
            continue;
        };
        // Per-item sub-meshes, as the IFC preview draws them, so each piece of
        // geometry can be weighed by the style it is drawn with.
        let subs: Vec<_> = match router.process_element_with_submeshes(&element, &mut decoder) {
            Ok(subs) if !subs.is_empty() => subs
                .iter()
                .map(|s| {
                    (
                        styles.for_item(s.geometry_id),
                        weld(&s.mesh.positions, &s.mesh.indices, scale, s.mesh.origin),
                    )
                })
                .collect(),
            _ => match router.process_element(&element, &mut decoder) {
                Ok(m) => vec![(None, weld(&m.positions, &m.indices, scale, m.origin))],
                Err(_) => continue,
            },
        };
        let mut mesh = mesh_fit::Mesh {
            positions: Vec::new(),
            indices: Vec::new(),
        };
        // Ordered, so a tie between styles resolves the same way on every run.
        let mut area_by_style: BTreeMap<u32, f32> = BTreeMap::new();
        for (style, (positions, indices)) in subs {
            if let Some(s) = style.or_else(|| styles.for_product(id)) {
                *area_by_style.entry(s).or_default() += area(&positions, &indices);
            }
            let base = mesh.positions.len() as u32;
            mesh.positions.extend(positions);
            mesh.indices.extend(indices.iter().map(|i| i + base));
        }
        if mesh.indices.is_empty() {
            continue;
        }
        if let Some((&s, _)) = area_by_style.iter().max_by(|a, b| a.1.total_cmp(b.1))
            && let Some(look) = styles.get(s)
        {
            element_styles.insert(guid.clone(), look.clone());
        }
        out.insert(guid, mesh);
    }
    (out, element_styles, styles)
}

/// Surface area of a triangle mesh.
fn area(positions: &[[f32; 3]], indices: &[u32]) -> f32 {
    indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let [a, b, c] = [0, 1, 2].map(|k| positions[t[k] as usize]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() / 2.0
        })
        .sum()
}
