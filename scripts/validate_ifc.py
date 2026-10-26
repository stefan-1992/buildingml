#!/usr/bin/env python3
"""IfcOpenShell validator for BuildingML-generated IFC files.

Validates IFC output against the IFC4 schema using IfcOpenShell.
Designed to be called from Rust integration tests and CI pipelines.

Usage:
    # Validate a single file
    python3 scripts/validate_ifc.py output.ifc

    # Validate multiple files
    python3 scripts/validate_ifc.py *.ifc

    # JSON output for programmatic use (from Rust tests)
    python3 scripts/validate_ifc.py --json output.ifc

    # Strict mode: exit 1 on any error (default for CI)
    python3 scripts/validate_ifc.py --strict output.ifc

    # Compile BML to IFC then validate (requires buildingml in PATH)
    python3 scripts/validate_ifc.py --compile input.bml

    # Ignore specific error categories
    python3 scripts/validate_ifc.py --ignore guid_base64 --ignore empty_set output.ifc

    # Also validate geometry (slower but more thorough)
    python3 scripts/validate_ifc.py --geometry output.ifc

Exit codes:
    0 - All files valid (or only ignored errors found)
    1 - Validation errors found
    2 - File not found or could not be opened
    3 - IfcOpenShell not installed

Requires: pip install ifcopenshell
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile
from collections import Counter
from dataclasses import asdict, dataclass, field
from pathlib import Path


def check_ifcopenshell():
    """Check if ifcopenshell is available."""
    try:
        import ifcopenshell  # noqa: F401
        import ifcopenshell.validate  # noqa: F401

        return True
    except ImportError:
        return False


# Error categories for filtering
CATEGORY_GUID_BASE64 = "guid_base64"
CATEGORY_EMPTY_SET = "empty_set"
CATEGORY_INVERSE = "inverse"
CATEGORY_SCHEMA = "schema"
CATEGORY_OTHER = "other"


@dataclass
class ValidationError:
    """A single IFC validation error."""

    level: str
    category: str
    attribute: str
    entity_type: str
    entity_id: int
    entity_name: str
    message: str


@dataclass
class GeometryFailure:
    """A geometry processing failure for a single IFC product."""

    entity_type: str
    entity_id: int
    entity_name: str
    global_id: str
    error: str


@dataclass
class ValidationResult:
    """Result of validating a single IFC file."""

    file: str
    schema: str
    entity_count: int
    errors: list[ValidationError] = field(default_factory=list)
    error_count: int = 0
    warning_count: int = 0
    categories: dict[str, int] = field(default_factory=dict)
    geometry_failures: list[GeometryFailure] = field(default_factory=list)
    geometry_product_count: int = 0
    valid: bool = True


def categorize_error(stmt: dict) -> str:
    """Classify an IfcOpenShell validation error into a category."""
    msg = stmt.get("message", "")
    attr = stmt.get("attribute", "")

    if "base64" in msg.lower() or "GlobalId" in msg:
        return CATEGORY_GUID_BASE64
    if "SET [1:?]" in msg or ("()" in msg and "Not valid" in msg):
        if "inverse" in attr.lower() or "AssociatedTo" in attr:
            return CATEGORY_INVERSE
        return CATEGORY_EMPTY_SET
    if stmt.get("type") == "schema":
        return CATEGORY_SCHEMA
    return CATEGORY_OTHER


def extract_entity_info(stmt: dict) -> tuple[str, int, str]:
    """Extract entity type, ID, and name from a validation statement."""
    inst = stmt.get("instance")
    if inst is None:
        return ("unknown", 0, "")

    inst_str = str(inst)
    # Try to get entity type
    entity_type = ""
    entity_id = 0
    entity_name = ""

    try:
        entity_type = inst.is_a()
    except (AttributeError, RuntimeError):
        pass
    try:
        entity_id = inst.id()
    except (AttributeError, RuntimeError):
        pass
    try:
        entity_name = inst.Name or ""
    except (AttributeError, RuntimeError):
        pass

    return (entity_type, entity_id, entity_name)


def validate_geometry(filepath: str, result: ValidationResult) -> None:
    """Validate geometry of all IfcProduct instances using IfcOpenShell.

    Attempts to create tessellated shapes for every product with a
    representation. Failures indicate malformed geometry.
    """
    import ifcopenshell
    import ifcopenshell.geom

    model = ifcopenshell.open(filepath)
    settings = ifcopenshell.geom.settings()

    products = [p for p in model.by_type("IfcProduct") if p.Representation]
    result.geometry_product_count = len(products)

    for product in products:
        try:
            ifcopenshell.geom.create_shape(settings, product)
        except RuntimeError as e:
            failure = GeometryFailure(
                entity_type=product.is_a(),
                entity_id=product.id(),
                entity_name=getattr(product, "Name", None) or "",
                global_id=product.GlobalId,
                error=str(e),
            )
            result.geometry_failures.append(failure)
            result.valid = False


def validate_file(
    filepath: str, ignore_categories: set[str] | None = None, check_geometry: bool = False,
) -> ValidationResult:
    """Validate an IFC file using IfcOpenShell.

    Args:
        filepath: Path to the IFC file.
        ignore_categories: Set of error category names to ignore.
        check_geometry: If True, also validate geometry via create_shape().

    Returns:
        ValidationResult with errors and summary.
    """
    import ifcopenshell
    import ifcopenshell.validate

    ignore_categories = ignore_categories or set()

    result = ValidationResult(file=filepath, schema="", entity_count=0)

    try:
        model = ifcopenshell.open(filepath)
    except Exception as e:
        result.valid = False
        result.errors.append(
            ValidationError(
                level="error",
                category="file_error",
                attribute="",
                entity_type="",
                entity_id=0,
                entity_name="",
                message=f"Failed to open IFC file: {e}",
            )
        )
        result.error_count = 1
        return result

    result.schema = model.schema
    result.entity_count = len(list(model))

    logger = ifcopenshell.validate.json_logger()
    ifcopenshell.validate.validate(model, logger)

    categories: Counter[str] = Counter()
    for stmt in logger.statements:
        category = categorize_error(stmt)
        entity_type, entity_id, entity_name = extract_entity_info(stmt)
        level = stmt.get("level", "error")

        if category in ignore_categories:
            continue

        categories[category] += 1
        error = ValidationError(
            level=level,
            category=category,
            attribute=stmt.get("attribute", ""),
            entity_type=entity_type,
            entity_id=entity_id,
            entity_name=entity_name,
            message=stmt.get("message", "").strip(),
        )
        result.errors.append(error)

        if level == "error":
            result.error_count += 1
        elif level == "warning":
            result.warning_count += 1

    result.categories = dict(categories)
    result.valid = result.error_count == 0

    if check_geometry:
        validate_geometry(filepath, result)

    return result


def compile_bml_to_ifc(bml_path: str, buildingml_bin: str = "buildingml") -> str | None:
    """Compile a BML file to a temporary IFC file.

    Returns the path to the temporary IFC file, or None on failure.
    """
    tmp = tempfile.NamedTemporaryFile(suffix=".ifc", delete=False)
    tmp.close()

    try:
        result = subprocess.run(
            [buildingml_bin, "compile", bml_path, "-o", tmp.name],
            capture_output=True,
            text=True,
            timeout=60,
        )
        if result.returncode != 0:
            print(f"Compilation failed for {bml_path}:", file=sys.stderr)
            print(result.stderr, file=sys.stderr)
            os.unlink(tmp.name)
            return None
        return tmp.name
    except FileNotFoundError:
        print(
            f"buildingml binary not found at '{buildingml_bin}'",
            file=sys.stderr,
        )
        os.unlink(tmp.name)
        return None
    except subprocess.TimeoutExpired:
        print(f"Compilation timed out for {bml_path}", file=sys.stderr)
        os.unlink(tmp.name)
        return None


def format_human(result: ValidationResult) -> str:
    """Format validation result for human consumption."""
    lines = []
    basename = os.path.basename(result.file)

    if result.valid and not result.geometry_failures:
        geom_info = ""
        if result.geometry_product_count > 0:
            geom_info = f", {result.geometry_product_count} geometries OK"
        lines.append(f"  {basename}: VALID ({result.entity_count} entities, {result.schema}{geom_info})")
    else:
        lines.append(
            f"  {basename}: INVALID - {result.error_count} error(s), "
            f"{result.warning_count} warning(s) "
            f"({result.entity_count} entities, {result.schema})"
        )

        if result.categories:
            lines.append("    Error categories:")
            for cat, count in sorted(result.categories.items(), key=lambda x: -x[1]):
                lines.append(f"      {cat}: {count}")

        # Show first few unique errors per category
        seen: set[str] = set()
        for error in result.errors:
            key = f"{error.category}:{error.attribute}"
            if key not in seen:
                seen.add(key)
                name_info = f" ({error.entity_name})" if error.entity_name else ""
                lines.append(
                    f"    [{error.level}] #{error.entity_id} "
                    f"{error.entity_type}{name_info} - {error.category}"
                )
                # Truncate long messages
                msg_lines = error.message.split("\n")
                for msg_line in msg_lines[:4]:
                    lines.append(f"      {msg_line}")
                if len(msg_lines) > 4:
                    lines.append(f"      ... ({len(msg_lines) - 4} more lines)")

        if result.geometry_failures:
            lines.append(
                f"    Geometry failures: {len(result.geometry_failures)}"
                f" / {result.geometry_product_count} products"
            )
            for gf in result.geometry_failures[:10]:
                name_info = f" ({gf.entity_name})" if gf.entity_name else ""
                lines.append(
                    f"    [geom] #{gf.entity_id} {gf.entity_type}{name_info}: {gf.error[:120]}"
                )
            if len(result.geometry_failures) > 10:
                lines.append(
                    f"    ... and {len(result.geometry_failures) - 10} more geometry failures"
                )

    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(
        description="Validate IFC files using IfcOpenShell",
        epilog="Requires: pip install ifcopenshell",
    )
    parser.add_argument("files", nargs="+", help="IFC or BML files to validate")
    parser.add_argument(
        "--json", action="store_true", help="Output results as JSON"
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        help="Exit with code 1 on any validation error",
    )
    parser.add_argument(
        "--compile",
        action="store_true",
        help="Treat input as BML files and compile to IFC first",
    )
    parser.add_argument(
        "--buildingml-bin",
        default=None,
        help="Path to buildingml binary (default: auto-detect)",
    )
    parser.add_argument(
        "--geometry",
        action="store_true",
        help="Also validate geometry via IfcOpenShell create_shape()",
    )
    parser.add_argument(
        "--ignore",
        action="append",
        default=[],
        choices=[
            CATEGORY_GUID_BASE64,
            CATEGORY_EMPTY_SET,
            CATEGORY_INVERSE,
            CATEGORY_SCHEMA,
            CATEGORY_OTHER,
        ],
        help="Ignore specific error categories",
    )

    args = parser.parse_args()

    if not check_ifcopenshell():
        print(
            "Error: ifcopenshell is not installed. Run: pip install ifcopenshell",
            file=sys.stderr,
        )
        sys.exit(3)

    # Auto-detect buildingml binary
    buildingml_bin = args.buildingml_bin
    if buildingml_bin is None:
        # Try common locations relative to project root
        script_dir = Path(__file__).parent
        project_root = script_dir.parent
        candidates = [
            project_root / "target" / "release" / "buildingml",
            project_root / "target" / "debug" / "buildingml",
        ]
        for candidate in candidates:
            if candidate.exists():
                buildingml_bin = str(candidate)
                break
        if buildingml_bin is None:
            buildingml_bin = "buildingml"  # Fall back to PATH

    ignore_categories = set(args.ignore)
    results: list[ValidationResult] = []
    temp_files: list[str] = []

    for filepath in args.files:
        if not os.path.exists(filepath):
            print(f"Error: file not found: {filepath}", file=sys.stderr)
            sys.exit(2)

        ifc_path = filepath
        if args.compile or filepath.endswith(".bml"):
            ifc_path_opt = compile_bml_to_ifc(filepath, buildingml_bin)
            if ifc_path_opt is None:
                sys.exit(2)
            ifc_path = ifc_path_opt
            temp_files.append(ifc_path)

        result = validate_file(ifc_path, ignore_categories, check_geometry=args.geometry)
        # Use original filename in results when compiling
        if filepath != ifc_path:
            result.file = filepath
        results.append(result)

    # Clean up temp files
    for tmp in temp_files:
        try:
            os.unlink(tmp)
        except OSError:
            pass

    # Output
    if args.json:
        output = {
            "results": [asdict(r) for r in results],
            "total_errors": sum(r.error_count for r in results),
            "total_warnings": sum(r.warning_count for r in results),
            "all_valid": all(r.valid for r in results),
        }
        print(json.dumps(output, indent=2))
    else:
        any_invalid = False
        print("IfcOpenShell Validation Results:")
        print("=" * 50)
        for result in results:
            print(format_human(result))
            if not result.valid:
                any_invalid = True
            print()

        total_errors = sum(r.error_count for r in results)
        total_warnings = sum(r.warning_count for r in results)
        if any_invalid:
            print(f"FAILED: {total_errors} error(s), {total_warnings} warning(s)")
        else:
            print(f"PASSED: {len(results)} file(s) valid")

    # Exit code
    if args.strict or not args.json:
        any_errors = any(not r.valid or r.geometry_failures for r in results)
        sys.exit(1 if any_errors else 0)
    else:
        sys.exit(0)


if __name__ == "__main__":
    main()
