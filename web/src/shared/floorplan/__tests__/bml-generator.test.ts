import { describe, it, expect } from 'vitest';
import {
  generateFullBml,
  findStoreyRanges,
  keptBlocks,
  mergeStoreyIntoSource,
} from '../bml-generator.ts';
import type { PlanRef } from '../model-importer.ts';
import type { FloorplanElement, WallElement, DetectedSpace } from '../element-model.ts';

// Helpers to build minimal test elements
function makeWall(id: string, from: [number, number], to: [number, number]): WallElement {
  return {
    type: 'Wall', id,
    from: { x: from[0], y: from[1] }, to: { x: to[0], y: to[1] },
    height: 2.8, thickness: 0.24, anchored: false, children: [],
  };
}

const storey = (index: number): PlanRef => ({ level: 'storey', index });

// A two-storey BML source for testing
const TWO_STOREY_SOURCE = `# Site: Test Site
  location: 0.0, 0.0

## Building: House
  type: residential

### Storey: Ground Floor
  elevation: 0 m
  height: 2.8 m

#### Space: Living Room
  area: 30 m2

  Wall: W-GF-01
    from: (0, 0) to: (6, 0)
    height: 2.8 m
    thickness: 0.24 m

  Wall: W-GF-02
    from: (6, 0) to: (6, 5)
    height: 2.8 m
    thickness: 0.24 m

### Storey: First Floor
  elevation: 3 m
  height: 2.8 m

#### Space: Bedroom
  area: 20 m2

  Wall: W-FF-01
    from: (0, 0) to: (5, 0)
    height: 2.8 m
    thickness: 0.24 m

  Wall: W-FF-02
    from: (5, 0) to: (5, 4)
    height: 2.8 m
    thickness: 0.24 m

`;

describe('findStoreyRanges', () => {
  it('finds two storey ranges in a two-storey source', () => {
    const ranges = findStoreyRanges(TWO_STOREY_SOURCE);
    expect(ranges).toHaveLength(2);
    // Both should have valid start positions
    expect(TWO_STOREY_SOURCE.slice(ranges[0].start, ranges[0].start + 20)).toContain('### Storey: Ground');
    expect(TWO_STOREY_SOURCE.slice(ranges[1].start, ranges[1].start + 20)).toContain('### Storey: First');
  });

  it('first storey ends before second storey starts', () => {
    const ranges = findStoreyRanges(TWO_STOREY_SOURCE);
    expect(ranges[0].end).toBeLessThanOrEqual(ranges[1].start);
  });

  it('returns empty for source without storeys', () => {
    expect(findStoreyRanges('# Site: Test\n')).toHaveLength(0);
  });
});

describe('mergeStoreyIntoSource', () => {
  it('replaces storey 0 while preserving storey 1', () => {
    const newStorey = `### Storey: Ground Floor EDITED\n  elevation: 0 m\n  height: 3 m\n\n`;
    const result = mergeStoreyIntoSource(TWO_STOREY_SOURCE, 0, newStorey);
    expect(result).toContain('Ground Floor EDITED');
    expect(result).toContain('### Storey: First Floor');
    expect(result).toContain('W-FF-01');
    expect(result).toContain('W-FF-02');
  });

  it('replaces storey 1 while preserving storey 0', () => {
    const newStorey = `### Storey: First Floor EDITED\n  elevation: 3 m\n  height: 3 m\n\n`;
    const result = mergeStoreyIntoSource(TWO_STOREY_SOURCE, 1, newStorey);
    expect(result).toContain('First Floor EDITED');
    expect(result).toContain('### Storey: Ground Floor');
    expect(result).toContain('W-GF-01');
    expect(result).toContain('W-GF-02');
  });

  it('preserves site and building headers', () => {
    const newStorey = `### Storey: Ground Floor\n  elevation: 0 m\n  height: 2.8 m\n\n`;
    const result = mergeStoreyIntoSource(TWO_STOREY_SOURCE, 0, newStorey);
    expect(result).toContain('# Site: Test Site');
    expect(result).toContain('## Building: House');
  });

  it('appends when storeyIndex is out of range', () => {
    const newStorey = `### Storey: Second Floor\n  elevation: 6 m\n  height: 2.8 m\n\n`;
    const result = mergeStoreyIntoSource(TWO_STOREY_SOURCE, 5, newStorey);
    // All original content preserved
    expect(result).toContain('### Storey: Ground Floor');
    expect(result).toContain('### Storey: First Floor');
    expect(result).toContain('### Storey: Second Floor');
  });
});

