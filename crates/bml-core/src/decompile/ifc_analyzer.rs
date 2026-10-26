use std::collections::{BTreeMap, HashMap, HashSet};

/// Result of analyzing an IFC file for entity coverage.
#[derive(Debug, Clone)]
pub struct IfcAnalysis {
    /// Name of the source file (for reporting).
    pub file_name: String,
    /// Total number of entity instances in the file.
    pub total_entities: usize,
    /// Count of each IFC entity type (e.g., IFCWALL → 118).
    pub entity_counts: BTreeMap<String, usize>,
    /// Spatial hierarchy: site → building → storey names.
    pub spatial_summary: SpatialSummary,
    /// Length unit detected (e.g., "METRE", "MILLI.METRE", "CENTI.METRE").
    pub length_unit: Option<String>,
    /// Which BML element types were found and their counts.
    pub bml_mapped_elements: BTreeMap<String, usize>,
    /// IFC entity types that have no BML mapping.
    pub unmapped_entity_types: BTreeMap<String, usize>,
}

/// Summary of the spatial hierarchy found in an IFC file.
#[derive(Debug, Clone, Default)]
pub struct SpatialSummary {
    pub sites: Vec<String>,
    pub buildings: Vec<String>,
    pub storeys: Vec<(String, Option<f64>)>,
    pub spaces: Vec<(String, String)>, // (space_number, space_name)
}

/// Aggregated analysis across multiple IFC files.
#[derive(Debug, Clone)]
pub struct AggregateAnalysis {
    pub per_file: Vec<IfcAnalysis>,
    /// Combined entity frequency across all files.
    pub combined_entity_counts: BTreeMap<String, usize>,
    /// Total BIM elements (walls, slabs, etc.) vs total entities.
    pub bim_element_count: usize,
    pub total_entity_count: usize,
    /// Coverage: % of BIM element instances that map to a BML type.
    pub coverage_percent: f64,
    /// Ranked list of BIM entity types by frequency.
    pub entity_ranking: Vec<(String, usize, bool)>, // (type, count, is_mapped)
    /// Gap analysis: unmapped types that would be needed for 80% coverage.
    pub gaps_for_80_percent: Vec<(String, usize)>,
}

/// IFC entity types that represent BIM building elements
/// and their mapping to BML ElementType names.
fn bim_element_mapping() -> HashMap<&'static str, Option<&'static str>> {
    let mut map = HashMap::new();
    // Direct BML mappings (12 element types)
    map.insert("IFCWALL", Some("Wall"));
    map.insert("IFCWALLSTANDARDCASE", Some("Wall"));
    map.insert("IFCSLAB", Some("Slab"));
    map.insert("IFCCOLUMN", Some("Column"));
    map.insert("IFCBEAM", Some("Beam"));
    map.insert("IFCWINDOW", Some("Window"));
    map.insert("IFCDOOR", Some("Door"));
    map.insert("IFCROOF", Some("Roof"));
    map.insert("IFCSTAIR", Some("Stair"));
    map.insert("IFCSTAIRFLIGHT", Some("Stair")); // sub-component of stair
    map.insert("IFCRAILING", Some("Railing"));
    map.insert("IFCRAMP", Some("Ramp"));
    map.insert("IFCRAMPFLIGHT", Some("Ramp"));
    map.insert("IFCFURNITURE", Some("Furniture"));
    map.insert("IFCTRANSPORTELEMENT", Some("Elevator"));
    map.insert("IFCSPACE", Some("Space"));

    map.insert("IFCBUILDINGELEMENTPROXY", Some("Proxy"));
    map.insert("IFCMEMBER", Some("Member"));
    map.insert("IFCPLATE", Some("Plate"));
    map.insert("IFCOPENINGELEMENT", Some("Opening"));
    map.insert("IFCGRID", Some("Grid"));
    map.insert("IFCCOVERING", Some("Covering"));
    map.insert("IFCFOOTING", Some("Footing"));

    // BIM elements without direct BML mapping (potential future additions)
    map.insert("IFCCURTAINWALL", None);
    map.insert("IFCFLOWTERMINAL", None);
    map.insert("IFCFLOWSEGMENT", None);
    map.insert("IFCFLOWFITTING", None);
    map.insert("IFCFLOWCONTROLLER", None);
    map.insert("IFCELEMENTASSEMBLY", None);
    map.insert("IFCPILE", None);
    map
}

