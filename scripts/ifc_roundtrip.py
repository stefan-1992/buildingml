#!/usr/bin/env python3
"""IFC -> BML -> IFC roundtrip report.

Decompiles an IFC file to BML, compiles it back, and compares the two IFC
files element by element. Elements are matched by GlobalId (the decompiler
keeps it as the `guid:` property). For each matched element the world-space
bounding boxes are compared; an element passes when all six bounds agree
within the tolerance.

Usage:
    python3 scripts/ifc_roundtrip.py tests/fixtures/model_1.ifc
    python3 scripts/ifc_roundtrip.py model.ifc --tol 0.05 --json
    python3 scripts/ifc_roundtrip.py model.ifc --keep out/   # keep BML + IFC
    python3 scripts/ifc_roundtrip.py --compare a.ifc b.ifc   # skip the pipeline

Requires: pip install ifcopenshell numpy
"""

import argparse
import collections
import json
import multiprocessing
import os
import subprocess
import sys
import tempfile

import ifcopenshell
import ifcopenshell.geom
import ifcopenshell.util.element
import numpy as np


def bboxes(path):
    """GlobalId -> (class, storey name, bbox min, bbox max) for every product with a body."""
    f = ifcopenshell.open(path)
    s = ifcopenshell.geom.settings()
    s.set("use-world-coords", True)
    s.set("disable-opening-subtractions", True)
    products = [
        p
        for p in f.by_type("IfcProduct")
        if p.Representation and (p.is_a("IfcSpace") or not p.is_a("IfcSpatialStructureElement"))
    ]
    out = {}
    if not products:
        return out
    it = ifcopenshell.geom.iterator(s, f, multiprocessing.cpu_count(), include=products)
    if it.initialize():
        while True:
            sh = it.get()
            v = np.array(sh.geometry.verts).reshape(-1, 3)
            if len(v):
                e = f.by_id(sh.id)
                c = ifcopenshell.util.element.get_container(e)
                if c is None and e.is_a("IfcSpace"):
                    c = ifcopenshell.util.element.get_aggregate(e)
                lo, hi = v.min(0), v.max(0)
                if e.GlobalId in out:  # one shape per body representation
                    lo, hi = np.minimum(lo, out[e.GlobalId][2]), np.maximum(hi, out[e.GlobalId][3])
                out[e.GlobalId] = (e.is_a(), c.Name if c else None, lo, hi)
            if not it.next():
                break
    return out


def compare(orig, rt, tol):
    rows = collections.defaultdict(collections.Counter)
    worst = []
    for gid, (cls, storey, lo, hi) in orig.items():
        r = rows[cls]
        r["total"] += 1
        m = rt.get(gid)
        if m is None:
            r["missing"] += 1
            continue
        r["matched"] += 1
        if m[1] == storey:
            r["storey_ok"] += 1
        err = float(max(np.abs(m[2] - lo).max(), np.abs(m[3] - hi).max()))
        if err <= tol:
            r["bbox_ok"] += 1
        else:
            worst.append((err, cls, gid))
    worst.sort(reverse=True)
    return rows, worst


def run_pipeline(src, workdir, binary):
    bml_dir = os.path.join(workdir, "bml")
    out_ifc = os.path.join(workdir, "roundtrip.ifc")
    for cmd in (
        [binary, "decompile", src, "-o", bml_dir],
        [binary, "compile", bml_dir, "-o", out_ifc],
    ):
        p = subprocess.run(cmd, capture_output=True, text=True)
        if p.returncode != 0:
            sys.exit(f"{' '.join(cmd)} failed:\n{p.stderr}{p.stdout}")
    return out_ifc


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("ifc", nargs="+", help="original IFC (or two files with --compare)")
    ap.add_argument("--compare", action="store_true", help="compare two IFC files, no pipeline")
    ap.add_argument("--tol", type=float, default=0.05, help="bbox tolerance in metres (default 0.05)")
    ap.add_argument("--bin", default="target/release/buildingml", help="buildingml binary")
    ap.add_argument("--keep", help="directory to keep the BML and roundtrip IFC in")
    ap.add_argument("--json", action="store_true", help="JSON output")
    ap.add_argument("--worst", type=int, default=10, help="list the N largest bbox errors")
    a = ap.parse_args()

    if a.compare:
        if len(a.ifc) != 2:
            sys.exit("--compare takes two IFC files")
        src, out_ifc = a.ifc
    else:
        src = a.ifc[0]
        workdir = a.keep or tempfile.mkdtemp(prefix="bml-roundtrip-")
        os.makedirs(workdir, exist_ok=True)
        out_ifc = run_pipeline(src, workdir, a.bin)

    rows, worst = compare(bboxes(src), bboxes(out_ifc), a.tol)
    tot = collections.Counter()
    for r in rows.values():
        tot.update(r)

    if a.json:
        print(json.dumps({"classes": rows, "total": tot, "worst": worst[: a.worst]}, indent=2))
        return
    print(f"{'class':28s} {'total':>6s} {'match':>6s} {'storey':>6s} {'bbox':>6s}")
    for cls in sorted(rows):
        r = rows[cls]
        print(f"{cls:28s} {r['total']:6d} {r['matched']:6d} {r['storey_ok']:6d} {r['bbox_ok']:6d}")
    pct = 100.0 * tot["bbox_ok"] / max(tot["total"], 1)
    print(f"{'TOTAL':28s} {tot['total']:6d} {tot['matched']:6d} {tot['storey_ok']:6d} {tot['bbox_ok']:6d}  ({pct:.1f}% within {a.tol} m)")
    for err, cls, gid in worst[: a.worst]:
        print(f"  {err:8.3f} m  {cls:24s} {gid}")


if __name__ == "__main__":
    main()
