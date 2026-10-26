//! Extract IFC property sets and quantities for the element inspector.
//!
//! `IfcRelDefinesByProperties` attaches an `IfcPropertySet` (the `Pset_*`
//! values like `IsExternal` or `FireRating`) or an `IfcElementQuantity` (the
//! `Qto_*` measurements like `GrossFloorArea`) to a list of products. Both are
//! flattened to `"<set> · <property>" -> "<value>"` pairs, which is the shape
//! `SceneNode::properties` already carries to the viewers.

use std::collections::{HashMap, HashSet};

use ifc_lite_core::{AttributeValue, EntityDecoder, EntityScanner};

use super::ifc_styles::NAME_SEP;

/// How to read a property/quantity member: where its name and value sit.
#[derive(Clone, Copy)]
enum Member {
    /// `IfcPropertySingleValue(Name, Description, NominalValue, Unit)`.
    Single,
    /// `IfcPropertyEnumeratedValue` / `IfcPropertyListValue` — a list at index 2.
    Multi,
    /// Every `IfcQuantity*` puts its measure at index 3.
    Quantity,
}

fn classify_member(type_name: &str) -> Option<Member> {
    Some(match type_name {
        "IFCPROPERTYSINGLEVALUE" => Member::Single,
        "IFCPROPERTYENUMERATEDVALUE" | "IFCPROPERTYLISTVALUE" => Member::Multi,
        "IFCQUANTITYLENGTH" | "IFCQUANTITYAREA" | "IFCQUANTITYVOLUME" | "IFCQUANTITYCOUNT"
        | "IFCQUANTITYWEIGHT" | "IFCQUANTITYTIME" | "IFCQUANTITYNUMBER" => Member::Quantity,
        _ => return None,
    })
}

/// Which attribute holds a set's members, and whether it is a quantity set.
#[derive(Clone, Copy)]
struct SetShape {
    members_at: usize,
}

fn classify_set(type_name: &str) -> Option<SetShape> {
    Some(match type_name {
        // IfcPropertySet(GlobalId, OwnerHistory, Name, Description, HasProperties)
        "IFCPROPERTYSET" => SetShape { members_at: 4 },
        // IfcElementQuantity(.., Name, Description, MethodOfMeasurement, Quantities)
        "IFCELEMENTQUANTITY" => SetShape { members_at: 5 },
        _ => return None,
    })
}

/// Element id → inspector rows, ordered by set name then file order.
pub struct PropertyIndex {
    by_element: HashMap<u32, Vec<(String, String)>>,
}

impl PropertyIndex {
    #[cfg(test)]
    pub fn rows(&self, element_id: u32) -> Option<&[(String, String)]> {
        self.by_element.get(&element_id).map(|v| v.as_slice())
    }

    /// Move an element's rows out; each element is emitted once.
    pub fn take(&mut self, element_id: u32) -> Option<Vec<(String, String)>> {
        self.by_element.remove(&element_id)
    }

