mod commands;
mod output;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "buildingml", about = "BuildingML compiler and tools")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Analyze IFC files for import coverage (entity frequency, BML mapping gaps)
    Analyze {
        /// Input IFC file(s)
        inputs: Vec<PathBuf>,
        /// Output format: text, json
        #[arg(short, long, default_value = "text")]
        format: Option<String>,
        /// Output file path (prints to stdout if not specified)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Compile BuildingML files to IFC
    Compile {
        /// Input file or directory
        input: PathBuf,
        /// Output IFC file path
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Lint BuildingML files
    Lint {
        /// Input file or directory
        input: PathBuf,
    },
    /// Run compliance checks on BuildingML files
    Check {
        /// Input file or directory
        input: PathBuf,
        /// Check suite to run (runs all if not specified)
        #[arg(short, long)]
        suite: Option<String>,
        /// Output format: text, json, html, sarif
        #[arg(short, long, default_value = "text")]
        format: Option<String>,
        /// Output file path (prints to stdout if not specified)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Decompile IFC files to BuildingML
    Decompile {
        /// Input IFC file
        input: PathBuf,
        /// Output directory
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Start the Language Server Protocol server (for editor integration)
    Lsp {
        /// Use stdio transport (default, accepted for VS Code compatibility)
        #[arg(long)]
        stdio: bool,
    },
    /// Start a 3D preview server for BuildingML files
    Preview {
        /// Input file or directory
        input: PathBuf,
        /// Port to serve on
        #[arg(short, long, default_value = "8080")]
        port: u16,
        /// Serve the React web app from this directory instead of embedded legacy HTML
        #[arg(long)]
        web_dir: Option<PathBuf>,
        /// Directory containing .bml example files to serve via /api/examples
        #[arg(long)]
        examples_dir: Option<PathBuf>,
    },
    /// Start a production server (POST-only API, no file access)
    Serve {
        /// Port to serve on
        #[arg(short, long, default_value = "3001")]
        port: u16,
        /// Bind address
        #[arg(short, long, default_value = "0.0.0.0")]
        bind: String,
        /// Web app directory (must contain index.html)
        #[arg(long)]
        web_dir: PathBuf,
        /// Directory containing .bml example files to serve via /api/examples
        #[arg(long)]
        examples_dir: Option<PathBuf>,
    },
    /// Output semantic model as JSON from BuildingML files
    Model {
        /// Input file or directory
        input: PathBuf,
    },
    /// Generate 3D scene graph JSON from BuildingML files
    Scene {
        /// Input file or directory
        input: PathBuf,
    },
    /// Generate 3D scene graph JSON directly from an IFC file (fast native reader)
    IfcScene {
        /// Input IFC file
        input: PathBuf,
    },
    /// Create a new BuildingML project
    Init {
        /// Project name (directory to create)
        name: Option<String>,
        /// Template to use: residential, office, mixed-use
        #[arg(short, long, default_value = "residential")]
        template: Option<String>,
        /// Create flat structure (no subdirectories)
        #[arg(long)]
        flat: bool,
        /// Overwrite existing directory
        #[arg(long)]
        force: bool,
    },
    /// Render SVG images (floorplans and 3D views) from BuildingML files
    Render {
        /// Input file or directory
        input: PathBuf,
        /// Output directory for SVG files
        #[arg(short, long, default_value = "render-output")]
        output: PathBuf,
        /// Generate per-storey floorplan SVGs
        #[arg(long)]
        floorplans: bool,
        /// 3D view angles to render (comma-separated: front,back,left,right,top,iso)
        #[arg(long)]
        views: Option<String>,
        /// Generate all floorplans and all standard 3D views
        #[arg(long)]
        all: bool,
        /// 3D rendering style: shaded or wireframe (default: shaded)
        #[arg(long)]
        style: Option<String>,
        /// Section cut height for floorplans in meters above floor (default: 1.0)
        #[arg(long)]
        cut_height: Option<f64>,
        /// Horizontal clip elevation for 3D views in meters (removes geometry above)
        #[arg(long)]
        clip_elevation: Option<f64>,
        /// Scale in pixels per meter (default: 100 for floorplans, 80 for 3D views)
        #[arg(long)]
        scale: Option<f64>,
    },
}

fn main() -> miette::Result<()> {
    // Load .env file if present (silently ignored in production)
    dotenvy::dotenv().ok();

    let cli = Cli::parse();
    match cli.command {
        Commands::Analyze {
            inputs,
            format,
            output,
        } => commands::analyze::run(inputs, format, output),
        Commands::Compile { input, output } => commands::compile::run(input, output),
        Commands::Lint { input } => commands::lint::run(input),
        Commands::Check {
            input,
            suite,
            format,
            output,
        } => commands::check::run(input, suite, format, output),
        Commands::Decompile { input, output } => commands::decompile::run(input, output),
        Commands::Lsp { .. } => {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(bml_lsp::server::run());
            Ok(())
        }
        Commands::Preview {
            input,
            port,
            web_dir,
            examples_dir,
        } => commands::preview::run(input, port, web_dir, examples_dir),
        Commands::Serve {
            port,
            bind,
            web_dir,
            examples_dir,
        } => commands::serve::run(port, bind, web_dir, examples_dir),
        Commands::Init {
            name,
            template,
            flat,
            force,
        } => commands::init::run(name, template, flat, force),
        Commands::Model { input } => commands::model::run(input),
        Commands::Scene { input } => commands::scene::run(input),
        Commands::IfcScene { input } => commands::ifc_scene::run(input),
        Commands::Render {
            input,
            output,
            floorplans,
            views,
            all,
            style,
            cut_height,
            clip_elevation,
            scale,
        } => commands::render::run(commands::render::RenderArgs {
            input,
            output,
            floorplans,
            views,
            all,
            style,
            cut_height,
            clip_elevation,
            scale,
        }),
    }
}
