/** BML code generation from floorplan elements. */

import type { FloorplanElement, WallElement, ColumnElement, SlabElement, FurnitureElement, DetectedSpace, Point2D } from './element-model.ts';
import { pointInPolygon } from './space-detection.ts';
import type { PlanRef } from './model-importer.ts';

function fmtNum(n: number): string {
  if (Number.isInteger(n) || Math.abs(n - Math.round(n)) < 0.0001) return Math.round(n).toString();
  let s = n.toFixed(4);
  s = s.replace(/\.?0+$/, '');
  return s;
}

function wallToBml(wall: WallElement, elements: FloorplanElement[], indent: string): string {
  let bml = '';
  bml += `${indent}Wall: ${wall.id}\n`;
  bml += `${indent}  from: (${fmtNum(wall.from.x)}, ${fmtNum(wall.from.y)}) to: (${fmtNum(wall.to.x)}, ${fmtNum(wall.to.y)})\n`;
  bml += `${indent}  height: ${fmtNum(wall.height)} m\n`;
  bml += `${indent}  thickness: ${fmtNum(wall.thickness)} m\n`;

  const children = elements.filter(e =>
    (e.type === 'Door' || e.type === 'Window') && 'parentWall' in e && e.parentWall === wall.id
  );
  for (const child of children) {
    if (child.type === 'Window' && 'sill_height' in child) {
      bml += `${indent}  Window: ${child.id}\n`;
      bml += `${indent}    offset: ${fmtNum(child.offset)} m\n`;
      bml += `${indent}    width: ${fmtNum(child.width)} m\n`;
      bml += `${indent}    height: ${fmtNum(child.height)} m\n`;
      bml += `${indent}    sill_height: ${fmtNum(child.sill_height)} m\n`;
    } else if (child.type === 'Door' && 'offset' in child) {
      bml += `${indent}  Door: ${child.id}\n`;
      bml += `${indent}    offset: ${fmtNum(child.offset)} m\n`;
      bml += `${indent}    width: ${fmtNum(child.width)} m\n`;
      bml += `${indent}    height: ${fmtNum(child.height)} m\n`;
    }
  }
  bml += '\n';
  return bml;
}

/** Generate just the storey block (### Storey: ...). */
export function generateStoreyBml(
  elements: FloorplanElement[],
  detectedSpaces: DetectedSpace[],
  storeyName: string,
  elevation: number,
  height: number,
): string {
  let bml = `### Storey: ${storeyName}\n`;
  bml += `  elevation: ${fmtNum(elevation)} m\n`;
  bml += `  height: ${fmtNum(height)} m\n\n`;
  return bml + generateContentsBml(elements, detectedSpaces, true);
}

/**
 * Generate the spaces and elements of a plan. `defaultSpace` adds an empty
 * "Main Space" when no space was detected.
 */
function generateContentsBml(
  elements: FloorplanElement[],
  detectedSpaces: DetectedSpace[],
  defaultSpace: boolean,
): string {
  let bml = '';
  const assignedWalls = new Set<string>();
  const assignedColumns = new Set<string>();
  const assignedFurniture = new Set<string>();

  if (detectedSpaces.length > 0) {
    for (const space of detectedSpaces) {
      bml += `#### Space: ${space.name}\n`;
      bml += `  area: ${fmtNum(space.area)} m2\n\n`;

      for (const wallId of space.wallIds) {
        if (assignedWalls.has(wallId)) continue;
        const wall = elements.find(e => e.id === wallId) as WallElement | undefined;
        if (!wall) continue;
        assignedWalls.add(wallId);
        bml += wallToBml(wall, elements, '  ');
      }

      for (const col of elements.filter((e): e is ColumnElement => e.type === 'Column')) {
        if (assignedColumns.has(col.id)) continue;
        if (pointInPolygon(col, space.polygon)) {
          assignedColumns.add(col.id);
          bml += `  Column: ${col.id}\n`;
          bml += `    position: (${fmtNum(col.x)}, ${fmtNum(col.y)})\n`;
          bml += `    height: ${fmtNum(col.height)} m\n`;
          bml += `    profile: rectangle(${fmtNum(col.profileW)}, ${fmtNum(col.profileD)})\n\n`;
        }
      }

      for (const furn of elements.filter((e): e is FurnitureElement => e.type === 'Furniture')) {
        if (assignedFurniture.has(furn.id)) continue;
        if (pointInPolygon({ x: furn.x, y: furn.y }, space.polygon)) {
          assignedFurniture.add(furn.id);
          bml += `  Furniture: ${furn.id}\n`;
          bml += `    asset: ${furn.asset}\n`;
          bml += `    position: (${fmtNum(furn.x)}, ${fmtNum(furn.y)})\n`;
          bml += `    rotation: ${fmtNum(furn.rotation)}\n`;
          bml += `    width: ${fmtNum(furn.width)} m\n`;
          bml += `    depth: ${fmtNum(furn.depth)} m\n\n`;
        }
      }
    }
  } else if (defaultSpace) {
    bml += `#### Space: Main Space\n`;
    bml += `  area: 0 m2\n\n`;
  }

  // Unassigned walls
  for (const wall of elements.filter((e): e is WallElement => e.type === 'Wall')) {
    if (assignedWalls.has(wall.id)) continue;
    bml += wallToBml(wall, elements, '  ');
  }

  // Slabs
  for (const slab of elements.filter((e): e is SlabElement => e.type === 'Slab')) {
    bml += `  Slab: ${slab.id}\n`;
    const pts = slab.boundary.map((p: Point2D) => `(${fmtNum(p.x)},${fmtNum(p.y)})`).join(', ');
    bml += `    boundary: [${pts}]\n`;
    bml += `    thickness: ${fmtNum(slab.thickness)} m\n\n`;
  }

  // Unassigned columns
  for (const col of elements.filter((e): e is ColumnElement => e.type === 'Column')) {
    if (assignedColumns.has(col.id)) continue;
    bml += `  Column: ${col.id}\n`;
    bml += `    position: (${fmtNum(col.x)}, ${fmtNum(col.y)})\n`;
    bml += `    height: ${fmtNum(col.height)} m\n`;
    bml += `    profile: rectangle(${fmtNum(col.profileW)}, ${fmtNum(col.profileD)})\n\n`;
  }

  // Unassigned furniture
  for (const furn of elements.filter((e): e is FurnitureElement => e.type === 'Furniture')) {
    if (assignedFurniture.has(furn.id)) continue;
    bml += `  Furniture: ${furn.id}\n`;
    bml += `    asset: ${furn.asset}\n`;
    bml += `    position: (${fmtNum(furn.x)}, ${fmtNum(furn.y)})\n`;
    bml += `    rotation: ${fmtNum(furn.rotation)}\n`;
    bml += `    width: ${fmtNum(furn.width)} m\n`;
    bml += `    depth: ${fmtNum(furn.depth)} m\n\n`;
  }

  return bml;
}

