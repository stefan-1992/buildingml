#!/usr/bin/env bash
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
# The repo's uv env (.venv, with ifcopenshell) wins over the system python3, for
# validate_ifc_all.sh and the IfcOpenShell tests in `cargo test` alike.
VENV_BIN="$(cd "$(dirname "$0")/.." && pwd)/.venv/bin"
if [ -d "$VENV_BIN" ]; then export PATH="$VENV_BIN:$PATH"; fi
echo "=== file sizes ==="
"$(dirname "$0")/check_file_sizes.sh"
echo ""
echo "=== cargo test ==="
cargo test
echo ""
echo "=== cargo clippy ==="
cargo clippy -- -D warnings
echo ""
# IfcOpenShell validation (skipped gracefully if not installed)
if python3 -c "import ifcopenshell" 2>/dev/null; then
    echo "=== IfcOpenShell IFC validation ==="
    SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
    "$SCRIPT_DIR/validate_ifc_all.sh"
    echo ""
else
    echo "(Skipping IfcOpenShell validation — not installed)"
    echo ""
fi
echo "All checks passed!"
