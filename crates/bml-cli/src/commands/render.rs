use std::path::PathBuf;

use bml_core::project::Project;
use bml_core::render::floorplan::{self, FloorplanOptions};
use bml_core::render::section::{self, ClipPlane};
use bml_core::render::view3d::{self, RenderStyle, ViewAngle, ViewOptions};
use bml_core::scene;

pub struct RenderArgs {
    pub input: PathBuf,
    pub output: PathBuf,
    pub floorplans: bool,
    pub views: Option<String>,
    pub all: bool,
    pub style: Option<String>,
    pub cut_height: Option<f64>,
    pub clip_elevation: Option<f64>,
    pub scale: Option<f64>,
}

pub fn run(args: RenderArgs) -> miette::Result<()> {
    let RenderArgs {
        input,
        output,
        floorplans,
        views,
        all,
        style,
        cut_height,
        clip_elevation,
        scale,
    } = args;
    // Parse the model
    let (model, scene_graph) = if input.is_dir() {
        let project =
            Project::load(&input).map_err(|e| miette::miette!("failed to load project: {}", e))?;
        let (model, diagnostics) = project.analyze();
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == bml_core::semantic::diagnostics::Severity::Error)
            .collect();
        if !errors.is_empty() {
            for e in &errors {
                eprintln!("  \x1b[31merror\x1b[0m [{}]: {}", e.code, e.message);
            }
            return Err(miette::miette!("{} error(s) in project", errors.len()));
        }
        let sg = scene::generate_scene(&model);
        (model, sg)
    } else {
        let source = std::fs::read_to_string(&input)
            .map_err(|e| miette::miette!("failed to read {}: {}", input.display(), e))?;
        let (model, diagnostics) = bml_core::project::analyze_source(Some(&input), &source)
            .map_err(|errors| {
                miette::miette!(
                    "Parse error(s): {}",
                    errors
                        .iter()
                        .map(|e| e.to_string())
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            })?;
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == bml_core::semantic::diagnostics::Severity::Error)
            .collect();
        if !errors.is_empty() {
            for e in &errors {
                eprintln!("  \x1b[31merror\x1b[0m [{}]: {}", e.code, e.message);
            }
            return Err(miette::miette!("{} error(s)", errors.len()));
        }
        let sg = scene::generate_scene(&model);
        (model, sg)
    };

    // Create output directory
    std::fs::create_dir_all(&output)
        .map_err(|e| miette::miette!("failed to create output directory: {}", e))?;

    let render_style = style
        .as_deref()
        .map(|s| {
            RenderStyle::parse(s).ok_or_else(|| {
                miette::miette!(
                    "unknown render style '{}', expected 'shaded' or 'wireframe'",
                    s
                )
            })
        })
        .transpose()?
        .unwrap_or(RenderStyle::Shaded);

    let fp_scale = scale.unwrap_or(100.0);
    let view_scale = scale.unwrap_or(80.0);

    let generate_floorplans = floorplans || all;
    let generate_views = views.is_some() || all;

    let view_angles: Vec<ViewAngle> = if all {
        vec![
            ViewAngle::Front,
            ViewAngle::Back,
            ViewAngle::Left,
            ViewAngle::Right,
            ViewAngle::Top,
            ViewAngle::Isometric,
        ]
    } else if let Some(ref views_str) = views {
        views_str
            .split(',')
            .map(|s| {
                ViewAngle::parse(s.trim()).ok_or_else(|| {
                    miette::miette!(
                        "unknown view angle '{}'. Valid: front, back, left, right, top, iso",
                        s.trim()
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };

    if !generate_floorplans && !generate_views {
        return Err(miette::miette!(
            "nothing to render. Use --floorplans, --views, or --all"
        ));
    }

    let mut file_count = 0;

    // Generate floorplans
    if generate_floorplans {
        let storey_names = floorplan::storey_names(&model);
        let options = FloorplanOptions {
            cut_height: cut_height.unwrap_or(1.0),
            scale: fp_scale,
            show_dimensions: true,
            show_grid: true,
            margin: 40.0,
        };

        let mut used = std::collections::HashSet::new();
        for (i, name) in storey_names.iter().enumerate() {
            if let Some(svg) = floorplan::render_floorplan(&model, i, &options) {
                // Plans can share a name (a storey named like its building)
                let mut filename = format!("floorplan-{}.svg", slugify(name));
                let mut n = 1;
                while !used.insert(filename.clone()) {
                    filename = format!("floorplan-{}-{}.svg", slugify(name), n);
                    n += 1;
                }
                let path = output.join(&filename);
                std::fs::write(&path, &svg)
                    .map_err(|e| miette::miette!("failed to write {}: {}", path.display(), e))?;
                eprintln!("  \x1b[32mwrote\x1b[0m {}", path.display());
                file_count += 1;
            }
        }
    }

    // Generate 3D views
    if generate_views {
        // Apply section cut if requested
        let active_scene = if let Some(clip_z) = clip_elevation {
            section::clip_scene(&scene_graph, &ClipPlane::HorizontalAbove(clip_z as f32))
        } else {
            scene_graph
        };

        let view_opts = ViewOptions {
            scale: view_scale,
            margin: 40.0,
            style: render_style,
        };

        for angle in &view_angles {
            let svg = view3d::render_3d_view(&active_scene, *angle, &view_opts);
            let filename = format!("view-{}.svg", angle.file_suffix());
            let path = output.join(&filename);
            std::fs::write(&path, &svg)
                .map_err(|e| miette::miette!("failed to write {}: {}", path.display(), e))?;
            eprintln!("  \x1b[32mwrote\x1b[0m {}", path.display());
            file_count += 1;
        }
    }

    eprintln!(
        "\x1b[32mRendered {} file(s)\x1b[0m to {}",
        file_count,
        output.display()
    );
    Ok(())
}

/// Convert a name to a URL-safe slug.
fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