describe('generateFullBml', () => {
  const gfElements: FloorplanElement[] = [
    makeWall('W-01', [0, 0], [6, 0]),
    makeWall('W-02', [6, 0], [6, 5]),
    makeWall('W-03', [6, 5], [0, 5]),
    makeWall('W-04', [0, 5], [0, 0]),
  ];
  const gfSpaces: DetectedSpace[] = [{
    id: 'space-0', name: 'Room A', area: 30,
    polygon: [{ x: 0, y: 0 }, { x: 6, y: 0 }, { x: 6, y: 5 }, { x: 0, y: 5 }],
    wallIds: ['W-01', 'W-02', 'W-03', 'W-04'],
  }];

  it('generates a standalone file when originalFileSource is empty', () => {
    const result = generateFullBml(gfElements, gfSpaces, 'Ground Floor', 0, 2.8, '', storey(0));
    expect(result).toContain('# Site: Project Site');
    expect(result).toContain('### Storey: Ground Floor');
    expect(result).toContain('W-01');
  });

  it('merges into originalFileSource when provided', () => {
    const result = generateFullBml(gfElements, gfSpaces, 'Ground Floor', 0, 2.8, TWO_STOREY_SOURCE, storey(0));
    expect(result).toContain('# Site: Test Site');
    expect(result).toContain('## Building: House');
    // Storey 0 replaced with new content
    expect(result).toContain('Room A');
    expect(result).toContain('W-01');
    // Storey 1 preserved
    expect(result).toContain('### Storey: First Floor');
    expect(result).toContain('W-FF-01');
    expect(result).toContain('W-FF-02');
  });

  it('does NOT delete other storeys when editing storey 0', () => {
    // This is the core regression test for the "save deletes other floors" bug
    const result = generateFullBml(gfElements, gfSpaces, 'Ground Floor', 0, 2.8, TWO_STOREY_SOURCE, storey(0));
    const ranges = findStoreyRanges(result);
    expect(ranges).toHaveLength(2);
    expect(result).toContain('### Storey: First Floor');
    expect(result).toContain('Bedroom');
  });
});

