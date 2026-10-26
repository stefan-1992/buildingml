import { describe, it, expect } from 'vitest';
import { buildChatContext, extractBmlFromResponse, extractSearchReplaceBlocks, applySearchReplaceBlocks } from '../context-builder.ts';
import type { SemanticModel, Diagnostic } from '../../types.ts';

describe('buildChatContext', () => {
  const baseBml = `# Site: My Site
  location: 49.45, 12.18

## Building: House
  type: residential

### Storey: Ground Floor
  elevation: 0.0 m
  height: 2.80 m

#### Space: Kitchen
  area: 12 m2

  Wall: W-01
    from: (0, 0) to: (4, 0)
    height: 2.80 m
    thickness: 0.24 m
`;

  const minModel: SemanticModel = {
    sites: [{
      name: 'My Site',
      properties: [],
      buildings: [{
        name: 'House',
        properties: [],
        storeys: [{
          name: 'Ground Floor',
          properties: [],
          spaces: [{
            name: 'Kitchen',
            properties: [],
            elements: [{
              element_type: 'Wall',
              id: 'W-01',
              properties: [
                { key: 'from', value: { Axis: { from: [0, 0], to: [4, 0] } }, span: { start: { line: 1, column: 1, byte_offset: 0 }, end: { line: 1, column: 1, byte_offset: 0 } } },
              ],
              children: [],
              span: { start: { line: 1, column: 1, byte_offset: 0 }, end: { line: 1, column: 1, byte_offset: 0 } },
            }],
            span: { start: { line: 1, column: 1, byte_offset: 0 }, end: { line: 1, column: 1, byte_offset: 0 } },
          }],
          elements: [],
          span: { start: { line: 1, column: 1, byte_offset: 0 }, end: { line: 1, column: 1, byte_offset: 0 } },
        }],
        span: { start: { line: 1, column: 1, byte_offset: 0 }, end: { line: 1, column: 1, byte_offset: 0 } },
      }],
      span: { start: { line: 1, column: 1, byte_offset: 0 }, end: { line: 1, column: 1, byte_offset: 0 } },
    }],
    materials: [],
    asset_libraries: [],
  };

  it('includes BML source in context', () => {
    const ctx = buildChatContext({
      source: baseBml,
      model: null,
      currentStoreyIndex: 0,
      selectedElementId: null,
      diagnostics: [],
      viewMode: '2d',
    });
    expect(ctx).toContain('# Site: My Site');
    expect(ctx).toContain('Wall: W-01');
  });

  it('includes view mode', () => {
    const ctx = buildChatContext({
      source: baseBml,
      model: null,
      currentStoreyIndex: 0,
      selectedElementId: null,
      diagnostics: [],
      viewMode: '3d',
    });
    expect(ctx).toContain('3D view mode');
  });

  it('includes storey name from model', () => {
    const ctx = buildChatContext({
      source: baseBml,
      model: minModel,
      currentStoreyIndex: 0,
      selectedElementId: null,
      diagnostics: [],
      viewMode: '2d',
    });
    expect(ctx).toContain('Active storey: "Ground Floor"');
    expect(ctx).toContain('Spaces in this storey: Kitchen');
  });

  it('includes selected element details', () => {
    const ctx = buildChatContext({
      source: baseBml,
      model: minModel,
      currentStoreyIndex: 0,
      selectedElementId: 'W-01',
      diagnostics: [],
      viewMode: '2d',
    });
    expect(ctx).toContain('Currently selected: Wall "W-01"');
  });

  it('includes diagnostics', () => {
    const diags: Diagnostic[] = [
      { code: 'E001', severity: 'error', message: 'Unknown property', help: null },
    ];
    const ctx = buildChatContext({
      source: baseBml,
      model: null,
      currentStoreyIndex: 0,
      selectedElementId: null,
      diagnostics: diags,
      viewMode: '2d',
    });
    expect(ctx).toContain('Current errors (1)');
    expect(ctx).toContain('Unknown property');
  });

  it('includes BML syntax reference', () => {
    const ctx = buildChatContext({
      source: baseBml,
      model: null,
      currentStoreyIndex: 0,
      selectedElementId: null,
      diagnostics: [],
      viewMode: '2d',
    });
    expect(ctx).toContain('BML syntax reference');
    expect(ctx).toContain('# Site: <name>');
  });

  it('includes SEARCH/REPLACE instructions', () => {
    const ctx = buildChatContext({
      source: baseBml,
      model: null,
      currentStoreyIndex: 0,
      selectedElementId: null,
      diagnostics: [],
      viewMode: '2d',
    });
    expect(ctx).toContain('SEARCH/REPLACE');
    expect(ctx).toContain('<<<<<<< SEARCH');
  });
});

describe('extractBmlFromResponse', () => {
  it('extracts BML from ```bml code block', () => {
    const response = `Sure, here's the updated building:

\`\`\`bml
# Site: My Site
  location: 49.45, 12.18

## Building: House
  type: residential
\`\`\`

I added the changes you requested.`;

    const result = extractBmlFromResponse(response);
    expect(result).not.toBeNull();
    expect(result).toContain('# Site: My Site');
    expect(result).toContain('## Building: House');
  });

  it('extracts BML from generic code block starting with # Site:', () => {
    const response = `Here you go:

\`\`\`
# Site: Test
  location: 0, 0
\`\`\``;

    const result = extractBmlFromResponse(response);
    expect(result).not.toBeNull();
    expect(result).toContain('# Site: Test');
  });

  it('returns null when no code block present', () => {
    const response = 'The bathroom is 4m2 because the remaining space is 2m x 2m.';
    const result = extractBmlFromResponse(response);
    expect(result).toBeNull();
  });

  it('returns null for non-BML code blocks', () => {
    const response = `Here's some JavaScript:

\`\`\`javascript
console.log("hello");
\`\`\``;

    const result = extractBmlFromResponse(response);
    expect(result).toBeNull();
  });

  it('handles multiple code blocks and extracts the BML one', () => {
    const response = `I'll explain and then show the code:

Some explanation here.

\`\`\`bml
# Site: Updated
  location: 1, 2

## Building: New
  type: commercial
\`\`\`

That should work!`;

    const result = extractBmlFromResponse(response);
    expect(result).not.toBeNull();
    expect(result).toContain('# Site: Updated');
  });
});