/// Set of IFC entity types that are spatial/structural containers (not elements).
fn spatial_types() -> HashSet<&'static str> {
    let mut set = HashSet::new();
    set.insert("IFCSITE");
    set.insert("IFCBUILDING");
    set.insert("IFCBUILDINGSTOREY");
    set.insert("IFCPROJECT");
    set
}

/// Set of IFC entity types that are type definitions (not instances).
fn type_definition_types() -> HashSet<&'static str> {
    let mut set = HashSet::new();
    set.insert("IFCWALLTYPE");
    set.insert("IFCSLABTYPE");
    set.insert("IFCCOLUMNTYPE");
    set.insert("IFCBEAMTYPE");
    set.insert("IFCWINDOWTYPE");
    set.insert("IFCDOORTYPE");
    set.insert("IFCROOFTYPE");
    set.insert("IFCSTAIRTYPE");
    set.insert("IFCSTAIRFLIGHTTYPE");
    set.insert("IFCRAILINGTYPE");
    set.insert("IFCRAMPTYPE");
    set.insert("IFCFURNITURETYPE");
    set.insert("IFCMEMBERTYPE");
    set.insert("IFCPLATETYPE");
    set.insert("IFCCURTAINWALLTYPE");
    set.insert("IFCCOVERINGTYPE");
    set.insert("IFCBUILDINGELEMENTPROXYTYPE");
    set.insert("IFCSPACETYPE");
    set
}

/// Analyze a single IFC STEP file and return a coverage analysis.
pub fn analyze_ifc(ifc_content: &str, file_name: &str) -> IfcAnalysis {
    let mut entity_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_entities = 0;
    let mut spatial = SpatialSummary::default();
    let mut length_unit = None;

    let mapping = bim_element_mapping();

    for statement in super::ifc_reader::entity_statements(ifc_content) {
        let trimmed: &str = &statement;
        let Some(eq_pos) = trimmed.find('=') else {
            continue;
        };
        let entity_text = trimmed[eq_pos + 1..].trim();
        let Some(paren_pos) = entity_text.find('(') else {
            continue;
        };
        let entity_type = entity_text[..paren_pos].trim();
        if entity_type.is_empty() {
            continue;
        }

        total_entities += 1;
        *entity_counts.entry(entity_type.to_string()).or_insert(0) += 1;

        // Extract spatial hierarchy info
        match entity_type {
            "IFCSITE" => {
                if let Some(name) = extract_name(entity_text) {
                    spatial.sites.push(name);
                }
            }
            "IFCBUILDING" => {
                if let Some(name) = extract_name(entity_text) {
                    spatial.buildings.push(name);
                }
            }
            "IFCBUILDINGSTOREY" => {
                let name = extract_name(entity_text).unwrap_or_else(|| "Storey".to_string());
                let elevation = extract_elevation(entity_text);
                spatial.storeys.push((name, elevation));
            }
            "IFCSPACE" => {
                let number = extract_name(entity_text).unwrap_or_default();
                let long_name = extract_long_name(entity_text).unwrap_or_default();
                spatial.spaces.push((number, long_name));
            }
            // Detect length unit: IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
            // Only take the first length unit (the one typically in IfcUnitAssignment)
            "IFCSIUNIT" if entity_text.contains(".LENGTHUNIT.") && length_unit.is_none() => {
                length_unit = Some(extract_unit_spec(entity_text));
            }
            _ => {}
        }
    }

    // Categorize entities
    let mut bml_mapped_elements: BTreeMap<String, usize> = BTreeMap::new();
    let mut unmapped_entity_types: BTreeMap<String, usize> = BTreeMap::new();

    let spatial_set = spatial_types();
    let type_defs = type_definition_types();

    for (entity_type, count) in &entity_counts {
        let upper = entity_type.to_uppercase();

        // Skip spatial containers, type definitions, and infrastructure
        if spatial_set.contains(upper.as_str()) || type_defs.contains(upper.as_str()) {
            continue;
        }

        if let Some(bml_mapping) = mapping.get(upper.as_str()) {
            match bml_mapping {
                Some(bml_type) => {
                    *bml_mapped_elements.entry(bml_type.to_string()).or_insert(0) += count;
                }
                None => {
                    unmapped_entity_types.insert(entity_type.clone(), *count);
                }
            }
        }
        // Non-BIM entities (geometry, relationships, etc.) are ignored for coverage
    }

    IfcAnalysis {
        file_name: file_name.to_string(),
        total_entities,
        entity_counts,
        spatial_summary: spatial,
        length_unit,
        bml_mapped_elements,
        unmapped_entity_types,
    }
}

