use std::io::{self, Write};
use std::path::{Path, PathBuf};

mod templates;

pub fn run(
    name: Option<String>,
    template: Option<String>,
    flat: bool,
    force: bool,
) -> miette::Result<()> {
    // Resolve project name — interactive if not provided
    let project_name = match name {
        Some(n) => n,
        None => prompt_interactive()?,
    };

    validate_name(&project_name)?;

    let template_name = template.unwrap_or_else(|| "residential".to_string());
    let template = resolve_template(&template_name)?;

    let project_dir = PathBuf::from(&project_name);

    // Check if directory already exists
    if project_dir.exists() {
        if !force {
            return Err(miette::miette!(
                "Directory '{}' already exists. Use --force to overwrite.",
                project_name
            ));
        }
        // With --force, verify it looks like a BuildingML project before deleting
        if project_dir.is_dir() && !project_dir.join("buildingml.toml").exists() {
            return Err(miette::miette!(
                "Directory '{}' exists but does not contain a buildingml.toml file. \
                 Refusing to delete — it may not be a BuildingML project. \
                 Remove it manually if you want to proceed.",
                project_name
            ));
        }
        std::fs::remove_dir_all(&project_dir)
            .map_err(|e| miette::miette!("Failed to remove existing directory: {}", e))?;
    }

    // Create project structure
    if flat {
        create_flat_project(&project_dir, &project_name, template)?;
    } else {
        create_nested_project(&project_dir, &project_name, template)?;
    }

    eprintln!(
        "\x1b[32mCreated\x1b[0m project '{}' with {} template ({})",
        project_name,
        template_name,
        if flat { "flat" } else { "nested" }
    );
    eprintln!();
    eprintln!("  cd {}", project_name);
    eprintln!("  buildingml lint project.bml");
    eprintln!("  buildingml compile . -o output.ifc");

    Ok(())
}

/// Interactive mode: prompt for project name, template, and structure.
fn prompt_interactive() -> miette::Result<String> {
    let stdin = io::stdin();

    // Project name
    eprint!("Project name: ");
    io::stderr().flush().ok();
    let mut name = String::new();
    stdin
        .read_line(&mut name)
        .map_err(|e| miette::miette!("Failed to read input: {}", e))?;
    let name = name.trim().to_string();

    if name.is_empty() {
        return Err(miette::miette!("Project name cannot be empty"));
    }

    Ok(name)
}

fn validate_name(name: &str) -> miette::Result<()> {
    if name.is_empty() {
        return Err(miette::miette!("Project name cannot be empty"));
    }

    // Allow alphanumeric, hyphens, underscores
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(miette::miette!(
            "Project name must contain only alphanumeric characters, hyphens (-), and underscores (_)"
        ));
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    Residential,
    Office,
    MixedUse,
}

fn resolve_template(name: &str) -> miette::Result<Template> {
    match name {
        "residential" => Ok(Template::Residential),
        "office" => Ok(Template::Office),
        "mixed-use" | "mixed_use" | "mixeduse" => Ok(Template::MixedUse),
        _ => Err(miette::miette!(
            "Unknown template: '{}'. Available: residential, office, mixed-use",
            name
        )),
    }
}

fn create_nested_project(dir: &Path, project_name: &str, template: Template) -> miette::Result<()> {
    // Create directories
    create_dir(dir)?;
    create_dir(&dir.join("storeys"))?;
    create_dir(&dir.join("checks"))?;
    create_dir(&dir.join("assets"))?;

    // Generate files
    let display_name = to_display_name(project_name);

    write_file(
        &dir.join("buildingml.toml"),
        &templates::buildingml_toml(&display_name),
    )?;
    write_file(&dir.join(".gitignore"), templates::GITIGNORE)?;
    write_file(
        &dir.join("project.bml"),
        &templates::project_bml_nested(&display_name, template),
    )?;
    write_file(
        &dir.join("materials.bml"),
        templates::materials_bml(template),
    )?;

    // Generate storey files in storeys/ subdirectory
    for (filename, content) in templates::storey_files(template) {
        write_file(&dir.join("storeys").join(filename), content)?;
    }

    // Generate templates.bml for mixed-use (has floor template)
    if let Some(tmpl_content) = templates::templates_bml(template) {
        write_file(&dir.join("templates.bml"), tmpl_content)?;
    }

    write_file(
        &dir.join("checks/compliance.bml"),
        templates::compliance_bml(template),
    )?;
    write_file(
        &dir.join("assets/furniture.bml"),
        templates::FURNITURE_ASSETS,
    )?;

    Ok(())
}

fn create_flat_project(dir: &Path, project_name: &str, template: Template) -> miette::Result<()> {
    create_dir(dir)?;

    let display_name = to_display_name(project_name);

    write_file(
        &dir.join("buildingml.toml"),
        &templates::buildingml_toml(&display_name),
    )?;
    write_file(&dir.join(".gitignore"), templates::GITIGNORE)?;
    write_file(
        &dir.join("project.bml"),
        &templates::project_bml_flat(&display_name, template),
    )?;
    write_file(
        &dir.join("materials.bml"),
        templates::materials_bml(template),
    )?;

    write_file(&dir.join("checks.bml"), templates::compliance_bml(template))?;

    Ok(())
}

