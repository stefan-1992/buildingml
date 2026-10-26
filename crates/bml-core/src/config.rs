use serde::Deserialize;
use std::path::Path;

/// Project configuration from `buildingml.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct ProjectConfig {
    /// BuildingML specification version (e.g., "0.1")
    #[serde(default = "default_version")]
    pub buildingml_version: String,

    /// IFC output version
    #[serde(default = "default_ifc_version")]
    pub ifc_version: String,

    /// Default output format
    #[serde(default = "default_output_format")]
    pub output_format: String,

    /// Default units configuration
    #[serde(default)]
    pub units: UnitsConfig,

    /// Entry point file (relative to project root)
    #[serde(default = "default_entry")]
    pub entry: String,

    /// Asset library files to auto-include (relative to project root).
    /// These are loaded before the entry file, making all assets available
    /// without explicit `import` directives.
    /// Example: `assets = ["assets/common-furniture.bml"]`
    #[serde(default)]
    pub assets: Vec<String>,

    /// Lint rule configuration
    #[serde(default)]
    pub lint: Option<LintConfig>,

    /// Largest chord error in metres when smooth and nurbs curves become polylines
    #[serde(default = "default_curve_tolerance")]
    pub curve_tolerance: f64,
}

/// Lint rule severity configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum LintLevel {
    /// Report as a warning
    #[default]
    Warn,
    /// Report as an error
    Error,
    /// Disable the rule
    Off,
}

/// Lint rule configuration in `buildingml.toml`.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct LintConfig {
    /// Wall thickness range warning (> 1.0m or < 0.05m)
    #[serde(default)]
    pub wall_thickness: LintLevel,

    /// Door width range warning (> 3.0m or < 0.5m)
    #[serde(default)]
    pub door_width: LintLevel,

    /// Window sill height warning (> 2.0m)
    #[serde(default)]
    pub window_sill: LintLevel,

    /// Storey height range warning (> 10m or < 2.0m)
    #[serde(default)]
    pub storey_height: LintLevel,

    /// Column aspect ratio warning (width/depth > 5:1)
    #[serde(default)]
    pub column_aspect: LintLevel,

    /// Beam span warning (> 20m)
    #[serde(default)]
    pub beam_span: LintLevel,

    /// Space has no door warning
    #[serde(default)]
    pub space_door: LintLevel,

    /// Coincident/duplicate walls on the same storey (collinear axis overlap
    /// or overlapping thickness bands of parallel walls)
    #[serde(default)]
    pub wall_overlap: LintLevel,
}

impl Default for LintConfig {
    fn default() -> Self {
        Self {
            wall_thickness: LintLevel::Warn,
            door_width: LintLevel::Warn,
            window_sill: LintLevel::Warn,
            storey_height: LintLevel::Warn,
            column_aspect: LintLevel::Warn,
            beam_span: LintLevel::Warn,
            space_door: LintLevel::Warn,
            wall_overlap: LintLevel::Warn,
        }
    }
}

/// Default units for the project.
#[derive(Debug, Clone, Deserialize)]
pub struct UnitsConfig {
    #[serde(default = "default_length_unit")]
    pub length: String,
    #[serde(default = "default_area_unit")]
    pub area: String,
    #[serde(default = "default_angle_unit")]
    pub angle: String,
}

impl Default for UnitsConfig {
    fn default() -> Self {
        Self {
            length: default_length_unit(),
            area: default_area_unit(),
            angle: default_angle_unit(),
        }
    }
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            buildingml_version: default_version(),
            ifc_version: default_ifc_version(),
            output_format: default_output_format(),
            units: UnitsConfig::default(),
            entry: default_entry(),
            assets: Vec::new(),
            lint: None,
            curve_tolerance: default_curve_tolerance(),
        }
    }
}

pub fn default_curve_tolerance() -> f64 {
    0.005
}
fn default_version() -> String {
    "0.1".to_string()
}
fn default_ifc_version() -> String {
    "IFC4".to_string()
}
fn default_output_format() -> String {
    "step".to_string()
}
fn default_length_unit() -> String {
    "m".to_string()
}
fn default_area_unit() -> String {
    "m2".to_string()
}
fn default_angle_unit() -> String {
    "deg".to_string()
}
fn default_entry() -> String {
    "project.bml".to_string()
}

impl ProjectConfig {
    /// Load project config from a `buildingml.toml` file.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::Io(path.display().to_string(), e.to_string()))?;
        Self::parse(&content)
    }

    /// Parse project config from a TOML string.
    pub fn parse(content: &str) -> Result<Self, ConfigError> {
        let config: Self =
            toml::from_str(content).map_err(|e| ConfigError::Parse(e.to_string()))?;
        if !(config.curve_tolerance > 0.0 && config.curve_tolerance.is_finite()) {
            return Err(ConfigError::Parse(format!(
                "`curve_tolerance` must be a positive length in metres, got {}",
                config.curve_tolerance
            )));
        }
        Ok(config)
    }
}

/// Errors during config loading.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file `{0}`: {1}")]
    Io(String, String),

    #[error("failed to parse config: {0}")]
    Parse(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_config() {
        let toml = r#"
buildingml_version = "0.1"
ifc_version = "IFC4"
output_format = "step"
entry = "main.bml"

[units]
length = "mm"
area = "m2"
angle = "deg"
"#;
        let config = ProjectConfig::parse(toml).unwrap();
        assert_eq!(config.buildingml_version, "0.1");
        assert_eq!(config.ifc_version, "IFC4");
        assert_eq!(config.output_format, "step");
        assert_eq!(config.entry, "main.bml");
        assert_eq!(config.units.length, "mm");
        assert_eq!(config.units.area, "m2");
    }

    #[test]
    fn test_parse_minimal_config() {
        let toml = r#"buildingml_version = "0.1""#;
        let config = ProjectConfig::parse(toml).unwrap();
        assert_eq!(config.buildingml_version, "0.1");
        assert_eq!(config.ifc_version, "IFC4");
        assert_eq!(config.entry, "project.bml");
        assert_eq!(config.units.length, "m");
    }

    #[test]
    fn test_parse_empty_config() {
        let config = ProjectConfig::parse("").unwrap();
        assert_eq!(config.buildingml_version, "0.1");
        assert_eq!(config.entry, "project.bml");
    }

    #[test]
    fn test_default_config() {
        let config = ProjectConfig::default();
        assert_eq!(config.buildingml_version, "0.1");
        assert_eq!(config.ifc_version, "IFC4");
        assert_eq!(config.output_format, "step");
        assert_eq!(config.entry, "project.bml");
        assert_eq!(config.units.length, "m");
        assert_eq!(config.units.area, "m2");
        assert_eq!(config.units.angle, "deg");
    }

    #[test]
    fn curve_tolerance_must_be_positive() {
        for bad in ["0", "-0.01", "nan", "inf"] {
            let err = ProjectConfig::parse(&format!("curve_tolerance = {bad}")).unwrap_err();
            assert!(err.to_string().contains("curve_tolerance"), "{err}");
        }
        assert_eq!(
            ProjectConfig::parse("curve_tolerance = 0.01")
                .unwrap()
                .curve_tolerance,
            0.01
        );
    }

    #[test]
    fn test_invalid_config() {
        let toml = "this is not valid toml [[[";
        assert!(ProjectConfig::parse(toml).is_err());
    }
}