describe('multi-storey edit round-trip (regression)', () => {
  // Simulates: edit storey 0 → update originalFileSource → edit storey 1 → verify both preserved
  it('preserves storey 0 edits when subsequently editing storey 1', () => {
    const gfElements: FloorplanElement[] = [
      makeWall('W-EDITED-01', [0, 0], [10, 0]),
      makeWall('W-EDITED-02', [10, 0], [10, 8]),
      makeWall('W-EDITED-03', [10, 8], [0, 8]),
      makeWall('W-EDITED-04', [0, 8], [0, 0]),
    ];
    const gfSpaces: DetectedSpace[] = [{
      id: 'space-0', name: 'Big Room', area: 80,
      polygon: [{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 8 }, { x: 0, y: 8 }],
      wallIds: ['W-EDITED-01', 'W-EDITED-02', 'W-EDITED-03', 'W-EDITED-04'],
    }];

    // Step 1: Edit storey 0 — merge into original source
    const afterEditStorey0 = generateFullBml(gfElements, gfSpaces, 'Ground Floor', 0, 2.8, TWO_STOREY_SOURCE, storey(0));
    expect(afterEditStorey0).toContain('W-EDITED-01');
    expect(afterEditStorey0).toContain('Big Room');
    expect(afterEditStorey0).toContain('### Storey: First Floor');

    // Step 2: Now simulate switching to storey 1 and editing
    // KEY: use afterEditStorey0 as the new originalFileSource (the fix)
    const ffElements: FloorplanElement[] = [
      makeWall('W-FF-NEW-01', [0, 0], [4, 0]),
      makeWall('W-FF-NEW-02', [4, 0], [4, 3]),
      makeWall('W-FF-NEW-03', [4, 3], [0, 3]),
      makeWall('W-FF-NEW-04', [0, 3], [0, 0]),
    ];
    const ffSpaces: DetectedSpace[] = [{
      id: 'space-0', name: 'Small Bedroom', area: 12,
      polygon: [{ x: 0, y: 0 }, { x: 4, y: 0 }, { x: 4, y: 3 }, { x: 0, y: 3 }],
      wallIds: ['W-FF-NEW-01', 'W-FF-NEW-02', 'W-FF-NEW-03', 'W-FF-NEW-04'],
    }];

    const afterEditStorey1 = generateFullBml(ffElements, ffSpaces, 'First Floor', 3, 2.8, afterEditStorey0, storey(1));

    // Both storeys should have their latest edits
    expect(afterEditStorey1).toContain('W-EDITED-01'); // storey 0 edit preserved
    expect(afterEditStorey1).toContain('Big Room');     // storey 0 space name preserved
    expect(afterEditStorey1).toContain('W-FF-NEW-01');  // storey 1 edited
    expect(afterEditStorey1).toContain('Small Bedroom'); // storey 1 space name
    expect(afterEditStorey1).toContain('# Site: Test Site'); // headers preserved

    // Verify two storeys exist
    const ranges = findStoreyRanges(afterEditStorey1);
    expect(ranges).toHaveLength(2);
  });

  it('BUG REGRESSION: using stale originalFileSource loses storey 0 edits', () => {
    // This test demonstrates the bug that existed before the fix:
    // Using the ORIGINAL source (not updated) when editing storey 1 would revert storey 0 edits.
    const gfElements: FloorplanElement[] = [
      makeWall('W-EDITED-01', [0, 0], [10, 0]),
      makeWall('W-EDITED-02', [10, 0], [10, 8]),
    ];
    const gfSpaces: DetectedSpace[] = [{
      id: 'space-0', name: 'Edited Room', area: 80,
      polygon: [{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 8 }, { x: 0, y: 8 }],
      wallIds: ['W-EDITED-01', 'W-EDITED-02'],
    }];

    // Edit storey 0
    const afterEditStorey0 = generateFullBml(gfElements, gfSpaces, 'Ground Floor', 0, 2.8, TWO_STOREY_SOURCE, storey(0));
    expect(afterEditStorey0).toContain('W-EDITED-01');

    // BUG PATH: using TWO_STOREY_SOURCE (stale) instead of afterEditStorey0 (updated)
    const ffElements: FloorplanElement[] = [
      makeWall('W-FF-X', [0, 0], [3, 0]),
    ];
    const ffSpaces: DetectedSpace[] = [];

    const bugResult = generateFullBml(ffElements, ffSpaces, 'First Floor', 3, 2.8, TWO_STOREY_SOURCE, storey(1));
    // The bug: storey 0 reverts to original, losing the W-EDITED-01 edits
    expect(bugResult).not.toContain('W-EDITED-01'); // confirms the bug path loses edits

    // FIX PATH: using afterEditStorey0 as the base
    const fixResult = generateFullBml(ffElements, ffSpaces, 'First Floor', 3, 2.8, afterEditStorey0, storey(1));
    // The fix: storey 0 edits are preserved
    expect(fixResult).toContain('W-EDITED-01'); // confirms the fix preserves edits
  });
});