/// Analyze multiple IFC files and produce an aggregate report.
pub fn analyze_multiple(analyses: Vec<IfcAnalysis>) -> AggregateAnalysis {
    let mut combined_entity_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut bim_element_count = 0;
    let mut total_entity_count = 0;

    let mapping = bim_element_mapping();
    let spatial_set = spatial_types();
    let type_defs = type_definition_types();

    for analysis in &analyses {
        total_entity_count += analysis.total_entities;

        for (entity_type, count) in &analysis.entity_counts {
            *combined_entity_counts
                .entry(entity_type.clone())
                .or_insert(0) += count;
        }
    }

    // Build ranked list of BIM element types
    let mut entity_ranking: Vec<(String, usize, bool)> = Vec::new();
    let mut mapped_count = 0usize;
    let mut unmapped_bim: Vec<(String, usize)> = Vec::new();

    for (entity_type, count) in &combined_entity_counts {
        let upper = entity_type.to_uppercase();
        if spatial_set.contains(upper.as_str()) || type_defs.contains(upper.as_str()) {
            continue;
        }
        if let Some(bml_mapping) = mapping.get(upper.as_str()) {
            bim_element_count += count;
            let is_mapped = bml_mapping.is_some();
            entity_ranking.push((entity_type.clone(), *count, is_mapped));
            if is_mapped {
                mapped_count += count;
            } else {
                unmapped_bim.push((entity_type.clone(), *count));
            }
        }
    }

    entity_ranking.sort_by_key(|e| std::cmp::Reverse(e.1));
    unmapped_bim.sort_by_key(|e| std::cmp::Reverse(e.1));

    let coverage_percent = if bim_element_count > 0 {
        (mapped_count as f64 / bim_element_count as f64) * 100.0
    } else {
        0.0
    };

    // Gap analysis: what unmapped types would be needed to reach 80%?
    let target = (bim_element_count as f64 * 0.8).ceil() as usize;
    let mut gaps_for_80_percent = Vec::new();
    let mut running_mapped = mapped_count;
    if running_mapped < target {
        for (entity_type, count) in &unmapped_bim {
            if running_mapped >= target {
                break;
            }
            gaps_for_80_percent.push((entity_type.clone(), *count));
            running_mapped += count;
        }
    }

    AggregateAnalysis {
        per_file: analyses,
        combined_entity_counts,
        bim_element_count,
        total_entity_count,
        coverage_percent,
        entity_ranking,
        gaps_for_80_percent,
    }
}

