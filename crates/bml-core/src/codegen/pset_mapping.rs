use ifc_rs::prelude::*;

use crate::semantic::model::{self, ResolvedProperty, ResolvedValue};

/// Map a BuildingML property to an IFC property set entry.
/// Returns (pset_name, ifc_property_name, ifc_value) if mapped, None otherwise.
///
/// The common keys go to `Pset_<Type>Common` when that set defines them.
pub fn map_property(
    element_type: model::ElementType,
    prop: &ResolvedProperty,
) -> Option<(&'static str, &'static str, IfcValue)> {
    use model::ElementType as T;
    let pset = match element_type {
        T::Wall => "Pset_WallCommon",
        T::Slab => "Pset_SlabCommon",
        T::Column => "Pset_ColumnCommon",
        T::Beam => "Pset_BeamCommon",
        T::Window => "Pset_WindowCommon",
        T::Door => "Pset_DoorCommon",
        T::Roof => "Pset_RoofCommon",
        T::Stair => "Pset_StairCommon",
        T::Railing => "Pset_RailingCommon",
        T::Plate => "Pset_PlateCommon",
        T::Member => "Pset_MemberCommon",
        T::Covering => "Pset_CoveringCommon",
        _ => return None,
    };
    let structural = matches!(
        element_type,
        T::Wall | T::Slab | T::Column | T::Beam | T::Plate | T::Member
    );
    let envelope = matches!(
        element_type,
        T::Wall | T::Slab | T::Roof | T::Window | T::Door | T::Plate | T::Covering
    );
    let (name, value) = match (prop.key.as_str(), &prop.value) {
        ("slab_type", ResolvedValue::Identifier(s)) if element_type == T::Slab => {
            ("Status", label_value(s))
        }
        ("is_external", ResolvedValue::Boolean(b)) => ("IsExternal", bool_value(*b)),
        ("load_bearing", ResolvedValue::Boolean(b)) if structural => {
            ("LoadBearing", bool_value(*b))
        }
        // Pset_RailingCommon has no FireRating
        ("fire_rating", ResolvedValue::Identifier(s) | ResolvedValue::String(s))
            if element_type != T::Railing =>
        {
            ("FireRating", label_value(s))
        }
        // Windows also take `u_value`
        (k @ ("thermal_transmittance" | "u_value"), ResolvedValue::Number(n))
            if envelope && (k != "u_value" || element_type == T::Window) =>
        {
            (
                "ThermalTransmittance",
                IfcValue::ThermalTransmittance((*n).into()),
            )
        }
        ("acoustic_rating", ResolvedValue::Number(n)) if envelope => {
            ("AcousticRating", real_value(*n))
        }
        _ => return None,
    };
    Some((pset, name, value))
}

fn bool_value(b: bool) -> IfcValue {
    // BoolPrimitive is pub(crate) in ifc_rs, so we use Label as a workaround
    if b {
        IfcValue::Label(LabelValue("TRUE".into()))
    } else {
        IfcValue::Label(LabelValue("FALSE".into()))
    }
}

fn label_value(s: &str) -> IfcValue {
    IfcValue::Label(LabelValue(s.into()))
}

fn real_value(n: f64) -> IfcValue {
    IfcValue::Real(RealValue(n.into()))
}
