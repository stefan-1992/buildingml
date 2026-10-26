#!/usr/bin/env python3
"""Prototype: build exact BRep solids from a BuildingML model using NVIDIA USD BRep.

Reads the semantic model JSON produced by `buildingml model <file.bml>` and
builds walls (with door/window openings cut by real booleans), slabs, columns
and beams as BRep solids. Writes:

  <out>/model_brep.usda   exact BRep solids (omniSolid BrepArray schema)
  <out>/model_mesh.usda   tessellated meshes (plain UsdGeom, viewable anywhere)
  <out>/report.json       per-element gross/net volumes and validity

Usage:
  buildingml model house.bml > model.json
  python bml_to_brep.py model.json -o out/ [--union-walls]

Requires `pip install usd-brep` (CPython 3.12, Linux/Windows x86_64).
"""

import argparse
import json
import math
import os
import sys

import usd_brep as ub

EPS = 0.01  # opening tool overshoot through the wall thickness (m)


def props(element):
    """Flatten [{key, value: {Kind: v}}] into {key: v}."""
    out = {}
    for p in element.get("properties", []):
        (kind, val), = p["value"].items()
        out[p["key"]] = val
    return out


def storey_elements(storey):
    yield from storey.get("elements", [])
    for space in storey.get("spaces", []):
        yield from space.get("elements", [])


def make_wall(p, children, z0):
    axis = p["from"]
    (x1, y1), (x2, y2) = axis["from"], axis["to"]
    length = math.hypot(x2 - x1, y2 - y1)
    h, t = p.get("height", 3.0), p.get("thickness", 0.2)
    if axis.get("via") or length <= 0:
        return None, 0  # curved walls: out of scope for the prototype

    # Build in a local frame: wall runs along +X, centred on Y=0, base at Z=0.
    solid = ub.create_box((0.0, -t / 2, 0.0), length, t, h)
    n_openings = 0
    for c in children:
        cp = props(c)
        if c["element_type"] not in ("Door", "Window"):
            continue
        is_door = c["element_type"] == "Door"
        w = cp.get("width", 0.9 if is_door else 1.0)
        oh = cp.get("height", 2.1 if is_door else 1.0)
        sill = 0.0 if is_door else cp.get("sill_height", 0.9)
        off = cp.get("offset", 0.0)
        top = min(sill + oh, h)
        # Doors at floor level: drop the tool slightly below Z=0 to avoid coplanar faces.
        bottom = sill if sill > 0 else -EPS
        tool = ub.create_box((off, -t / 2 - EPS, bottom), w, t + 2 * EPS, top - bottom)
        solid = ub.boolean_difference(solid, tool)
        n_openings += 1

    ub.rotate(solid, (0, 0, 0), (0, 0, 1), math.degrees(math.atan2(y2 - y1, x2 - x1)))
    ub.translate(solid, (x1, y1, z0))
    return solid, n_openings


def make_slab(p, z0):
    pts = p.get("boundary")
    if not pts:
        return None
    t = p.get("thickness", 0.2)
    # Slab sits below the storey elevation (top face = storey floor level).
    loop = [
        ub.create_line_segment((a[0], a[1], z0), (b[0], b[1], z0))
        for a, b in zip(pts, pts[1:] + pts[:1])
    ]
    return ub.linear_sweep(loop, (0, 0, -1), t)


def make_column(p, z0, storey_h):
    x, y = p["position"]
    w, d = (p.get("profile", {}).get("params") or [0.3, 0.3])[:2]
    return ub.create_box((x - w / 2, y - d / 2, z0), w, d, p.get("height", storey_h))


def make_beam(p, z0, storey_h):
    (x1, y1), (x2, y2) = p["from"]["from"], p["from"]["to"]
    length = math.hypot(x2 - x1, y2 - y1)
    w, d = (p.get("profile", {}).get("params") or [0.3, 0.5])[:2]
    solid = ub.create_box((0.0, -w / 2, storey_h - d), length, w, d)
    ub.rotate(solid, (0, 0, 0), (0, 0, 1), math.degrees(math.atan2(y2 - y1, x2 - x1)))
    ub.translate(solid, (x1, y1, z0))
    return solid