/// Format an analysis report as human-readable text.
pub fn format_report(aggregate: &AggregateAnalysis) -> String {
    let mut out = String::new();

    out.push_str("╔══════════════════════════════════════════════════════════════╗\n");
    out.push_str("║           IFC IMPORT COVERAGE ANALYSIS REPORT              ║\n");
    out.push_str("╚══════════════════════════════════════════════════════════════╝\n\n");

    // Per-file summaries
    for analysis in &aggregate.per_file {
        out.push_str(&format!("━━━ {} ━━━\n", analysis.file_name));
        out.push_str(&format!(
            "  Total IFC entities: {}\n",
            analysis.total_entities
        ));
        if let Some(unit) = &analysis.length_unit {
            out.push_str(&format!("  Length unit: {}\n", unit));
        }

        // Spatial summary
        out.push_str("  Spatial hierarchy:\n");
        for s in &analysis.spatial_summary.sites {
            out.push_str(&format!("    Site: {}\n", s));
        }
        for b in &analysis.spatial_summary.buildings {
            out.push_str(&format!("    Building: {}\n", b));
        }
        for (name, elev) in &analysis.spatial_summary.storeys {
            if let Some(e) = elev {
                out.push_str(&format!("    Storey: {} (elevation: {})\n", name, e));
            } else {
                out.push_str(&format!("    Storey: {}\n", name));
            }
        }
        if !analysis.spatial_summary.spaces.is_empty() {
            out.push_str(&format!(
                "    Spaces: {} total\n",
                analysis.spatial_summary.spaces.len()
            ));
        }

        // BML mapped elements
        if !analysis.bml_mapped_elements.is_empty() {
            out.push_str("  BML-mapped elements:\n");
            let mut sorted: Vec<_> = analysis.bml_mapped_elements.iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(a.1));
            for (bml_type, count) in sorted {
                out.push_str(&format!("    {:20} {}\n", bml_type, count));
            }
        }

        // Unmapped
        if !analysis.unmapped_entity_types.is_empty() {
            out.push_str("  Unmapped BIM entities:\n");
            let mut sorted: Vec<_> = analysis.unmapped_entity_types.iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(a.1));
            for (ifc_type, count) in sorted {
                out.push_str(&format!("    {:40} {}\n", ifc_type, count));
            }
        }
        out.push('\n');
    }

    // Aggregate summary
    out.push_str("━━━ AGGREGATE ANALYSIS ━━━\n");
    out.push_str(&format!("  Files analyzed: {}\n", aggregate.per_file.len()));
    out.push_str(&format!(
        "  Total IFC entities: {}\n",
        aggregate.total_entity_count
    ));
    out.push_str(&format!(
        "  Total BIM elements: {}\n",
        aggregate.bim_element_count
    ));
    out.push_str(&format!(
        "  BML coverage: {:.1}%\n\n",
        aggregate.coverage_percent
    ));

    // Entity ranking table
    out.push_str("  BIM Element Frequency Ranking:\n");
    out.push_str("  ┌─────────────────────────────────────────┬───────┬─────────┐\n");
    out.push_str("  │ IFC Entity Type                         │ Count │ Mapped? │\n");
    out.push_str("  ├─────────────────────────────────────────┼───────┼─────────┤\n");
    for (entity_type, count, is_mapped) in &aggregate.entity_ranking {
        let status = if *is_mapped { "  ✓  " } else { "  ✗  " };
        out.push_str(&format!(
            "  │ {:39} │ {:>5} │ {} │\n",
            entity_type, count, status
        ));
    }
    out.push_str("  └─────────────────────────────────────────┴───────┴─────────┘\n\n");

    // Gap analysis
    if aggregate.coverage_percent < 80.0 {
        out.push_str("  Gap Analysis (entities needed for 80% coverage):\n");
        for (entity_type, count) in &aggregate.gaps_for_80_percent {
            out.push_str(&format!("    → {} ({} instances)\n", entity_type, count));
        }
    } else {
        out.push_str(&format!(
            "  ✓ Coverage exceeds 80% threshold ({:.1}%)\n",
            aggregate.coverage_percent
        ));
    }

    // Recommendations
    out.push_str("\n  Prioritized Implementation Recommendations:\n");
    let mut priority = 1;
    for (entity_type, count, is_mapped) in &aggregate.entity_ranking {
        if !is_mapped {
            out.push_str(&format!(
                "    {}. Add BML mapping for {} ({} instances across files)\n",
                priority, entity_type, count
            ));
            priority += 1;
        }
    }

    out
}

