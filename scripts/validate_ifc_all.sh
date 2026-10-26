#!/usr/bin/env bash
# Validate all compilable BML fixtures through IfcOpenShell.
# This is the "definition of done" check for the IFC writer.
#
# Usage:
#   ./scripts/validate_ifc_all.sh            # Compile + validate all fixtures
#   ./scripts/validate_ifc_all.sh --geometry  # Also check geometry processing
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Auto-detect buildingml binary
if [ -f "$PROJECT_ROOT/target/release/buildingml" ]; then
    BML_BIN="$PROJECT_ROOT/target/release/buildingml"
elif [ -f "$PROJECT_ROOT/target/debug/buildingml" ]; then
    BML_BIN="$PROJECT_ROOT/target/debug/buildingml"
else
    echo "Error: buildingml binary not found. Run 'cargo build' first."
    exit 1
fi

# Check if ifcopenshell is available
if ! python3 -c "import ifcopenshell" 2>/dev/null; then
    echo "Warning: ifcopenshell not installed. Run: pip install ifcopenshell"
    echo "Skipping IFC validation."
    exit 0
fi

GEOMETRY_FLAG=""
if [[ "${1:-}" == "--geometry" ]]; then
    GEOMETRY_FLAG="--geometry"
fi

echo "=== IfcOpenShell IFC Validation ==="
echo "Binary: $BML_BIN"
echo ""

# Fixtures that compile successfully
FIXTURES=(
    "minimal.bml"
    "wall_only.bml"
    "slab_only.bml"
    "spatial_hierarchy.bml"
    "two_storey_house.bml"
    "full_example.bml"
    "furniture_assets.bml"
    "phase7c_geometry.bml"
    "six_storey_residential.bml"
    "layered_materials.bml"
    "curves.bml"
    "university_campus_project"
    "university_campus_lod300_project"
    "university_campus_lod350_project"
    "university_campus_fluid_project"
)

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

PASS=0
FAIL=0

for fixture in "${FIXTURES[@]}"; do
    bml_file="$PROJECT_ROOT/tests/fixtures/$fixture"
    if [ ! -e "$bml_file" ]; then
        echo "  FAIL $fixture (not found)"
        FAIL=$((FAIL + 1))
        continue
    fi

    ifc_file="$TMPDIR/${fixture%.bml}.ifc"

    # Compile BML to IFC
    if ! "$BML_BIN" compile "$bml_file" -o "$ifc_file" 2>/dev/null; then
        echo "  FAIL $fixture (compilation error)"
        FAIL=$((FAIL + 1))
        continue
    fi

    # Validate with IfcOpenShell
    if python3 "$SCRIPT_DIR/validate_ifc.py" $GEOMETRY_FLAG "$ifc_file" 2>/dev/null; then
        PASS=$((PASS + 1))
    else
        echo ""
        FAIL=$((FAIL + 1))
    fi
done

echo ""
echo "Results: $PASS passed, $FAIL failed"

if [ "$FAIL" -gt 0 ]; then
    echo "FAILED: IFC validation errors found"
    exit 1
else
    echo "PASSED: All IFC files are valid"
    exit 0
fi