fn create_dir(path: &Path) -> miette::Result<()> {
    std::fs::create_dir_all(path)
        .map_err(|e| miette::miette!("Failed to create directory '{}': {}", path.display(), e))
}

fn write_file(path: &Path, content: &str) -> miette::Result<()> {
    std::fs::write(path, content)
        .map_err(|e| miette::miette!("Failed to write '{}': {}", path.display(), e))
}

/// Convert "my-project" or "my_project" to "My Project"
fn to_display_name(name: &str) -> String {
    name.split(['-', '_'])
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    let upper: String = first.to_uppercase().collect();
                    upper + chars.as_str()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_validate_name_valid() {
        assert!(validate_name("my-project").is_ok());
        assert!(validate_name("my_project").is_ok());
        assert!(validate_name("project123").is_ok());
        assert!(validate_name("MyProject").is_ok());
    }

    #[test]
    fn test_validate_name_invalid() {
        assert!(validate_name("").is_err());
        assert!(validate_name("my project").is_err());
        assert!(validate_name("my/project").is_err());
        assert!(validate_name("my.project").is_err());
    }

    #[test]
    fn test_resolve_template() {
        assert_eq!(
            resolve_template("residential").unwrap(),
            Template::Residential
        );
        assert_eq!(resolve_template("office").unwrap(), Template::Office);
        assert_eq!(resolve_template("mixed-use").unwrap(), Template::MixedUse);
        assert_eq!(resolve_template("mixed_use").unwrap(), Template::MixedUse);
        assert!(resolve_template("unknown").is_err());
    }

    #[test]
    fn test_to_display_name() {
        assert_eq!(to_display_name("my-project"), "My Project");
        assert_eq!(to_display_name("my_project"), "My Project");
        assert_eq!(to_display_name("simple"), "Simple");
    }

    #[test]
    fn test_init_residential_nested() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("test-project");
        create_nested_project(&project_dir, "test-project", Template::Residential).unwrap();

        assert!(project_dir.join("buildingml.toml").exists());
        assert!(project_dir.join("project.bml").exists());
        assert!(project_dir.join("materials.bml").exists());
        assert!(project_dir.join(".gitignore").exists());
        assert!(project_dir.join("storeys/ground-floor.bml").exists());
        assert!(project_dir.join("checks/compliance.bml").exists());
        assert!(project_dir.join("assets/furniture.bml").exists());

        // Verify buildingml.toml content
        let toml_content = fs::read_to_string(project_dir.join("buildingml.toml")).unwrap();
        assert!(toml_content.contains("buildingml_version"));
        assert!(toml_content.contains("project.bml"));

        // Verify project.bml imports storeys (not inline content)
        let project_content = fs::read_to_string(project_dir.join("project.bml")).unwrap();
        assert!(project_content.contains("import ground from \"./storeys/ground-floor.bml\""));
        assert!(project_content.contains("import materials"));
        assert!(
            !project_content.contains("### Storey:"),
            "Storey content should be in storeys/ files, not project.bml"
        );

        // Verify storey file contains building content
        let storey_content =
            fs::read_to_string(project_dir.join("storeys/ground-floor.bml")).unwrap();
        assert!(storey_content.contains("### Storey: Ground Floor"));
    }

    #[test]
    fn test_init_office_nested() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("office-building");
        create_nested_project(&project_dir, "office-building", Template::Office).unwrap();

        assert!(project_dir.join("storeys/ground-floor.bml").exists());
        let content = fs::read_to_string(project_dir.join("storeys/ground-floor.bml")).unwrap();
        assert!(content.contains("Reception"));
        assert!(content.contains("Office A"));
    }

    #[test]
    fn test_init_mixed_use_nested() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("mixed-building");
        create_nested_project(&project_dir, "mixed-building", Template::MixedUse).unwrap();

        // Verify project.bml has storey imports, not inline content
        let content = fs::read_to_string(project_dir.join("project.bml")).unwrap();
        assert!(content.contains("import ground from \"./storeys/ground-floor.bml\""));
        assert!(content.contains("import first from \"./storeys/first-floor.bml\""));
        assert!(content.contains("import templates from \"./templates.bml\""));

        // Verify storey files
        let ground = fs::read_to_string(project_dir.join("storeys/ground-floor.bml")).unwrap();
        assert!(ground.contains("### Storey: Ground Floor"));

        let first = fs::read_to_string(project_dir.join("storeys/first-floor.bml")).unwrap();
        assert!(first.contains("### Storey: First Floor"));
        assert!(first.contains("apply_template: TypicalResidentialFloor"));

        // Verify templates.bml
        let templates = fs::read_to_string(project_dir.join("templates.bml")).unwrap();
        assert!(templates.contains("Template: TypicalResidentialFloor"));
    }

    #[test]
    fn test_init_flat_structure() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("flat-project");
        create_flat_project(&project_dir, "flat-project", Template::Residential).unwrap();

        assert!(project_dir.join("buildingml.toml").exists());
        assert!(project_dir.join("project.bml").exists());
        assert!(project_dir.join("materials.bml").exists());
        assert!(project_dir.join("checks.bml").exists());
        // Storey content is in project.bml
        let content = fs::read_to_string(project_dir.join("project.bml")).unwrap();
        assert!(content.contains("### Storey: Ground Floor"));
    }

    #[test]
    fn test_init_directory_exists_no_force() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("existing");
        create_dir(&project_dir).unwrap();

        let result = create_nested_project(&project_dir, "existing", Template::Residential);
        assert!(result.is_ok());
    }

    #[test]
    fn test_init_force_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("overwrite-me");
        create_nested_project(&project_dir, "overwrite-me", Template::Residential).unwrap();

        // Add a marker file
        fs::write(project_dir.join("marker.txt"), "old").unwrap();

        // Re-create with force — buildingml.toml exists so force is allowed
        assert!(project_dir.join("buildingml.toml").exists());
        std::fs::remove_dir_all(&project_dir).unwrap();
        create_nested_project(&project_dir, "overwrite-me", Template::Office).unwrap();

        assert!(!project_dir.join("marker.txt").exists());
        let content = fs::read_to_string(project_dir.join("storeys/ground-floor.bml")).unwrap();
        assert!(content.contains("Reception"));
    }

    #[test]
    fn test_force_refuses_non_buildingml_directory() {
        use std::sync::Mutex;
        // Serialize CWD-changing tests to avoid interference with other tests
        static CWD_MUTEX: Mutex<()> = Mutex::new(());
        let _lock = CWD_MUTEX.lock().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("not-a-project");
        create_dir(&project_dir).unwrap();
        fs::write(project_dir.join("random.txt"), "not a buildingml project").unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let result = run(
            Some("not-a-project".to_string()),
            None,
            false,
            true, // --force
        );

        std::env::set_current_dir(original_dir).unwrap();

        assert!(
            result.is_err(),
            "Expected error when --force targets a non-BuildingML directory"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("does not contain a buildingml.toml"),
            "Error message should mention buildingml.toml, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_generated_project_bml_parses() {
        use bml_core::parser::cst_to_ast;

        for template in [Template::Residential, Template::Office, Template::MixedUse] {
            let source = templates::project_bml_nested("Test Project", template);
            let result = cst_to_ast::parse(&source);
            assert!(
                result.is_ok(),
                "Project BML (nested) for {:?} failed to parse: {:?}",
                template,
                result.err()
            );

            let flat_source = templates::project_bml_flat("Test Project", template);
            let flat_result = cst_to_ast::parse(&flat_source);
            assert!(
                flat_result.is_ok(),
                "Project BML (flat) for {:?} failed to parse: {:?}",
                template,
                flat_result.err()
            );
        }
    }

    #[test]
    fn test_generated_storey_files_parse() {
        use bml_core::parser::cst_to_ast;

        for template in [Template::Residential, Template::Office, Template::MixedUse] {
            for (filename, content) in templates::storey_files(template) {
                let result = cst_to_ast::parse(content);
                assert!(
                    result.is_ok(),
                    "Storey file {} for {:?} failed to parse: {:?}",
                    filename,
                    template,
                    result.err()
                );
            }
            if let Some(tmpl_content) = templates::templates_bml(template) {
                let result = cst_to_ast::parse(tmpl_content);
                assert!(
                    result.is_ok(),
                    "Templates file for {:?} failed to parse: {:?}",
                    template,
                    result.err()
                );
            }
        }
    }

    #[test]
    fn test_generated_materials_parse() {
        use bml_core::parser::cst_to_ast;

        for template in [Template::Residential, Template::Office, Template::MixedUse] {
            let source = templates::materials_bml(template);
            let result = cst_to_ast::parse(source);
            assert!(
                result.is_ok(),
                "Materials for {:?} failed to parse: {:?}",
                template,
                result.err()
            );
        }
    }

    #[test]
    fn test_generated_furniture_assets_parse() {
        use bml_core::parser::cst_to_ast;

        let result = cst_to_ast::parse(templates::FURNITURE_ASSETS);
        assert!(
            result.is_ok(),
            "Furniture assets failed to parse: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_buildingml_toml_valid() {
        use bml_core::config::ProjectConfig;

        let toml_content = templates::buildingml_toml("Test Project");
        let config = ProjectConfig::parse(&toml_content);
        assert!(
            config.is_ok(),
            "Generated TOML is invalid: {:?}",
            config.err()
        );

        let config = config.unwrap();
        assert_eq!(config.buildingml_version, "0.1");
        assert_eq!(config.ifc_version, "IFC4");
        assert_eq!(config.entry, "project.bml");
        assert_eq!(config.units.length, "m");
    }

    #[test]
    fn test_gitignore_content() {
        assert!(templates::GITIGNORE.contains("*.ifc"));
        assert!(templates::GITIGNORE.contains("dist/"));
        assert!(templates::GITIGNORE.contains(".env"));
    }
}