/// Format analysis as JSON.
pub fn format_json(aggregate: &AggregateAnalysis) -> String {
    let mut out = String::from("{\n");

    out.push_str("  \"summary\": {\n");
    out.push_str(&format!(
        "    \"files_analyzed\": {},\n",
        aggregate.per_file.len()
    ));
    out.push_str(&format!(
        "    \"total_ifc_entities\": {},\n",
        aggregate.total_entity_count
    ));
    out.push_str(&format!(
        "    \"total_bim_elements\": {},\n",
        aggregate.bim_element_count
    ));
    out.push_str(&format!(
        "    \"bml_coverage_percent\": {:.1}\n",
        aggregate.coverage_percent
    ));
    out.push_str("  },\n");

    // Per-file details
    out.push_str("  \"files\": [\n");
    for (i, analysis) in aggregate.per_file.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!("      \"name\": \"{}\",\n", analysis.file_name));
        out.push_str(&format!(
            "      \"total_entities\": {},\n",
            analysis.total_entities
        ));
        out.push_str(&format!(
            "      \"length_unit\": {},\n",
            analysis
                .length_unit
                .as_ref()
                .map(|u| format!("\"{}\"", u))
                .unwrap_or_else(|| "null".to_string())
        ));

        // Spatial
        out.push_str(&format!(
            "      \"storeys\": {},\n",
            analysis.spatial_summary.storeys.len()
        ));
        out.push_str(&format!(
            "      \"spaces\": {},\n",
            analysis.spatial_summary.spaces.len()
        ));

        // Mapped
        out.push_str("      \"bml_mapped\": {");
        let mapped: Vec<String> = analysis
            .bml_mapped_elements
            .iter()
            .map(|(k, v)| format!("\"{}\": {}", k, v))
            .collect();
        out.push_str(&mapped.join(", "));
        out.push_str("},\n");

        // Unmapped
        out.push_str("      \"unmapped_bim\": {");
        let unmapped: Vec<String> = analysis
            .unmapped_entity_types
            .iter()
            .map(|(k, v)| format!("\"{}\": {}", k, v))
            .collect();
        out.push_str(&unmapped.join(", "));
        out.push('}');

        out.push_str("\n    }");
        if i < aggregate.per_file.len() - 1 {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ],\n");

    // Entity ranking
    out.push_str("  \"entity_ranking\": [\n");
    for (i, (entity_type, count, is_mapped)) in aggregate.entity_ranking.iter().enumerate() {
        out.push_str(&format!(
            "    {{\"type\": \"{}\", \"count\": {}, \"mapped\": {}}}",
            entity_type, count, is_mapped
        ));
        if i < aggregate.entity_ranking.len() - 1 {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ],\n");

    // Gaps
    out.push_str("  \"gaps_for_80_percent\": [");
    let gaps: Vec<String> = aggregate
        .gaps_for_80_percent
        .iter()
        .map(|(k, v)| format!("{{\"type\": \"{}\", \"count\": {}}}", k, v))
        .collect();
    out.push_str(&gaps.join(", "));
    out.push_str("]\n");

    out.push_str("}\n");
    out
}

// Helper functions (shared with ifc_reader but kept local to avoid coupling)