def build(model, union_walls=False):
    solids, report = [], []
    for site in model["sites"]:
        for building in site["buildings"]:
            for storey in building["storeys"]:
                sp = props(storey)
                z0, sh = sp.get("elevation", 0.0), sp.get("height", 3.0)
                walls = []
                for e in storey_elements(storey):
                    p, kind = props(e), e["element_type"]
                    entry = {"id": e["id"], "type": kind, "storey": storey["name"]}
                    try:
                        if kind == "Wall":
                            s, n = make_wall(p, e.get("children", []), z0)
                            if s is not None:
                                (x1, y1), (x2, y2) = p["from"]["from"], p["from"]["to"]
                                gross = math.hypot(x2 - x1, y2 - y1) * p.get("thickness", 0.2) * p.get("height", 3.0)
                                entry.update(openings=n, gross_volume=round(gross, 4))
                        elif kind == "Slab":
                            s = make_slab(p, z0)
                        elif kind == "Column":
                            s = make_column(p, z0, sh)
                        elif kind == "Beam":
                            s = make_beam(p, z0, sh)
                        else:
                            continue
                    except Exception as exc:  # report and keep going
                        entry["error"] = str(exc)
                        report.append(entry)
                        continue
                    if s is None:
                        entry["skipped"] = "unsupported geometry"
                        report.append(entry)
                        continue
                    entry.update(
                        net_volume=round(ub.compute_volume(s), 4),
                        manifold=ub.is_manifold_solid(s),
                        faces=s.face_count(),
                    )
                    report.append(entry)
                    (walls if kind == "Wall" else solids).append(s)

                if union_walls and walls:
                    # Merge all walls of the storey into one solid: junctions become clean.
                    merged = walls[0]
                    for w in walls[1:]:
                        merged = ub.boolean_union(merged, w)
                    report.append({
                        "id": f"walls:{storey['name']}", "type": "WallUnion",
                        "net_volume": round(ub.compute_volume(merged), 4),
                        "manifold": ub.is_manifold_solid(merged),
                        "faces": merged.face_count(),
                    })
                    solids.append(merged)
                else:
                    solids.extend(walls)
    return solids, report


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("model", help="JSON from `buildingml model` (use - for stdin)")
    ap.add_argument("-o", "--out", default="brep_out")
    ap.add_argument("--union-walls", action="store_true", help="boolean-union walls per storey")
    args = ap.parse_args()

    model = json.load(sys.stdin if args.model == "-" else open(args.model))
    solids, report = build(model, args.union_walls)

    os.makedirs(args.out, exist_ok=True)
    meshes = [ub.tessellate(s, chord_height_tolerance=0.01) for s in solids]
    ub.usd.export_breps(solids, os.path.join(args.out, "model_brep.usda"))
    ub.usd.export_meshes(meshes, os.path.join(args.out, "model_mesh.usda"))
    with open(os.path.join(args.out, "report.json"), "w") as f:
        json.dump(report, f, indent=2)

    walls = [r for r in report if r["type"] == "Wall" and "net_volume" in r]
    errors = [r for r in report if "error" in r]
    print(f"solids: {len(solids)}  errors: {len(errors)}  "
          f"non-manifold: {sum(1 for r in report if r.get('manifold') is False)}")
    if walls:
        gross = sum(r["gross_volume"] for r in walls)
        net = sum(r["net_volume"] for r in walls)
        print(f"walls: {len(walls)}  gross {gross:.2f} m3  net {net:.2f} m3  "
              f"openings removed {gross - net:.2f} m3")
    for r in errors:
        print(f"  error {r['id']}: {r['error']}")


if __name__ == "__main__":
    main()