    /// Resolve property sets for `wanted` only.
    ///
    /// Most sets in an exported file hang off the project, the site or spaces
    /// rather than the elements we render — on `model_2.ifc` that is the
    /// majority of 15,808 sets — so restricting the walk to referenced sets
    /// avoids decoding them.
    pub fn build(content: &[u8], decoder: &mut EntityDecoder, wanted: &HashSet<u32>) -> Self {
        let mut rel_ids: Vec<u32> = Vec::new();
        let mut sets: HashMap<u32, SetShape> = HashMap::new();
        let mut members: HashMap<u32, Member> = HashMap::new();

        let mut scanner = EntityScanner::new(content);
        while let Some((id, type_name, _, _)) = scanner.next_entity() {
            if type_name == "IFCRELDEFINESBYPROPERTIES" {
                rel_ids.push(id);
            } else if let Some(shape) = classify_set(type_name) {
                sets.insert(id, shape);
            } else if let Some(member) = classify_member(type_name) {
                members.insert(id, member);
            }
        }

        // Definition id -> the rendered elements it applies to.
        let mut definition_targets: HashMap<u32, Vec<u32>> = HashMap::new();
        for rid in &rel_ids {
            let Ok(rel) = decoder.decode_by_id(*rid) else {
                continue;
            };
            let Some(definition) = rel.get(5).and_then(|a| a.as_entity_ref()) else {
                continue;
            };
            if !sets.contains_key(&definition) {
                continue;
            }
            let Some(objects) = rel.get(4).and_then(|a| a.as_list()) else {
                continue;
            };
            for oid in objects.iter().filter_map(|o| o.as_entity_ref()) {
                if wanted.contains(&oid) {
                    definition_targets.entry(definition).or_default().push(oid);
                }
            }
        }

        // Resolve each referenced set once, then fan its rows out to its elements.
        let mut by_element: HashMap<u32, Vec<(String, String)>> = HashMap::new();
        let mut definitions: Vec<u32> = definition_targets.keys().copied().collect();
        definitions.sort_unstable();
        for definition in definitions {
            let shape = sets[&definition];
            let Ok(entity) = decoder.decode_by_id(definition) else {
                continue;
            };
            let set_name = entity
                .get(2)
                .and_then(|a| a.as_string())
                .unwrap_or("Properties")
                .to_string();
            let Some(member_refs) = entity.get(shape.members_at).and_then(|a| a.as_list()) else {
                continue;
            };
            let member_ids: Vec<u32> = member_refs
                .iter()
                .filter_map(|m| m.as_entity_ref())
                .collect();

            let mut rows: Vec<(String, String)> = Vec::new();
            for mid in member_ids {
                let Some(kind) = members.get(&mid).copied() else {
                    continue; // complex quantities and unsupported property types
                };
                let Ok(member) = decoder.decode_by_id(mid) else {
                    continue;
                };
                let Some(name) = member.get(0).and_then(|a| a.as_string()) else {
                    continue;
                };
                if name.is_empty() {
                    continue;
                }
                let value_at = match kind {
                    Member::Single | Member::Multi => 2,
                    Member::Quantity => 3,
                };
                let Some(value) = member.get(value_at).and_then(format_value) else {
                    continue;
                };
                rows.push((format!("{set_name}{NAME_SEP}{name}"), value));
            }
            if rows.is_empty() {
                continue;
            }
            for eid in &definition_targets[&definition] {
                by_element
                    .entry(*eid)
                    .or_default()
                    .extend(rows.iter().cloned());
            }
        }

        // Sets arrive in id order; sort by the rendered key so an element's rows
        // group by set name regardless of where they sat in the file.
        for rows in by_element.values_mut() {
            rows.sort_by(|a, b| a.0.cmp(&b.0));
            rows.dedup();
        }

        PropertyIndex { by_element }
    }
}

/// Render an `IfcValue` for display.
///
/// Values reach us as STEP typed values — `IFCLABEL('Concrete')`,
/// `IFCBOOLEAN(.T.)`, `IFCAREAMEASURE(56.3)` — which the decoder stores as a
/// list of `[type name, inner]`. A plain list of strings has the same shape, so
/// the `IFC` prefix on the first element is what tells them apart.
fn format_value(attr: &AttributeValue) -> Option<String> {
    if let Some(items) = attr.as_list() {
        let is_typed = items.len() >= 2
            && items[0]
                .as_string()
                .is_some_and(|t| t.starts_with("IFC") || t.starts_with("Ifc"));
        if is_typed {
            return format_scalar(&items[1]);
        }
        let parts: Vec<String> = items.iter().filter_map(format_value).collect();
        return (!parts.is_empty()).then(|| parts.join(", "));
    }
    format_scalar(attr)
}

fn format_scalar(value: &AttributeValue) -> Option<String> {
    match value {
        AttributeValue::String(s) => (!s.is_empty()).then(|| s.clone()),
        AttributeValue::Integer(i) => Some(i.to_string()),
        AttributeValue::Float(f) => Some(format_float(*f)),
        // STEP writes booleans and logicals as .T. / .F. / .U.
        AttributeValue::Enum(e) => Some(match e.as_str() {
            "T" => "true".to_string(),
            "F" => "false".to_string(),
            "U" => "unknown".to_string(),
            other => other.to_lowercase(),
        }),
        _ => None,
    }
}

