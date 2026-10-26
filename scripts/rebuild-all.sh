#!/usr/bin/env bash
set -euo pipefail

# Rebuild everything after big changes:
#   1. Rust binary (cargo build)
#   2. Web frontend (npm build)
#   3. Shared renderer IIFE bundle (for VS Code extension)
#   4. VS Code extension TypeScript

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "=== 1/5 Cargo build ==="
export PATH="$HOME/.cargo/bin:$PATH"
cargo build
echo ""

echo "=== 2/5 Web frontend (npm install + build) ==="
cd "$ROOT/web"
npm install --silent
npm run build
echo ""

echo "=== 3/5 Shared renderer IIFE bundle (for VS Code) ==="
npm run build:lib
echo ""

echo "=== 4/5 VS Code extension (npm install + compile) ==="
cd "$ROOT/editors/vscode"
npm install --silent
npm run compile
echo ""

echo "=== 5/5 Cargo install (update global binary) ==="
cd "$ROOT"
cargo install --path crates/bml-cli
echo ""

echo "All builds complete! Reload VS Code to pick up extension changes."
