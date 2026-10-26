import { describe, it, expect } from 'vitest';
import { getStoreys, importStorey } from '../model-importer.ts';
import type { ResolvedElement, SemanticModel, Span } from '../../types.ts';

const pos = { line: 0, column: 0, byte_offset: 0 };
const span: Span = { start: pos, end: pos };

function column(id: string): ResolvedElement {
  return {
    element_type: 'Column', id, children: [], span,
    properties: [{ key: 'position', value: { Point: [1, 2] }, span }],
  };
}

const model: SemanticModel = {
  materials: [],
  asset_libraries: [],
  sites: [
    {
      name: 'Campus', properties: [], span,
      elements: [column('C-Site')],
      buildings: [
        {
          name: 'House', properties: [], span,
          spaces: [{ name: 'Atrium', properties: [], elements: [column('C-Atrium')], span }],
          storeys: [
            { name: 'GF', properties: [], spaces: [], elements: [column('C-GF')], span },
            { name: 'F1', properties: [], spaces: [], elements: [], span },
          ],
        },
        { name: 'Shed', properties: [], storeys: [], span },
        { name: 'Garage', properties: [], storeys: [], elements: [column('C-Garage')], span },
      ],
    },
  ],
};

describe('getStoreys', () => {
  it('lists storeys, then buildings and sites with their own contents', () => {
    const plans = getStoreys(model).map(p => [p.level, p.index, p.name]);
    // The empty Shed gets no plan but still counts for the building index
    expect(plans).toEqual([
      ['storey', 0, 'GF'],
      ['storey', 1, 'F1'],
      ['building', 0, 'House'],
      ['building', 2, 'Garage'],
      ['site', 0, 'Campus'],
    ]);
  });

  it('imports building- and site-level elements, including those in spaces', () => {
    const building = importStorey(model, 2)!;
    expect(building.plan).toEqual({ level: 'building', index: 0 });
    expect(building.elements.map(e => e.id)).toEqual(['C-Atrium']);
    expect(importStorey(model, 4)!.elements.map(e => e.id)).toEqual(['C-Site']);
  });
});

describe('getStoreys with an implicit site', () => {
  it('does not count a site without a `# Site` heading', () => {
    const implicit: SemanticModel = {
      materials: [],
      asset_libraries: [],
      sites: [
        // `## Building: A` before any `# Site`
        { name: 'ImplicitSite', implicit: true, properties: [], span, buildings: [{ name: 'A', properties: [], storeys: [], span }] },
        { name: 'Campus', properties: [], span, elements: [column('C-Site')], buildings: [] },
      ],
    };
    expect(getStoreys(implicit).map(p => [p.level, p.index])).toEqual([['site', 0]]);
  });
});

describe('importStorey with a curtain wall', () => {
  it('leaves a wall with a mullion grid out, so its block is kept as written', () => {
    const wall = (id: string, extra: ResolvedElement['properties']): ResolvedElement => ({
      element_type: 'Wall', id, children: [], span,
      properties: [{ key: 'from', value: { Axis: { from: [0, 0], to: [6, 0] } }, span }, ...extra],
    });
    const m: SemanticModel = {
      materials: [], asset_libraries: [],
      sites: [{
        name: 'S', properties: [], span, buildings: [{
          name: 'B', properties: [], span, storeys: [{
            name: 'GF', properties: [], spaces: [], span,
            elements: [wall('W-01', []), wall('CW-01', [{ key: 'mullions', value: { Number: 1.5 }, span }])],
          }],
        }],
      }],
    };
    expect(importStorey(m, 0)!.elements.map(e => e.id)).toEqual(['W-01']);
  });
});

describe('importStorey with a curved slab', () => {
  it('leaves a slab with a smooth outline out, so its curve is kept as written', () => {
    const slab = (id: string, curved?: string[]): ResolvedElement => ({
      element_type: 'Slab', id, children: [], span, curved,
      properties: [{ key: 'boundary', value: { Polygon: [[0, 0], [6, 0], [6, 4]] }, span }],
    });
    const m: SemanticModel = {
      materials: [], asset_libraries: [],
      sites: [{
        name: 'S', properties: [], span, buildings: [{
          name: 'B', properties: [], span, storeys: [{
            name: 'GF', properties: [], spaces: [], span,
            elements: [slab('SL-01'), slab('SL-02', ['boundary'])],
          }],
        }],
      }],
    };
    expect(importStorey(m, 0)!.elements.map(e => e.id)).toEqual(['SL-01']);
  });
});
