/** Import SemanticModel IR into floorplan element array. */

import type { SemanticModel, ResolvedProperty, ResolvedValue, ResolvedSpace, ResolvedElement, Span } from '../types.ts';
import type { FloorplanElement, WallElement, IdCounters } from './element-model.ts';

/** Build a map from asset ID to its {width, depth} dimensions. */
function buildAssetDimensions(model: SemanticModel): Map<string, { width: number; depth: number }> {
  const map = new Map<string, { width: number; depth: number }>();
  for (const lib of model.asset_libraries || []) {
    for (const asset of lib.assets || []) {
      const width = getNumericValue(asset.properties, 'width', 1.0);
      const depth = getNumericValue(asset.properties, 'depth', 1.0);
      map.set(asset.id, { width, depth });
    }
  }
  return map;
}

function getProperty(properties: ResolvedProperty[], key: string): ResolvedValue | undefined {
  const prop = properties.find(p => p.key === key);
  return prop?.value;
}

function getNumericValue(properties: ResolvedProperty[], key: string, fallback: number): number {
  const val = getProperty(properties, key);
  if (val && typeof val === 'object' && 'Number' in val) return val.Number;
  return fallback;
}

/**
 * Which heading a plan edits: the `index`-th `### Storey`, `## Building` or
 * `# Site` heading of the model, counting every heading of that level.
 */
export interface PlanRef {
  level: 'storey' | 'building' | 'site';
  index: number;
}

/** A 2D plan: a storey, or the contents of a building or site outside its storeys or buildings. */
export interface Plan extends PlanRef {
  name: string;
  properties: ResolvedProperty[];
  spaces: ResolvedSpace[];
  elements: ResolvedElement[];
  span: Span;
}

export interface ImportedStorey {
  name: string;
  plan: PlanRef;
  /** Ids of the plan's elements in `elements`, including door/window children. */
  importedIds: Set<string>;
  elevation: number;
  height: number;
  elements: FloorplanElement[];
  idCounters: IdCounters;
}

/**
 * Get every plan of a semantic model: each building's storeys, then the building
 * itself if it holds spaces or elements directly, and per site the site itself
 * likewise (the order of `SemanticModel::containers` in bml-core).
 */
export function getStoreys(model: SemanticModel): Plan[] {
  const plans: Plan[] = [];
  let storeyIndex = 0;
  let buildingIndex = 0;
  let siteIndex = 0;
  for (const site of model.sites || []) {
    // A building before any `# Site` heading gets an implicit site without a heading to count
    if (!site.implicit) siteIndex++;
    for (const building of site.buildings || []) {
      for (const storey of building.storeys || []) {
        const { name, properties, spaces, elements, span } = storey;
        plans.push({ level: 'storey', index: storeyIndex++, name, properties, spaces, elements, span });
      }
      const index = buildingIndex++;
      if (building.spaces?.length || building.elements?.length) {
        const { name, properties, spaces = [], elements = [], span } = building;
        plans.push({ level: 'building', index, name, properties, spaces, elements, span });
      }
    }
    if (site.spaces?.length || site.elements?.length) {
      const { name, properties, spaces = [], elements = [], span } = site;
      plans.push({ level: 'site', index: siteIndex - 1, name, properties, spaces, elements, span });
    }
  }
  return plans;
}

/** A plan's elements, then those of its spaces (`contents()` in bml-core). */
export function planElements(plan: Plan): ResolvedElement[] {
  return [...plan.elements, ...plan.spaces.flatMap(s => s.elements)];
}