describe('extractSearchReplaceBlocks', () => {
  it('extracts a single SEARCH/REPLACE block', () => {
    const response = `I'll update the wall thickness:

<<<<<<< SEARCH
  Wall: W-01
    from: (0, 0) to: (4, 0)
    thickness: 0.24 m
=======
  Wall: W-01
    from: (0, 0) to: (4, 0)
    thickness: 0.30 m
>>>>>>> REPLACE

Done!`;

    const blocks = extractSearchReplaceBlocks(response);
    expect(blocks).toHaveLength(1);
    expect(blocks[0].search).toContain('thickness: 0.24 m');
    expect(blocks[0].replace).toContain('thickness: 0.30 m');
  });

  it('extracts multiple SEARCH/REPLACE blocks', () => {
    const response = `Two changes:

<<<<<<< SEARCH
    thickness: 0.24 m
=======
    thickness: 0.30 m
>>>>>>> REPLACE

<<<<<<< SEARCH
    height: 2.80 m
=======
    height: 3.00 m
>>>>>>> REPLACE`;

    const blocks = extractSearchReplaceBlocks(response);
    expect(blocks).toHaveLength(2);
    expect(blocks[0].search).toContain('0.24');
    expect(blocks[1].search).toContain('2.80');
  });

  it('returns empty array when no blocks present', () => {
    const response = 'The wall is 4 meters long.';
    const blocks = extractSearchReplaceBlocks(response);
    expect(blocks).toHaveLength(0);
  });

  it('handles empty REPLACE section with blank line', () => {
    const response = `<<<<<<< SEARCH
  Wall: W-02
    from: (0, 0) to: (2, 0)
    thickness: 0.12 m
=======

>>>>>>> REPLACE`;

    const blocks = extractSearchReplaceBlocks(response);
    expect(blocks).toHaveLength(1);
    expect(blocks[0].search).toContain('Wall: W-02');
    expect(blocks[0].replace).toBe('');
  });

  it('handles empty REPLACE section without blank line', () => {
    const response = `<<<<<<< SEARCH
  Wall: W-02
    from: (0, 0) to: (2, 0)
=======
>>>>>>> REPLACE`;

    const blocks = extractSearchReplaceBlocks(response);
    expect(blocks).toHaveLength(1);
    expect(blocks[0].search).toContain('Wall: W-02');
    expect(blocks[0].replace).toBe('');
  });

  it('ignores stray markers after a valid block', () => {
    const response = `<<<<<<< SEARCH
  Wall: W-01
    thickness: 0.24 m
=======
>>>>>>> REPLACE
=======
The wall has been removed.
>>>>>>> REPLACE`;

    const blocks = extractSearchReplaceBlocks(response);
    expect(blocks).toHaveLength(1);
    expect(blocks[0].search).toContain('Wall: W-01');
    expect(blocks[0].replace).toBe('');
  });
});

describe('applySearchReplaceBlocks', () => {
  const source = `# Site: My Site
  location: 49.45, 12.18

## Building: House
  type: residential

### Storey: Ground Floor
  elevation: 0.0 m
  height: 2.80 m

#### Space: Kitchen
  area: 12 m2

  Wall: W-01
    from: (0, 0) to: (4, 0)
    height: 2.80 m
    thickness: 0.24 m`;

  it('applies a single replacement', () => {
    const blocks = [{ search: 'thickness: 0.24 m', replace: 'thickness: 0.30 m' }];
    const { result, errors } = applySearchReplaceBlocks(source, blocks);
    expect(errors).toHaveLength(0);
    expect(result).toContain('thickness: 0.30 m');
    expect(result).not.toContain('thickness: 0.24 m');
  });

  it('applies multiple replacements', () => {
    const blocks = [
      { search: 'thickness: 0.24 m', replace: 'thickness: 0.30 m' },
      { search: 'area: 12 m2', replace: 'area: 15 m2' },
    ];
    const { result, errors } = applySearchReplaceBlocks(source, blocks);
    expect(errors).toHaveLength(0);
    expect(result).toContain('thickness: 0.30 m');
    expect(result).toContain('area: 15 m2');
  });

  it('reports error for unmatched search', () => {
    const blocks = [{ search: 'this does not exist in source', replace: 'something' }];
    const { result, errors } = applySearchReplaceBlocks(source, blocks);
    expect(errors).toHaveLength(1);
    expect(errors[0]).toContain('Could not find text');
    expect(result).toBe(source); // unchanged
  });

  it('can delete lines with empty replace', () => {
    const blocks = [{ search: '    thickness: 0.24 m', replace: '' }];
    const { result, errors } = applySearchReplaceBlocks(source, blocks);
    expect(errors).toHaveLength(0);
    expect(result).not.toContain('thickness');
  });
});
