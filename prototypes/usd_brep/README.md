# USD BRep prototype

Exploratory prototype: builds exact BRep solids from a BuildingML model with
NVIDIA's open-source [USD BRep](https://pypi.org/project/usd-brep/) kernel
(SMLib, Apache 2.0). Background, results and integration options are in
`Specs_V0_3.md` §14.

Not part of the build, tests or CI.

## Setup

The wheel needs CPython 3.12 (Linux/Windows):

```bash
uv venv -p 3.12 .venv-brep
uv pip install -p .venv-brep/bin/python usd-brep
```

## Run

```bash
cargo build -p bml-cli
./target/debug/buildingml model tests/fixtures/two_storey_house.bml > /tmp/model.json
.venv-brep/bin/python prototypes/usd_brep/bml_to_brep.py /tmp/model.json -o /tmp/brep_out --union-walls
```

Output:

- `model_brep.usda`: exact BRep solids (`BrepArray` schema)
- `model_mesh.usda`: tessellated `UsdGeom.Mesh`, which opens in any USD viewer
- `report.json`: per-element net/gross volume, face count and manifold check.
  With `--union-walls`, it also includes one merged wall solid per storey. Comparing
  that solid's volume with the sum of the individual walls shows overlapping walls.

Harmless `No module named 'pxr'` warnings come from the OpenUSD runtime bundled in the wheel.

## Scope

Supported: straight walls with door/window openings, polygon slabs, rectangular
columns and beams. Not supported: curved walls (`via:`), roofs, stairs,
furniture, wall end trimming at junctions.