fn extract_name(entity_text: &str) -> Option<String> {
    let paren_start = entity_text.find('(')?;
    let args = &entity_text[paren_start + 1..];

    let mut field_idx = 0;
    let mut in_quotes = false;
    let mut depth = 0;
    let mut name_start = None;

    for (i, ch) in args.char_indices() {
        match ch {
            '\'' if depth == 0 => {
                if in_quotes {
                    if field_idx == 2 {
                        return Some(args[name_start.unwrap()..i].to_string());
                    }
                    in_quotes = false;
                } else {
                    in_quotes = true;
                    if field_idx == 2 {
                        name_start = Some(i + 1);
                    }
                }
            }
            ',' if !in_quotes && depth == 0 => {
                field_idx += 1;
            }
            '(' if !in_quotes => depth += 1,
            ')' if !in_quotes && depth > 0 => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Extract the "long name" (8th field, index 7) from IFCSPACE.
fn extract_long_name(entity_text: &str) -> Option<String> {
    let paren_start = entity_text.find('(')?;
    let args = &entity_text[paren_start + 1..];

    let mut field_idx = 0;
    let mut in_quotes = false;
    let mut depth = 0;
    let mut name_start = None;

    for (i, ch) in args.char_indices() {
        match ch {
            '\'' if depth == 0 => {
                if in_quotes {
                    if field_idx == 7 {
                        return Some(args[name_start.unwrap()..i].to_string());
                    }
                    in_quotes = false;
                } else {
                    in_quotes = true;
                    if field_idx == 7 {
                        name_start = Some(i + 1);
                    }
                }
            }
            ',' if !in_quotes && depth == 0 => {
                field_idx += 1;
            }
            '(' if !in_quotes => depth += 1,
            ')' if !in_quotes && depth > 0 => depth -= 1,
            _ => {}
        }
    }
    None
}

fn extract_elevation(entity_text: &str) -> Option<f64> {
    let paren_start = entity_text.find('(')?;
    let paren_end = entity_text.rfind(')')?;
    let args = &entity_text[paren_start + 1..paren_end];

    let parts = split_ifc_args(args);
    // IFC4 IFCBUILDINGSTOREY: elevation is at index 9 (10 params)
    if parts.len() >= 10 {
        let elev_str = parts[9].trim();
        if elev_str != "$"
            && let Ok(v) = elev_str.trim_end_matches('.').parse::<f64>()
        {
            return Some(v);
        }
    }
    // Fallback to index 8 for shorter formats
    if parts.len() >= 9 {
        let elev_str = parts[8].trim();
        if elev_str != "$" && !elev_str.starts_with('.') {
            return elev_str.trim_end_matches('.').parse::<f64>().ok();
        }
    }
    None
}

fn extract_unit_spec(entity_text: &str) -> String {
    // IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.); → "millimetre"
    // IFCSIUNIT(*,.LENGTHUNIT.,.CENTI.,.METRE.); → "centimetre"
    // IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.); → "metre"
    let text = entity_text.to_uppercase();
    if text.contains(".MILLI.") {
        "millimetre".to_string()
    } else if text.contains(".CENTI.") {
        "centimetre".to_string()
    } else {
        "metre".to_string()
    }
}

fn split_ifc_args(args: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut in_quotes = false;
    let mut start = 0;

    for (i, ch) in args.char_indices() {
        match ch {
            '\'' => in_quotes = !in_quotes,
            '(' if !in_quotes => depth += 1,
            ')' if !in_quotes => depth -= 1,
            ',' if !in_quotes && depth == 0 => {
                parts.push(&args[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&args[start..]);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_small_ifc() {
        let ifc_content = r#"ISO-10303-21;
HEADER;
FILE_SCHEMA(('IFC4'));
ENDSEC;
DATA;
#1=IFCSITE('abc',#2,'MySite',$,$,#3,$,$,.ELEMENT.,$,$,0.,$,$);
#2=IFCBUILDING('def',#2,'MyBuilding',$,$,#3,$,'MyBuilding',.ELEMENT.,$,$,$);
#3=IFCBUILDINGSTOREY('ghi',#2,'Ground Floor',$,$,#3,$,'Ground Floor',.ELEMENT.,0.);
#4=IFCWALL('w1',#2,'W-01',$,$,#3,$,$);
#5=IFCWALL('w2',#2,'W-02',$,$,#3,$,$);
#6=IFCSLAB('s1',#2,'S-01',$,$,#3,$,$);
#7=IFCBEAM('b1',#2,'B-01',$,$,#3,$,$);
#8=IFCCOLUMN('c1',#2,'C-01',$,$,#3,$,$);
#9=IFCMEMBER('m1',#2,'M-01',$,$,#3,$,$);
#10=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
ENDSEC;
END-ISO-10303-21;"#;

        let analysis = analyze_ifc(ifc_content, "test.ifc");

        assert_eq!(analysis.file_name, "test.ifc");
        assert_eq!(analysis.total_entities, 10);
        assert_eq!(analysis.length_unit.as_deref(), Some("millimetre"));

        // Spatial
        assert_eq!(analysis.spatial_summary.sites.len(), 1);
        assert_eq!(analysis.spatial_summary.sites[0], "MySite");
        assert_eq!(analysis.spatial_summary.buildings.len(), 1);
        assert_eq!(analysis.spatial_summary.storeys.len(), 1);

        // BML mapped
        assert_eq!(analysis.bml_mapped_elements.get("Wall"), Some(&2));
        assert_eq!(analysis.bml_mapped_elements.get("Slab"), Some(&1));
        assert_eq!(analysis.bml_mapped_elements.get("Beam"), Some(&1));
        assert_eq!(analysis.bml_mapped_elements.get("Column"), Some(&1));

        // Member is now mapped
        assert_eq!(analysis.bml_mapped_elements.get("Member"), Some(&1));
    }

    #[test]
    fn test_aggregate_analysis() {
        let a1 = IfcAnalysis {
            file_name: "a.ifc".to_string(),
            total_entities: 100,
            entity_counts: BTreeMap::from([
                ("IFCWALL".to_string(), 10),
                ("IFCMEMBER".to_string(), 5),
            ]),
            spatial_summary: SpatialSummary::default(),
            length_unit: Some("metre".to_string()),
            bml_mapped_elements: BTreeMap::from([
                ("Wall".to_string(), 10),
                ("Member".to_string(), 5),
            ]),
            unmapped_entity_types: BTreeMap::new(),
        };
        let a2 = IfcAnalysis {
            file_name: "b.ifc".to_string(),
            total_entities: 200,
            entity_counts: BTreeMap::from([
                ("IFCWALL".to_string(), 20),
                ("IFCMEMBER".to_string(), 15),
            ]),
            spatial_summary: SpatialSummary::default(),
            length_unit: Some("millimetre".to_string()),
            bml_mapped_elements: BTreeMap::from([
                ("Wall".to_string(), 20),
                ("Member".to_string(), 15),
            ]),
            unmapped_entity_types: BTreeMap::new(),
        };

        let agg = analyze_multiple(vec![a1, a2]);

        assert_eq!(agg.per_file.len(), 2);
        assert_eq!(agg.bim_element_count, 50); // 10+5+20+15
        assert_eq!(agg.total_entity_count, 300);
        // Mapped: 50 out of 50 = 100%
        assert!((agg.coverage_percent - 100.0).abs() < 0.1);
    }

    #[test]
    fn test_extract_unit_spec() {
        assert_eq!(
            extract_unit_spec("IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.)"),
            "millimetre"
        );
        assert_eq!(
            extract_unit_spec("IFCSIUNIT(*,.LENGTHUNIT.,.CENTI.,.METRE.)"),
            "centimetre"
        );
        assert_eq!(
            extract_unit_spec("IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.)"),
            "metre"
        );
    }

    #[test]
    fn test_analyze_real_model_1() {
        let path = "../../tests/fixtures/model_1.ifc";
        if !std::path::Path::new(path).exists() {
            return; // Skip if fixture not available
        }
        let content = std::fs::read_to_string(path).unwrap();
        let analysis = analyze_ifc(&content, "model_1.ifc");

        // model_1 has 6 storeys, 20 spaces, millimetre units
        assert_eq!(analysis.spatial_summary.storeys.len(), 6);
        assert_eq!(analysis.spatial_summary.spaces.len(), 20);
        assert_eq!(analysis.length_unit.as_deref(), Some("millimetre"));

        // Should have walls, beams, columns, etc.
        assert!(analysis.bml_mapped_elements.contains_key("Wall"));
        assert!(analysis.bml_mapped_elements.contains_key("Beam"));
        assert!(analysis.bml_mapped_elements.contains_key("Column"));
        assert!(analysis.bml_mapped_elements.contains_key("Furniture"));
    }

    #[test]
    fn test_analyze_real_model_2() {
        let path = "../../tests/fixtures/model_2.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let content = std::fs::read_to_string(path).unwrap();
        let analysis = analyze_ifc(&content, "model_2.ifc");

        assert_eq!(analysis.spatial_summary.storeys.len(), 6);
        assert_eq!(analysis.spatial_summary.spaces.len(), 35);
        assert_eq!(analysis.length_unit.as_deref(), Some("centimetre"));
    }

    #[test]
    fn test_analyze_real_model_3() {
        let path = "../../tests/fixtures/model_3.ifc";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let content = std::fs::read_to_string(path).unwrap();
        let analysis = analyze_ifc(&content, "model_3.ifc");

        assert_eq!(analysis.spatial_summary.storeys.len(), 6);
        assert_eq!(analysis.length_unit.as_deref(), Some("metre"));
    }

    #[test]
    fn test_aggregate_real_files() {
        let files = [
            ("../../tests/fixtures/model_1.ifc", "model_1.ifc"),
            ("../../tests/fixtures/model_2.ifc", "model_2.ifc"),
            ("../../tests/fixtures/model_3.ifc", "model_3.ifc"),
        ];

        let mut analyses = Vec::new();
        for (path, name) in &files {
            if !std::path::Path::new(path).exists() {
                return;
            }
            let content = std::fs::read_to_string(path).unwrap();
            analyses.push(analyze_ifc(&content, name));
        }

        let agg = analyze_multiple(analyses);
        // Should have a reasonable coverage percent
        assert!(agg.coverage_percent > 0.0);
        assert!(!agg.entity_ranking.is_empty());

        // Print the report for inspection
        let report = format_report(&agg);
        assert!(report.contains("AGGREGATE ANALYSIS"));
    }
}