describe('building- and site-level plans', () => {
  const SOURCE = `# Site: Campus
  location: 0.0, 0.0
  Wall: W-Fence
    from: (0, 0) to: (20, 0)
    height: 1.5 m
    thickness: 0.1 m

## Building: House
  type: residential
  // the atrium spans all storeys
#### Space: Atrium
  area: 9 m2

### Storey: Ground Floor
  elevation: 0 m
  height: 2.8 m
`;

  it('replaces only the building contents and keeps its properties', () => {
    const result = generateFullBml([makeWall('W-New', [0, 0], [4, 0])], [], 'House', 0, 2.8, SOURCE, { level: 'building', index: 0 });
    expect(result).toContain('## Building: House\n  type: residential\n  // the atrium spans all storeys\n');
    expect(result).toContain('Wall: W-New');
    expect(result).not.toContain('Atrium');
    expect(result).not.toContain('Main Space');
    expect(result).toContain('W-Fence');
    expect(result).toContain('### Storey: Ground Floor\n  elevation: 0 m');
    expect(result.indexOf('W-New')).toBeLessThan(result.indexOf('### Storey'));
  });

  it('replaces only the site contents', () => {
    const result = generateFullBml([makeWall('W-Gate', [0, 0], [2, 0])], [], 'Campus', 0, 2.8, SOURCE, { level: 'site', index: 0 });
    expect(result).toContain('# Site: Campus\n  location: 0.0, 0.0\n\n  Wall: W-Gate');
    expect(result).not.toContain('W-Fence');
    expect(result.indexOf('W-Gate')).toBeLessThan(result.indexOf('## Building: House'));
    expect(result).toContain('#### Space: Atrium');
  });

  it('leaves the source unchanged for a missing building', () => {
    expect(generateFullBml([], [], 'X', 0, 2.8, SOURCE, { level: 'building', index: 3 })).toBe(SOURCE);
  });
});

describe('element blocks the 2D editor does not import', () => {
  const SOURCE = `# Site: Campus

## Building: House

### Storey: Ground Floor
  elevation: 0 m
  height: 2.8 m

  Wall: W-01
    from: (0, 0) to: (4, 0)
  Beam: B-01
    from: (0, 0, 2.8) to: (4, 0, 2.8)

    profile: rect(0.2, 0.4)
  Proxy:
    boundary: polygon((0, 0), (1, 0), (1, 1))

#### Space: Kitchen
  usage: kitchen
  Column: C-01
    position: (1, 1)
  Pipe: P-01
    from: (0, 0, 0) to: (1, 0, 0)
`;
  const imported = new Set(['W-01', 'C-01']);

  it('keeps them when a storey is regenerated', () => {
    const result = generateFullBml([makeWall('W-01', [0, 0], [5, 0])], [], 'Ground Floor', 0, 2.8, SOURCE, storey(0), keptBlocks(SOURCE, storey(0), imported));
    expect(result).toContain('to: (5, 0)');
    expect(result).not.toContain('to: (4, 0)\n');
    expect(result).not.toContain('C-01');
    expect(result).toContain('  Beam: B-01\n    from: (0, 0, 2.8) to: (4, 0, 2.8)\n\n    profile: rect(0.2, 0.4)\n');
    expect(result).toContain('  Proxy:\n    boundary:');
    // The imported column before it does not cut the space heading off
    expect(result).toContain('#### Space: Kitchen\n  usage: kitchen\n  Pipe: P-01\n');
  });

  it('drops them without kept blocks', () => {
    const result = generateFullBml([], [], 'Ground Floor', 0, 2.8, SOURCE, storey(0));
    expect(result).not.toContain('B-01');
  });

  it('does not duplicate added elements over repeated edits', () => {
    const kept = keptBlocks(SOURCE, storey(0), imported);
    const once = generateFullBml([makeWall('W-05', [0, 0], [1, 0])], [], 'Ground Floor', 0, 2.8, SOURCE, storey(0), kept);
    const twice = generateFullBml([makeWall('W-05', [0, 0], [2, 0])], [], 'Ground Floor', 0, 2.8, once, storey(0), kept);
    expect(twice.match(/W-05/g)).toHaveLength(1);
    expect(twice.match(/B-01/g)).toHaveLength(1);
  });

  it('keeps them when a building plan is regenerated', () => {
    const source = SOURCE.replace('## Building: House\n', '## Building: House\n  Beam: B-Bridge\n    from: (0, 0, 5) to: (9, 0, 5)\n  Column: C-Old\n    position: (0, 0)\n');
    const building: PlanRef = { level: 'building', index: 0 };
    const result = generateFullBml([], [], 'House', 0, 2.8, source, building, keptBlocks(source, building, new Set(['C-Old'])));
    expect(result).toContain('## Building: House\n\n  Beam: B-Bridge\n    from: (0, 0, 5) to: (9, 0, 5)\n');
    expect(result).not.toContain('C-Old');
    expect(result).toContain('### Storey: Ground Floor');
  });
});