/**
 * Find the sections starting at each match of `heading` (a `gm` regex for one
 * heading kind); a section ends at the next site, building or storey heading.
 */
function findHeadingRanges(source: string, heading: RegExp): { start: number; end: number }[] {
  return [...source.matchAll(heading)].map(({ index: start }) => {
    const lineEnd = source.indexOf('\n', start);
    const after = lineEnd >= 0 ? lineEnd + 1 : start + 1;
    const nextHeading = source.slice(after).search(/^#{1,3} /m);
    return { start, end: nextHeading >= 0 ? after + nextHeading : source.length };
  });
}

const PLAN_HEADINGS: Record<PlanRef['level'], RegExp> = {
  storey: /^### Storey:/gm,
  building: /^## Building:/gm,
  site: /^# Site:/gm,
};

/** Find storey section boundaries in BML source. */
export function findStoreyRanges(source: string): { start: number; end: number }[] {
  return findHeadingRanges(source, PLAN_HEADINGS.storey);
}

/** An element line (`  Wall: W-01`): indent, type and id (empty for a bare `Type:`). */
const ELEMENT_LINE = /^([ \t]+)([A-Z]\w*):[ \t]*([^\s/]*)/;

/** Element types the 2D editor imports at plan level; a bare `Type:` line of one is regenerated. */
const PLAN_TYPES = new Set(['Wall', 'Slab', 'Column', 'Furniture']);

/**
 * Element blocks (an element line and its deeper-indented lines) of a source
 * section whose id the 2D editor did not import: beams, proxies, walls without
 * an axis, ... Grouped by the `####` space they sit under ('' for the plan
 * itself); each space keeps its heading and property lines in front.
 */
function unimportedBlocks(section: string, imported: ReadonlySet<string>): KeptBlocks {
  const blocks = new Map<string, string>();
  const add = (line: string) => blocks.set(space, (blocks.get(space) ?? '') + line + '\n');
  let space = '';
  let spaceHeader = '';
  let inHeader = false; // between a space heading and its first element
  let keep = false;
  let indent = -1; // indent of the current element line, -1 outside an element
  for (const line of section.split('\n')) {
    const depth = line.length - line.trimStart().length;
    if (indent >= 0 && (line.trim() === '' || depth > indent)) {
      if (keep) add(line);
      continue;
    }
    indent = -1;
    if (line.startsWith('#')) {
      space = line.startsWith('####') ? line.trimEnd() : '';
      spaceHeader = space ? line + '\n' : '';
      inHeader = !!space;
      continue;
    }
    const m = line.match(ELEMENT_LINE);
    if (!m) {
      if (inHeader && line.trim()) spaceHeader += line + '\n';
      continue;
    }
    indent = m[1].length;
    keep = m[3] ? !imported.has(m[3]) : !PLAN_TYPES.has(m[2]);
    if (keep && !blocks.has(space)) blocks.set(space, spaceHeader);
    inHeader = false;
    if (keep) add(line);
  }
  return blocks;
}

/**
 * Add the blocks of `unimportedBlocks` to regenerated plan BML: plan-level ones
 * before the first space, the others into the space of the same heading, or
 * under their original heading when no such space was regenerated.
 */
function withBlocks(generated: string, blocks: KeptBlocks): string {
  if (blocks.size === 0) return generated;
  let out = generated.trim() ? generated.replace(/\n*$/, '\n') : '';
  const append = (body: string) => (out ? out + '\n' + body : body);
  for (const [heading, text] of blocks) {
    const body = text.replace(/\n+$/, '\n') + '\n';
    if (!heading) {
      const firstSpace = out.search(/^#### /m);
      out = firstSpace < 0 ? append(body) : out.slice(0, firstSpace) + body + out.slice(firstSpace);
      continue;
    }
    const lines = out.split('\n');
    const at = lines.findIndex(l => l.trimEnd() === heading);
    if (at < 0) {
      out = append(body);
      continue;
    }
    // `text` starts with the heading and its properties; the regenerated space has its own
    const elements = body.slice(body.search(new RegExp(ELEMENT_LINE.source, 'm')));
    const next = lines.findIndex((l, i) => i > at && l.startsWith('#'));
    const end = next < 0 ? lines.length : next;
    out = [...lines.slice(0, end), ...elements.split('\n').slice(0, -1), ...lines.slice(end)].join('\n');
  }
  return out;
}

/** Element blocks per space heading ('' for the plan itself), see `unimportedBlocks`. */
export type KeptBlocks = ReadonlyMap<string, string>;

/**
 * The element blocks of a plan's section in `source` whose id is not in
 * `imported` (the ids the 2D editor imported), to be kept by `generateFullBml`.
 * Taken once when the plan is imported: later the section holds regenerated
 * elements the editor added.
 */
export function keptBlocks(source: string, plan: PlanRef, imported: ReadonlySet<string>): KeptBlocks {
  const range = findHeadingRanges(source, PLAN_HEADINGS[plan.level])[plan.index];
  return range ? unimportedBlocks(source.slice(range.start, range.end), imported) : new Map();
}

/** Replace one storey in the original source with new content. */
export function mergeStoreyIntoSource(source: string, storeyIndex: number, newStoreyBml: string): string {
  const ranges = findStoreyRanges(source);
  if (storeyIndex < 0 || storeyIndex >= ranges.length) {
    return source + '\n' + newStoreyBml;
  }
  const r = ranges[storeyIndex];
  const trimmed = newStoreyBml.replace(/\n+$/, '\n');
  return source.slice(0, r.start) + trimmed + '\n' + source.slice(r.end);
}

/**
 * Replace the own contents of the `index`-th building or site in the source:
 * everything after its heading line and properties up to the next site,
 * building or storey heading. Appends nothing if there is no such heading.
 */
export function mergeContainerIntoSource(source: string, plan: PlanRef, contentsBml: string): string {
  const range = findHeadingRanges(source, PLAN_HEADINGS[plan.level])[plan.index];
  if (!range) return source;
  const lineEnd = source.indexOf('\n', range.start);
  const after = lineEnd >= 0 ? lineEnd + 1 : source.length;
  // Keep the heading's properties: every line before the first element (`  Wall: ...`) or space heading
  const lines = source.slice(after, range.end).split('\n');
  const propCount = lines.findIndex(line => line.startsWith('#') || ELEMENT_LINE.test(line));
  const props = (propCount < 0 ? lines : lines.slice(0, propCount)).join('\n');
  const header = props ? props.replace(/\n*$/, '\n') : '';
  const body = contentsBml.replace(/\n+$/, '\n');
  return source.slice(0, after) + header + (body ? '\n' + body : '') + '\n' + source.slice(range.end);
}

/**
 * Generate full BML output (wrapping storey in site/building if no original
 * source), adding the `kept` element blocks to the plan.
 */
export function generateFullBml(
  elements: FloorplanElement[],
  detectedSpaces: DetectedSpace[],
  storeyName: string,
  elevation: number,
  height: number,
  originalFileSource: string,
  plan: PlanRef,
  kept: KeptBlocks = new Map(),
): string {
  if (originalFileSource && plan.level !== 'storey') {
    return mergeContainerIntoSource(originalFileSource, plan, withBlocks(generateContentsBml(elements, detectedSpaces, false), kept));
  }
  const storeyBml = withBlocks(generateStoreyBml(elements, detectedSpaces, storeyName, elevation, height), kept);

  if (originalFileSource) {
    return mergeStoreyIntoSource(originalFileSource, plan.index, storeyBml);
  }
  return `# Site: Project Site\n  location: 0.0, 0.0\n\n## Building: Building\n  type: residential\n\n` + storeyBml;
}
