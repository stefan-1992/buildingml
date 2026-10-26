#!/usr/bin/env bash
# Fail if any tracked file exceeds the size limit (default 20 MB).
set -euo pipefail
LIMIT_MB="${1:-20}"
LIMIT=$((LIMIT_MB * 1024 * 1024))
# Files allowed over the limit
EXCEPTIONS="tests/fixtures/model_1_generated.ifc"
cd "$(git rev-parse --show-toplevel)"
big=$(git ls-files -z | xargs -0 wc -c 2>/dev/null | awk -v l="$LIMIT" -v ex=" $EXCEPTIONS " '$2 != "total" && $1 > l && index(ex, " " $2 " ") == 0 { printf "  %.1f MB  %s\n", $1/1048576, $2 }')
if [ -n "$big" ]; then
    echo "Files over ${LIMIT_MB} MB (keep large fixtures out of git):"
    echo "$big"
    exit 1
fi
echo "No tracked files over ${LIMIT_MB} MB"