/** Import a single plan (see `getStoreys`) from a semantic model into floorplan elements. */
export function importStorey(model: SemanticModel, storeyIndex: number): ImportedStorey | null {
  const storeys = getStoreys(model);
  if (storeys.length === 0) return null;

  const storey = storeys[storeyIndex ?? 0];
  if (!storey) return null;

  const elements: FloorplanElement[] = [];
  const idCounters: IdCounters = { Wall: 1, Door: 1, Window: 1, Column: 1, Slab: 1, Beam: 1, Furniture: 1 };

  const elevation = getNumericValue(storey.properties, 'elevation', 0);
  // Building- and site-level contents are 3 m high, as in bml-core's `Container::height()`
  const height = storey.level === 'storey' ? getNumericValue(storey.properties, 'height', 2.8) : 3;

  const allElements = planElements(storey);

  // First pass: walls and their children
  for (const elem of allElements) {
    if (elem.element_type === 'Wall') {
      // A curtain wall grid stays a kept block: the editor cannot write it back.
      if (getProperty(elem.properties, 'mullions')) continue;
      const axisVal = getProperty(elem.properties, 'from');
      if (!axisVal || !('Axis' in axisVal)) continue;
      const from = axisVal.Axis.from;
      const to = axisVal.Axis.to;
      const wall: WallElement = {
        type: 'Wall',
        id: elem.id,
        from: { x: from[0], y: from[1] },
        to: { x: to[0], y: to[1] },
        height: getNumericValue(elem.properties, 'height', height),
        thickness: getNumericValue(elem.properties, 'thickness', 0.24),
        anchored: false,
        children: [],
      };

      const idNum = parseInt(elem.id.replace(/\D+/g, ''), 10);
      if (!isNaN(idNum) && idNum >= idCounters.Wall) {
        idCounters.Wall = idNum + 1;
      }
      elements.push(wall);

      // Children (windows, doors)
      for (const child of elem.children || []) {
        if (child.element_type === 'Window') {
          elements.push({
            type: 'Window',
            id: child.id,
            parentWall: elem.id,
            offset: getNumericValue(child.properties, 'offset', 0),
            width: getNumericValue(child.properties, 'width', 1.2),
            height: getNumericValue(child.properties, 'height', 1.4),
            sill_height: getNumericValue(child.properties, 'sill_height', 0.9),
          });
          const winNum = parseInt(child.id.replace(/\D+/g, ''), 10);
          if (!isNaN(winNum) && winNum >= idCounters.Window) {
            idCounters.Window = winNum + 1;
          }
        } else if (child.element_type === 'Door') {
          elements.push({
            type: 'Door',
            id: child.id,
            parentWall: elem.id,
            offset: getNumericValue(child.properties, 'offset', 0),
            width: getNumericValue(child.properties, 'width', 0.9),
            height: getNumericValue(child.properties, 'height', 2.1),
          });
          const doorNum = parseInt(child.id.replace(/\D+/g, ''), 10);
          if (!isNaN(doorNum) && doorNum >= idCounters.Door) {
            idCounters.Door = doorNum + 1;
          }
        }
      }
    }
  }

  // Second pass: columns
  for (const elem of allElements) {
    if (elem.element_type === 'Column') {
      const posVal = getProperty(elem.properties, 'position');
      let x = 0, y = 0;
      if (posVal && 'Point' in posVal) {
        x = posVal.Point[0];
        y = posVal.Point[1];
      }
      const profVal = getProperty(elem.properties, 'profile');
      let profileW = 0.3, profileD = 0.3;
      if (profVal && 'Profile' in profVal && profVal.Profile.params.length >= 2) {
        profileW = profVal.Profile.params[0];
        profileD = profVal.Profile.params[1];
      }
      elements.push({
        type: 'Column',
        id: elem.id,
        x, y,
        height: getNumericValue(elem.properties, 'height', height),
        profileW, profileD,
      });
      const colNum = parseInt(elem.id.replace(/\D+/g, ''), 10);
      if (!isNaN(colNum) && colNum >= idCounters.Column) {
        idCounters.Column = colNum + 1;
      }
    }
  }

  // Third pass: slabs
  for (const elem of allElements) {
    if (elem.element_type === 'Slab') {
      // A curved outline stays a kept block: the editor would write it back as its polyline.
      if (elem.curved?.length) continue;
      const bndVal = getProperty(elem.properties, 'boundary');
      if (!bndVal || !('Polygon' in bndVal)) continue;
      const points = bndVal.Polygon.map(p => ({ x: p[0], y: p[1] }));
      elements.push({
        type: 'Slab',
        id: elem.id,
        boundary: points,
        thickness: getNumericValue(elem.properties, 'thickness', 0.22),
      });
      const slabNum = parseInt(elem.id.replace(/\D+/g, ''), 10);
      if (!isNaN(slabNum) && slabNum >= idCounters.Slab) {
        idCounters.Slab = slabNum + 1;
      }
    }
  }

  const assetDims = buildAssetDimensions(model);

  // Fourth pass: furniture
  for (const elem of allElements) {
    if (elem.element_type === 'Furniture') {
      const posVal = getProperty(elem.properties, 'position');
      let x = 0, y = 0;
      if (posVal && 'Point' in posVal) {
        x = posVal.Point[0];
        y = posVal.Point[1];
      }
      const assetVal = getProperty(elem.properties, 'asset');
      const asset = (assetVal && ('String' in assetVal ? assetVal.String : 'Identifier' in assetVal ? assetVal.Identifier : null)) ?? 'Unknown';

      const dims = assetDims.get(asset) ?? { width: 1.0, depth: 1.0 };
      elements.push({
        type: 'Furniture',
        id: elem.id,
        asset,
        x, y,
        rotation: getNumericValue(elem.properties, 'rotation', 0),
        width: getNumericValue(elem.properties, 'width', dims.width),
        depth: getNumericValue(elem.properties, 'depth', dims.depth),
      });

      const furnNum = parseInt(elem.id.replace(/\D+/g, ''), 10);
      if (!isNaN(furnNum) && furnNum >= (idCounters.Furniture || 1)) {
        idCounters.Furniture = furnNum + 1;
      }
    }
  }

  return {
    name: storey.name,
    plan: { level: storey.level, index: storey.index },
    importedIds: new Set(elements.map(e => e.id)),
    elevation,
    height,
    elements,
    idCounters,
  };
}
