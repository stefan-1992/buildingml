#!/bin/bash
# Start backend + Vite dev server for local development
# Usage: ./scripts/dev.sh [bml-file]

BML_FILE="${1:-tests/fixtures/six_storey_residential.bml}"
PORT=8080

echo "Starting backend on :$PORT with $BML_FILE"
echo "Starting Vite dev server on :5173"
echo "Open http://localhost:5173"
echo ""

cargo run --bin buildingml -- preview "$BML_FILE" --examples-dir tests/fixtures/ --port $PORT &
BACKEND_PID=$!

cd web && npm run dev &
VITE_PID=$!

trap "kill $BACKEND_PID $VITE_PID 2>/dev/null" EXIT
wait