/// Trim exporter noise like `54.760437263888569` down to something readable
/// without losing meaningful precision.
fn format_float(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }
    let s = format!("{value:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ifc_lite_core::build_entity_index;

    fn build(entities: &str, wanted: &[u32]) -> PropertyIndex {
        let content = format!(
            "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n{entities}\nENDSEC;\nEND-ISO-10303-21;\n"
        )
        .into_bytes();
        let index = build_entity_index(&content);
        let mut decoder = EntityDecoder::with_index(&content, index);
        PropertyIndex::build(&content, &mut decoder, &wanted.iter().copied().collect())
    }

    #[test]
    fn extracts_property_set_values() {
        let index = build(
            "#1=IFCPROPERTYSINGLEVALUE('IsExternal',$,IFCBOOLEAN(.T.),$);\n\
             #2=IFCPROPERTYSINGLEVALUE('FireRating',$,IFCLABEL('REI 90'),$);\n\
             #3=IFCPROPERTYSET('guid',$,'Pset_WallCommon',$,(#1,#2));\n\
             #4=IFCRELDEFINESBYPROPERTIES('guid',$,$,$,(#50),#3);",
            &[50],
        );
        let rows = index.rows(50).unwrap();
        assert_eq!(
            rows,
            [
                (
                    "Pset_WallCommon · FireRating".to_string(),
                    "REI 90".to_string()
                ),
                (
                    "Pset_WallCommon · IsExternal".to_string(),
                    "true".to_string()
                ),
            ]
        );
    }

    #[test]
    fn extracts_quantities_with_readable_precision() {
        let index = build(
            "#1=IFCQUANTITYAREA('NetFloorArea','',$,54.760437263888569,$);\n\
             #2=IFCQUANTITYVOLUME('GrossVolume','',$,12.,$);\n\
             #3=IFCELEMENTQUANTITY('guid',$,'Qto_SpaceBaseQuantities',$,$,(#1,#2));\n\
             #4=IFCRELDEFINESBYPROPERTIES('guid',$,$,$,(#50),#3);",
            &[50],
        );
        let rows = index.rows(50).unwrap();
        assert_eq!(
            rows,
            [
                (
                    "Qto_SpaceBaseQuantities · GrossVolume".to_string(),
                    "12".to_string()
                ),
                (
                    "Qto_SpaceBaseQuantities · NetFloorArea".to_string(),
                    "54.7604".to_string()
                ),
            ]
        );
    }

    /// One set shared by many elements must reach all of them.
    #[test]
    fn fans_a_shared_set_out_to_every_element() {
        let index = build(
            "#1=IFCPROPERTYSINGLEVALUE('Reference',$,IFCIDENTIFIER('W-01'),$);\n\
             #2=IFCPROPERTYSET('guid',$,'Pset_WallCommon',$,(#1));\n\
             #3=IFCRELDEFINESBYPROPERTIES('guid',$,$,$,(#50,#51,#52),#2);",
            &[50, 51, 52],
        );
        for id in [50, 51, 52] {
            assert_eq!(index.rows(id).unwrap().len(), 1, "element {id}");
        }
    }

    /// Sets attached only to unrendered objects must not be collected.
    #[test]
    fn skips_sets_for_unwanted_elements() {
        let index = build(
            "#1=IFCPROPERTYSINGLEVALUE('Client Name',$,IFCTEXT('Acme'),$);\n\
             #2=IFCPROPERTYSET('guid',$,'Other',$,(#1));\n\
             #3=IFCRELDEFINESBYPROPERTIES('guid',$,$,$,(#99),#2);",
            &[50],
        );
        assert!(index.rows(99).is_none());
        assert!(index.rows(50).is_none());
    }

    #[test]
    fn joins_enumerated_values() {
        let index = build(
            "#1=IFCPROPERTYENUMERATEDVALUE('Status',$,(IFCLABEL('New'),IFCLABEL('Existing')),$);\n\
             #2=IFCPROPERTYSET('guid',$,'Pset_Common',$,(#1));\n\
             #3=IFCRELDEFINESBYPROPERTIES('guid',$,$,$,(#50),#2);",
            &[50],
        );
        assert_eq!(index.rows(50).unwrap()[0].1, "New, Existing");
    }

    /// Empty values carry no information and would only pad the panel.
    #[test]
    fn drops_empty_values() {
        let index = build(
            "#1=IFCPROPERTYSINGLEVALUE('Comments',$,IFCTEXT(''),$);\n\
             #2=IFCPROPERTYSINGLEVALUE('Missing',$,$,$);\n\
             #3=IFCPROPERTYSET('guid',$,'Pset_Common',$,(#1,#2));\n\
             #4=IFCRELDEFINESBYPROPERTIES('guid',$,$,$,(#50),#3);",
            &[50],
        );
        assert!(index.rows(50).is_none());
    }
}
